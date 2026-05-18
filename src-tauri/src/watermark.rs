//! 面單列印次數浮水印
//!
//! 對應雲端 OrderPrintController 的浮水印邏輯:當 `print_num > 1` 時,
//! 在面單上印 `(N)` 字樣標示這是第幾次列印。
//!
//! - 字串:`({print_num})`
//! - 字型:Verdana Bold 16pt(視覺與雲端對齊;字型檔路徑見下)
//! - 顏色:純黑 `#000000`
//! - 位置:
//!   - 順豐速運(`shipping_provider = "E"`):右下角,距右 30、距下 50
//!   - 其他物流商:右上角,距右 15、距上 50
//!
//! 字型載入:啟動時嘗試讀 `app_data_dir/fonts/verdanab.ttf`。
//! 讀不到就停用浮水印功能並 log warn,GET /api/parcel 會 fallback 回原圖。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ab_glyph::{FontVec, PxScale};
use image::Rgba;
use parking_lot::RwLock;
use tauri::{AppHandle, Manager};

/// 順豐速運 provider 代碼(對應 cix3752iWeb 的 SP_EXPRESS)
const SP_EXPRESS: &str = "E";

#[derive(Clone)]
pub struct WatermarkRenderer {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    /// 字型 bytes(載入失敗為 None,此時 apply() 一律回 Err)
    font: Option<Arc<FontVec>>,
    /// 啟動時嘗試的字型路徑(用於錯誤訊息)
    font_path: PathBuf,
}

impl WatermarkRenderer {
    /// 建立並嘗試載入字型;不論成功失敗都回傳實例,失敗時 apply() 會回 Err
    pub fn new(handle: &AppHandle) -> Self {
        let font_path = handle
            .path()
            .app_data_dir()
            .map(|p| p.join("fonts").join("verdanab.ttf"))
            .unwrap_or_else(|_| PathBuf::from("verdanab.ttf"));

        let font = std::fs::read(&font_path)
            .ok()
            .and_then(|bytes| FontVec::try_from_vec(bytes).ok())
            .map(Arc::new);

        if font.is_some() {
            tracing::info!(?font_path, "面單浮水印字型載入成功");
        } else {
            tracing::warn!(
                ?font_path,
                "面單浮水印字型未提供,print_num>1 時將 fallback 回原圖。\
                 請放置 Verdana Bold 字型至此路徑以啟用浮水印"
            );
        }

        Self {
            inner: Arc::new(RwLock::new(Inner { font, font_path })),
        }
    }

    /// 是否可用;font 為 None 時嘗試 lazy reload 一次,字型剛放好就會自動生效
    pub fn is_enabled(&self) -> bool {
        if self.inner.read().font.is_some() {
            return true;
        }
        self.try_reload()
    }

    /// 嘗試重新讀字型檔(供 lazy retry 用)。成功回 true、失敗回 false
    fn try_reload(&self) -> bool {
        let path = self.inner.read().font_path.clone();
        let font = std::fs::read(&path)
            .ok()
            .and_then(|bytes| FontVec::try_from_vec(bytes).ok())
            .map(Arc::new);
        let loaded = font.is_some();
        if loaded {
            self.inner.write().font = font;
            tracing::info!(?path, "面單浮水印字型已 lazy 載入");
        }
        loaded
    }

    /// 對 `src` 圖加上 `(print_num)` 浮水印後寫入 `dst`
    ///
    /// 字型若未載入,回傳 Err 讓呼叫端 fallback 回原圖
    pub fn apply(
        &self,
        src: &Path,
        dst: &Path,
        print_num: u32,
        provider: &str,
    ) -> Result<(), String> {
        let font = {
            let g = self.inner.read();
            g.font.clone()
        };
        let font = match font {
            Some(f) => f,
            None => {
                if !self.try_reload() {
                    let g = self.inner.read();
                    return Err(format!("字型未載入: {}", g.font_path.display()));
                }
                self.inner.read().font.clone().expect("just reloaded")
            }
        };

        let dyn_img = image::open(src).map_err(|e| format!("讀取原圖失敗: {e}"))?;
        let mut img = dyn_img.to_rgba8();
        let (img_w, img_h) = img.dimensions();

        let text = format!("({})", print_num);
        // PHP ImageWorkshop 的 16 是 pt,換算成 px(96 DPI):16 * 96 / 72 ≈ 21.33
        let scale = PxScale::from(16.0 * 96.0 / 72.0);

        let (text_w, text_h) = imageproc::drawing::text_size(scale, font.as_ref(), &text);

        // PHP ImageWorkshop 座標慣例:RT/RB 為「文字框該邊距畫布該邊的內縮距離」
        // RT(15, 50):文字框右邊距畫布右邊 15,文字框上邊距畫布上邊 50
        // RB(30, 50):文字框右邊距畫布右邊 30,文字框下邊距畫布下邊 50
        let (x_i, y_i) = if provider == SP_EXPRESS {
            (
                img_w as i32 - 30 - text_w as i32,
                img_h as i32 - 50 - text_h as i32,
            )
        } else {
            (img_w as i32 - 15 - text_w as i32, 50)
        };

        imageproc::drawing::draw_text_mut(
            &mut img,
            Rgba([0u8, 0, 0, 255]),
            x_i.max(0),
            y_i.max(0),
            scale,
            font.as_ref(),
            &text,
        );

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("建立浮水印輸出目錄失敗: {e}"))?;
        }
        img.save(dst).map_err(|e| format!("寫入浮水印檔失敗: {e}"))?;
        Ok(())
    }
}

/// 由原 label_key 推導浮水印版本的 key
/// 例:`HCT/20260513/abc.png` + provider=`H` → `@repeat/WH-abc.png`
pub fn derive_repeat_key(label_key: &str, provider: &str) -> String {
    let basename = label_key.rsplit('/').next().unwrap_or(label_key);
    format!("@repeat/W{}-{}", provider, basename)
}
