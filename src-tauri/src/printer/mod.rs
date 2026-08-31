use serde::{Deserialize, Serialize};

use crate::AppResult;

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
/// 平台分流:
/// - macOS / Linux 走 `printers` crate → CUPS,filter chain 會自動把 PNG/JPEG
///   轉成印表機認得的格式。
/// - Windows 走 GDI(本檔 `windows_gdi` 子模組)。`printers` crate 在 Windows
///   用 `WritePrinter` + `datatype="RAW"` 把 image bytes 原封不動丟給印表機,
///   熱感 / 小票印表機(ESC-POS、TSPL)不認 PNG 檔頭,spooler 會卡在「列印中」。
///   解法:自己解 PNG → DIB → `StretchDIBits` 印出,任何 Windows 印表機驅動皆吃。
pub fn print_image_bytes(printer_name: &str, bytes: &[u8]) -> AppResult<()> {
    #[cfg(windows)]
    {
        windows_gdi::print_image_bytes(printer_name, bytes)
    }
    #[cfg(not(windows))]
    {
        unix_path::print_image_bytes(printer_name, bytes)
    }
}

#[cfg(not(windows))]
mod unix_path {
    use printers::common::base::job::PrinterJobOptions;

    use crate::{AppError, AppResult};

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
}

#[cfg(windows)]
mod windows_gdi {
    use std::ffi::c_void;
    use std::os::raw::{c_int, c_uint};

    use crate::{AppError, AppResult};

    type HDC = *mut c_void;

    #[repr(C)]
    struct DOCINFOW {
        cbSize: c_int,
        lpszDocName: *const u16,
        lpszOutput: *const u16,
        lpszDatatype: *const u16,
        fwType: u32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct BITMAPINFOHEADER {
        biSize: u32,
        biWidth: i32,
        biHeight: i32,
        biPlanes: u16,
        biBitCount: u16,
        biCompression: u32,
        biSizeImage: u32,
        biXPelsPerMeter: i32,
        biYPelsPerMeter: i32,
        biClrUsed: u32,
        biClrImportant: u32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct RGBQUAD {
        rgbBlue: u8,
        rgbGreen: u8,
        rgbRed: u8,
        rgbReserved: u8,
    }

    // 1-bit DIB 需要 2-entry palette(bit=0 → palette[0],bit=1 → palette[1])
    #[repr(C)]
    struct BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER,
        bmiColors: [RGBQUAD; 2],
    }

    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: c_uint = 0;
    const SRCCOPY: u32 = 0x00CC_0020;
    const HORZRES: c_int = 8;
    const VERTRES: c_int = 10;
    const GDI_ERROR: u32 = 0xFFFF_FFFF;

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateDCW(
            pwszDriver: *const u16,
            pwszDevice: *const u16,
            pwszPort: *const u16,
            pdm: *const c_void,
        ) -> HDC;
        fn DeleteDC(hdc: HDC) -> c_int;
        fn StartDocW(hdc: HDC, lpdi: *const DOCINFOW) -> c_int;
        fn EndDoc(hdc: HDC) -> c_int;
        fn AbortDoc(hdc: HDC) -> c_int;
        fn StartPage(hdc: HDC) -> c_int;
        fn EndPage(hdc: HDC) -> c_int;
        fn GetDeviceCaps(hdc: HDC, index: c_int) -> c_int;
        fn StretchDIBits(
            hdc: HDC,
            xDest: c_int,
            yDest: c_int,
            wDest: c_int,
            hDest: c_int,
            xSrc: c_int,
            ySrc: c_int,
            wSrc: c_int,
            hSrc: c_int,
            lpBits: *const c_void,
            lpbmi: *const BITMAPINFO,
            iUsage: c_uint,
            rop: u32,
        ) -> c_int;
    }

