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
#[tauri::command]
pub async fn print_image(
    _state: State<'_, SharedState>,
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
        (None, Some(path)) => std::fs::read(&path)
            .map_err(|e| AppError::Printer(format!("讀取 {path} 失敗: {e}")))?,
        (None, None) => {
            return Err(AppError::Printer("缺少 image_base64 或 image_path".into()))
        }
    };

    printer::print_image_bytes(&req.printer_name, &bytes)
}
