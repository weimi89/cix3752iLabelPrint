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
        // 原子寫入(見 crate::fs_atomic):寫同目錄臨時檔再 rename 覆蓋 dst,避免 /images 服務在
        // 編碼過程中讀到半寫入的截斷檔(printer 端解碼報 "unexpected end of file")。
        // 輸出格式由 dst 副檔名決定(無法判斷則回錯,對齊原 img.save 行為);臨時檔名與副檔名無關,
        // 故用 save_with_format 明確指定格式寫入臨時檔。
        let fmt = image::ImageFormat::from_path(dst)
            .map_err(|e| format!("無法由副檔名判斷浮水印輸出格式: {e}"))?;
        let tmp = crate::fs_atomic::sibling_tmp(dst);
        if let Err(e) = img.save_with_format(&tmp, fmt) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("寫入浮水印檔失敗: {e}"));
        }
        if let Err(e) = crate::fs_atomic::rename_with_retry_sync(&tmp, dst) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("覆蓋浮水印檔失敗: {e}"));
        }
        Ok(())
    }
}

impl Default for WatermarkRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// 由原 label_key 推導浮水印版本的 key
///
/// **必須保留 `label_key` 的完整子路徑唯一性**:主 key(`derive_label_key`)是靠
/// `物流商/日期/檔名` 整條相對路徑來確保唯一,若這裡只取最後一段檔名,
/// 不同日期、同檔名的兩張面單會推出同一個 repeat_key → 浮水印圖互相覆蓋/讀錯(找錯面單圖)。
/// 因此把子路徑分隔 `/` 攤平成 `__` 併進檔名,讓 repeat_key 與主 key 一樣全域唯一,
/// 同時維持 `@repeat/` 單層目錄結構(清理/過期邏輯依此運作)。
///
/// 例:`HCT/20260513/abc.png` + provider=`H` → `@repeat/WH-HCT__20260513__abc.png`
pub fn derive_repeat_key(label_key: &str, provider: &str) -> String {
    let flat = label_key.trim_start_matches('/').replace('/', "__");
    format!("@repeat/W{}-{}", provider, flat)
}

#[cfg(test)]
mod tests {
    use super::derive_repeat_key;

    /// 回歸:同物流商、不同日期、**同檔名**的兩張面單,repeat_key 必須不同。
    /// 舊實作只取 `rsplit('/').next()`(檔名)→ 兩者都是 `@repeat/WH-abc.png` 碰撞,
    /// 浮水印圖互相覆蓋,工控機/UI 拿到他單的 `(N)` 面單(找錯面單圖)。
    #[test]
    fn repeat_key_distinct_across_dates_same_filename() {
        let a = derive_repeat_key("HCT/20260513/abc.png", "H");
        let b = derive_repeat_key("HCT/20260614/abc.png", "H");
        assert_ne!(a, b, "跨日同檔名的 repeat_key 不可碰撞");
        assert_eq!(a, "@repeat/WH-HCT__20260513__abc.png");
        assert_eq!(b, "@repeat/WH-HCT__20260614__abc.png");
    }

    /// 不同物流商即使 label_key 相同也要區隔(provider 進前綴)。
    #[test]
    fn repeat_key_distinct_across_providers() {
        let h = derive_repeat_key("X/20260513/abc.png", "H");
        let e = derive_repeat_key("X/20260513/abc.png", "E");
        assert_ne!(h, e);
    }

    /// 維持 `@repeat/` 單層目錄(清理/過期邏輯依此),子路徑攤平不再產生巢狀資料夾。
    #[test]
    fn repeat_key_stays_single_level_under_at_repeat() {
        let k = derive_repeat_key("HCT/20260513/abc.png", "H");
        assert!(k.starts_with("@repeat/"));
        // @repeat 之後不應再有 '/'(子路徑已攤平成 __)
        assert!(!k["@repeat/".len()..].contains('/'));
    }
}
