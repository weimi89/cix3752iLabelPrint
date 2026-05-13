use printers::common::base::job::PrinterJobOptions;
use serde::{Deserialize, Serialize};

use crate::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPrinter {
    pub name: String,
    pub system_name: String,
    pub driver_name: Option<String>,
    pub is_default: bool,
    pub state: String,
}

/// 列出本機印表機
pub fn list_printers() -> Vec<LocalPrinter> {
    printers::get_printers()
        .into_iter()
        .map(|p| LocalPrinter {
            name: p.name.clone(),
            system_name: p.system_name.clone(),
            driver_name: Some(p.driver_name.clone()).filter(|s| !s.is_empty()),
            is_default: p.is_default,
            state: format!("{:?}", p.state),
        })
        .collect()
}

/// 印出單張圖（傳 image bytes 給系統印表機）
///
/// macOS / Linux 走 lpr / CUPS；Windows 走 winspool。實際的尺寸 / 縮放由
/// 印表機驅動處理；後續可在這裡擴充 paper size、orientation 等選項。
pub fn print_image_bytes(printer_name: &str, bytes: &[u8]) -> AppResult<()> {
    let printers = printers::get_printers();
    let printer = printers
        .iter()
        .find(|p| p.name == printer_name || p.system_name == printer_name)
        .ok_or_else(|| AppError::Printer(format!("找不到印表機: {printer_name}")))?;

    printer
        .print(bytes, PrinterJobOptions::none())
        .map_err(|e| AppError::Printer(format!("送印失敗: {e:?}")))?;

    Ok(())
}
