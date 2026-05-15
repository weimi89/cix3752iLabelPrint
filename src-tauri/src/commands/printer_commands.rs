use base64::Engine;
use serde::Deserialize;
use tauri::State;

use crate::printer::{self, LocalPrinter};
use crate::{AppError, AppResult, SharedState};

#[derive(Debug, Deserialize)]
pub struct PrintImageRequest {
    pub printer_name: String,
    /// 二擇一：base64 影像，或本地檔案路徑
    #[serde(default)]
    pub image_base64: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

/// 列出本機可用印表機
#[tauri::command]
pub async fn list_printers(_state: State<'_, SharedState>) -> AppResult<Vec<LocalPrinter>> {
    Ok(printer::list_printers())
}

/// 印出單張圖
///
/// image_path 支援三種來源:
/// - http:// / https:// → 透過 cloud http client 下載 bytes（自動套用 SSL 設定）
/// - file:// → 去掉前綴後讀本地檔
/// - 一般路徑 → 直接讀本地檔
#[tauri::command]
pub async fn print_image(
    state: State<'_, SharedState>,
    req: PrintImageRequest,
) -> AppResult<()> {
    let bytes = match (req.image_base64, req.image_path) {
        (Some(b64), _) => {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| AppError::Printer(format!("base64 decode 失敗: {e}")))?;
            image::guess_format(&raw)
                .map_err(|e| AppError::Printer(format!("非法圖片格式: {e}")))?;
            raw
        }
        (None, Some(path)) => {
            if path.starts_with("http://") || path.starts_with("https://") {
                state.cloud.fetch_image_bytes(&path).await
                    .map_err(|e| AppError::Printer(format!("下載 {path} 失敗: {e}")))?
            } else {
                let local = path.strip_prefix("file://").unwrap_or(&path);
                std::fs::read(local)
                    .map_err(|e| AppError::Printer(format!("讀取 {local} 失敗: {e}")))?
            }
        }
        (None, None) => {
            return Err(AppError::Printer("缺少 image_base64 或 image_path".into()))
        }
    };

    printer::print_image_bytes(&req.printer_name, &bytes)
}