    /// 列印 image bytes
    ///
    /// 策略:**pre-scale + 1-bit B&W bottom-up DIB**(XP-460B / EPSON L6190 實機驗證過的組合)。
    /// 三個條件缺一都會卡 Windows spooler,不可為了「單純一點」改回去:
    /// - **1-bit(非 24-bit BGR)**:24-bit 對 Print to PDF 等虛擬印表機正常,對實體 driver 會在
    ///   raster expansion + 色彩空間轉換階段 hang。1-bit 讓 raster 縮到 1/24(800×1200:
    ///   2.88MB → 120KB),driver 不必轉色彩空間、送 USB 的資料量也極小
    /// - **bottom-up(`biHeight = +ih`)**:老 driver 對 top-down DIB 會卡
    /// - **pre-scale 到印表機物理 DPI**:`StretchDIBits` dest=src 不縮放,driver 不必再 scale
    pub fn print_image_bytes(printer_name: &str, bytes: &[u8]) -> AppResult<()> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| AppError::Printer(format!("圖片解碼失敗: {e}")))?;
        let luma = img.to_luma8();
        let (iw, ih) = (luma.width() as i32, luma.height() as i32);
        if iw <= 0 || ih <= 0 {
            return Err(AppError::Printer("圖片尺寸無效".into()));
        }

        let printer_w: Vec<u16> = printer_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let doc_name: Vec<u16> = "cix3752i Label"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let hdc = CreateDCW(
                std::ptr::null(),
                printer_w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if hdc.is_null() {
                return Err(AppError::Printer(format!(
                    "開啟印表機失敗 (CreateDCW): {printer_name}"
                )));
            }

            let page_w = GetDeviceCaps(hdc, HORZRES);
            let page_h = GetDeviceCaps(hdc, VERTRES);
            if page_w <= 0 || page_h <= 0 {
                DeleteDC(hdc);
                return Err(AppError::Printer(
                    "讀不到印表機可印區域 (GetDeviceCaps)".into(),
                ));
            }

            // 等比例縮到 printer 可印 pixel 區
            let ratio = (page_w as f64 / iw as f64).min(page_h as f64 / ih as f64);
            let tw = ((iw as f64) * ratio).round().max(1.0) as u32;
            let th = ((ih as f64) * ratio).round().max(1.0) as u32;
            let resized = image::imageops::resize(
                &luma,
                tw,
                th,
                image::imageops::FilterType::Triangle,
            );

            // pack 成 1-bit packed DIB(bit=1 黑,bit=0 白;MSB 為左邊像素;bottom-up)
            let tw_i = tw as i32;
            let th_i = th as i32;
            let row_bytes = ((tw as usize) + 7) / 8;
            let stride = (row_bytes + 3) & !3;
            let mut bits: Vec<u8> = Vec::with_capacity(stride * th as usize);
            for y in (0..th as usize).rev() {
                let row_start = y * tw as usize;
                let row = &resized.as_raw()[row_start..row_start + tw as usize];
                let mut byte: u8 = 0;
                let mut bit_count: u32 = 0;
                let mut wrote: usize = 0;
                for &px in row {
                    let bit: u8 = if px < 128 { 1 } else { 0 };
                    byte = (byte << 1) | bit;
                    bit_count += 1;
                    if bit_count == 8 {
                        bits.push(byte);
                        byte = 0;
                        bit_count = 0;
                        wrote += 1;
                    }
                }
                if bit_count > 0 {
                    byte <<= 8 - bit_count;
                    bits.push(byte);
                    wrote += 1;
                }
                bits.resize(bits.len() + (stride - wrote), 0);
            }

            let dx = (page_w - tw_i) / 2;
            let dy = (page_h - th_i) / 2;

            let docinfo = DOCINFOW {
                cbSize: std::mem::size_of::<DOCINFOW>() as c_int,
                lpszDocName: doc_name.as_ptr(),
                lpszOutput: std::ptr::null(),
                lpszDatatype: std::ptr::null(),
                fwType: 0,
            };
            if StartDocW(hdc, &docinfo) <= 0 {
                DeleteDC(hdc);
                return Err(AppError::Printer("StartDocW 失敗".into()));
            }
            if StartPage(hdc) <= 0 {
                EndDoc(hdc);
                DeleteDC(hdc);
                return Err(AppError::Printer("StartPage 失敗".into()));
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: tw_i,
                    biHeight: th_i, // bottom-up(正值)
                    biPlanes: 1,
                    biBitCount: 1,
                    biCompression: BI_RGB,
                    biSizeImage: (stride * th as usize) as u32,
                    biClrUsed: 2,
                    ..Default::default()
                },
                bmiColors: [
                    RGBQUAD { rgbBlue: 0xFF, rgbGreen: 0xFF, rgbRed: 0xFF, rgbReserved: 0 }, // 白
                    RGBQUAD { rgbBlue: 0x00, rgbGreen: 0x00, rgbRed: 0x00, rgbReserved: 0 }, // 黑
                ],
            };

            // dest 與 src 同尺寸:driver 不必再做 raster scaling
            let scanlines = StretchDIBits(
                hdc,
                dx, dy, tw_i, th_i,
                0, 0, tw_i, th_i,
                bits.as_ptr() as *const c_void,
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            let stretch_failed = scanlines == 0 || (scanlines as u32) == GDI_ERROR;

            // 影像沒成功畫上就不能提交:AbortDoc 丟棄整份文件(不送空白頁進 spooler),
            // 而非 EndPage/EndDoc(那會把半張/空白頁 commit 給印表機)
            if stretch_failed {
                AbortDoc(hdc);
                DeleteDC(hdc);
                return Err(AppError::Printer("StretchDIBits 失敗".into()));
            }

            let end_page_ok = EndPage(hdc) > 0;
            let end_doc_ok = EndDoc(hdc) > 0;
            DeleteDC(hdc);

            if !end_page_ok {
                return Err(AppError::Printer("EndPage 失敗".into()));
            }
            if !end_doc_ok {
                return Err(AppError::Printer("EndDoc 失敗".into()));
            }
        }

        Ok(())
    }
}
