//! 面單列印次數浮水印
//!
//! 對應雲端 OrderPrintController 的浮水印邏輯:當 `print_num > 1` 時,
//! 在面單上印 `(N)` 字樣標示這是第幾次列印。
//!
//! - 字串:`({print_num})`
//! - 字型:DejaVu Sans Bold 16pt(OFL 授權,編譯期內嵌進 binary,零部署摩擦)
//! - 顏色:純黑 `#000000`
//! - 位置:
//!   - 順豐速運(`shipping_provider = "E"`):右下角,距右 30、距下 50
//!   - 其他物流商:右上角,距右 15、距上 50

use std::path::Path;
use std::sync::Arc;

use ab_glyph::{FontVec, PxScale};
use image::Rgba;

/// 順豐速運 provider 代碼(對應 cix3752iWeb 的 SP_EXPRESS)
const SP_EXPRESS: &str = "E";

/// 編譯期內嵌的浮水印字型(DejaVu Sans Bold, OFL 授權)
const FONT_BYTES: &[u8] =
    include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");

#[derive(Clone)]
pub struct WatermarkRenderer {
    font: Arc<FontVec>,
}

impl WatermarkRenderer {
    /// 建立 renderer;字型已編譯期嵌入,解析失敗代表 build artifact 損毀,直接 panic
    pub fn new() -> Self {
        let font = FontVec::try_from_vec(FONT_BYTES.to_vec())
            .expect("內嵌浮水印字型 DejaVuSans-Bold.ttf 解析失敗(build artifact 損毀)");
        Self { font: Arc::new(font) }
    }

    /// 對 `src` 圖加上 `(print_num)` 浮水印後寫入 `dst`
    pub fn apply(
        &self,
        src: &Path,
        dst: &Path,
        print_num: u32,
        provider: &str,
    ) -> Result<(), String> {
        let dyn_img = image::open(src).map_err(|e| format!("讀取原圖失敗: {e}"))?;
        let mut img = dyn_img.to_rgba8();
        let (img_w, img_h) = img.dimensions();

        let text = format!("({})", print_num);
        // PHP ImageWorkshop 的 16 是 pt,換算成 px(96 DPI):16 * 96 / 72 ≈ 21.33
        let scale = PxScale::from(16.0 * 96.0 / 72.0);

        let (text_w, text_h) = imageproc::drawing::text_size(scale, self.font.as_ref(), &text);

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
            self.font.as_ref(),
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

impl Default for WatermarkRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// 由原 label_key 推導浮水印版本的 key
/// 例:`HCT/20260513/abc.png` + provider=`H` → `@repeat/WH-abc.png`
pub fn derive_repeat_key(label_key: &str, provider: &str) -> String {
    let basename = label_key.rsplit('/').next().unwrap_or(label_key);
    format!("@repeat/W{}-{}", provider, basename)
}
