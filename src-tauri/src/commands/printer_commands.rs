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

/// 入倉裝箱標籤一張(對齊雲端 WarehouseScannerService::generateLabelData 回傳欄位)
#[derive(Debug, Deserialize)]
pub struct BoxLabel {
    #[serde(default)]
    pub package_sn: String,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub serial_number: String,
    #[serde(default)]
    pub package_date: String,
    #[serde(default)]
    pub warehouse_name: String,
    #[serde(default)]
    pub package_remarks: String,
}

#[derive(Debug, Deserialize)]
pub struct WarehousePrintLabelsRequest {
    pub printer_name: String,
    pub labels: Vec<BoxLabel>,
}

/// 入倉驗單:箱標本地列印(由前端先打雲端 label-data 取回箱標資料,再逐張渲染 PNG 走本地印表機)。
/// 任一張失敗即中止並回錯誤,讓現場人員知道是哪台/哪張沒印出。
#[tauri::command]
pub async fn warehouse_print_labels(
    _state: State<'_, SharedState>,
    req: WarehousePrintLabelsRequest,
) -> AppResult<u32> {
    if req.printer_name.trim().is_empty() {
        return Err(AppError::Printer("未選擇印表機".into()));
    }
    if req.labels.is_empty() {
        return Err(AppError::Printer("無箱標資料可列印".into()));
    }

    let mut printed = 0u32;
    for label in &req.labels {
        let bytes = crate::error_label::generate_box_label(
            &label.provider_name,
            &label.serial_number,
            &label.package_date,
            &label.package_sn,
            &label.warehouse_name,
            &label.package_remarks,
        );
        if bytes.is_empty() {
            return Err(AppError::Printer(format!("箱標 {} 影像生成失敗", label.package_sn)));
        }
        printer::print_image_bytes(&req.printer_name, &bytes)
            .map_err(|e| AppError::Printer(format!("箱標 {} 列印失敗: {e}", label.package_sn)))?;
        printed += 1;
    }
    Ok(printed)
}
