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

    #[repr(C)]
    struct BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER,
        bmiColors: [RGBQUAD; 1],
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

    pub fn print_image_bytes(printer_name: &str, bytes: &[u8]) -> AppResult<()> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| AppError::Printer(format!("圖片解碼失敗: {e}")))?;
        let rgba = img.to_rgba8();
        let (iw, ih) = (rgba.width() as i32, rgba.height() as i32);
        if iw <= 0 || ih <= 0 {
            return Err(AppError::Printer("圖片尺寸無效".into()));
        }

        // GDI DIB 用 BGRA 順序;header.biHeight 設負值表示 top-down,免再翻轉列。
        let mut bgra: Vec<u8> = rgba.into_raw();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
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
            let (dx, dy, dw, dh) = fit(iw, ih, page_w, page_h);

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
                    biWidth: iw,
                    biHeight: -ih,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: (iw * ih * 4) as u32,
                    ..Default::default()
                },
                bmiColors: [RGBQUAD::default()],
            };

            let scanlines = StretchDIBits(
                hdc,
                dx,
                dy,
                dw,
                dh,
                0,
                0,
                iw,
                ih,
                bgra.as_ptr() as *const c_void,
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );

            EndPage(hdc);
            EndDoc(hdc);
            DeleteDC(hdc);

            if scanlines == 0 || (scanlines as u32) == GDI_ERROR {
                return Err(AppError::Printer("StretchDIBits 失敗".into()));
            }
        }

        Ok(())
    }

    /// 把 img 等比例縮放至 page 內並置中,回傳 (dx, dy, dw, dh)。
    fn fit(iw: i32, ih: i32, pw: i32, ph: i32) -> (i32, i32, i32, i32) {
        let r = (pw as f64 / iw as f64).min(ph as f64 / ih as f64);
        let dw = ((iw as f64) * r).round() as i32;
        let dh = ((ih as f64) * r).round() as i32;
        let dx = (pw - dw) / 2;
        let dy = (ph - dh) / 2;
        (dx, dy, dw, dh)
    }
}
