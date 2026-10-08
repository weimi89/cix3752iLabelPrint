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
    print_image_bytes_tracked(printer_name, bytes).map(|_| ())
}

/// 同 [`print_image_bytes`],另外回傳 Windows 列印佇列給這張的工作編號(其他平台回 None)。
///
/// 送印成功只代表 Windows 收下了這張,不代表印表機已經印出來 —— 缺紙、卡紙、離線時
/// 工作會留在佇列,換好紙後一次吐出。呼叫端用這個編號配合 [`inspect`] 追蹤它有沒有真的印完。
pub fn print_image_bytes_tracked(printer_name: &str, bytes: &[u8]) -> AppResult<Option<u32>> {
    #[cfg(windows)]
    {
        windows_gdi::print_image_bytes(printer_name, bytes).map(Some)
    }
    #[cfg(not(windows))]
    {
        unix_path::print_image_bytes(printer_name, bytes).map(|_| None)
    }
}

/// 印表機狀態旗標裡代表「現在印不出來」的那幾種,轉成現場看得懂的原因。
/// 旗標值對應 Windows winspool.h 的 `PRINTER_STATUS_*`;同時有多種時回第一種。
/// 忙碌、列印中、預熱、碳粉偏低這類「還能印」的狀態不算。
#[cfg_attr(not(windows), allow(dead_code))]
pub fn status_problem(status: u32) -> Option<&'static str> {
    const FLAGS: [(u32, &str); 11] = [
        (0x0000_0010, "缺紙"),
        (0x0000_0008, "卡紙"),
        (0x0000_0040, "紙張異常"),
        (0x0040_0000, "機蓋沒關"),
        (0x0000_0080, "離線"),
        (0x0000_1000, "無法使用"),
        (0x0000_0800, "出紙口滿了"),
        (0x0004_0000, "沒有色帶或碳粉"),
        (0x0010_0000, "需要人員處理"),
        (0x0000_0001, "被暫停"),
        (0x0000_0002, "發生錯誤"),
    ];
    FLAGS.iter().find(|(bit, _)| status & bit != 0).map(|(_, why)| *why)
}

/// 一次檢查的結果
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Inspection {
    /// 印表機狀態顯示印不出來的原因;讀不到狀態時為 None(不因讀不到就判定故障)
    pub problem: Option<&'static str>,
    /// 追蹤中、已經不在佇列裡的工作(印完了)
    pub gone: Vec<u32>,
    /// 卡太久、這次被取消的工作 —— 這些面單**沒有**印出來
    pub cancelled: Vec<u32>,
    /// 卡太久但沒有取消的工作:取消失敗,或卡住時正在列印(取消也收不回已送進印表機的資料,
    /// 可能其實已經印出)。這些的實體結果不確定,印表機恢復後也可能吐出
    pub stuck: Vec<u32>,
    /// 這次查不出狀態的工作(列印服務暫時沒回應等):不當成印完,也不當成卡住
    pub unknown: Vec<u32>,
}

/// 一張追蹤中的面單等了多久、該不該當成卡住處理(由呼叫端依「多久沒進展」決定)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// 還在正常等待
    Fresh,
    /// 等太久了,但只有印表機自己回報缺紙、卡紙、離線等異常時才當成卡住
    IfProblem,
    /// 等太久了,不論印表機有沒有回報異常都當成卡住
    Now,
}

impl Staleness {
    /// 依這次讀到的印表機狀態,決定這張要不要當成卡住(只有 Windows 查得到列印佇列)
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn is_stale(self, printer_reports_problem: bool) -> bool {
        match self {
            Staleness::Fresh => false,
            Staleness::IfProblem => printer_reports_problem,
            Staleness::Now => true,
        }
    }
}

/// 查印表機狀態,並逐一確認追蹤中的工作:不在佇列或已標印完 → 印完了;查不出來 → 放進 `unknown`。
/// 判定卡住([`Staleness::is_stale`])的工作還在佇列時才取消 —— 但只要這次有任何一張印完(印表機有進展),整批都不取消:
/// 呼叫端的「多久沒進展」是查詢前算的,worker 忙著下載時可能好一陣子沒查,不能憑舊的判斷砍掉正常排隊的面單。
/// 正在列印的那張不取消,放進 `stuck`(取消收不回已送進印表機的資料,硬砍可能讓現場以為沒印而重印)。
/// `tracked` 是 (工作編號, 等待程度)。非 Windows 平台一律回「正常、全部印完」。
pub fn inspect(printer_name: &str, tracked: &[(u32, Staleness)]) -> AppResult<Inspection> {
    #[cfg(windows)]
    {
        windows_spool::inspect(printer_name, tracked)
    }
    #[cfg(not(windows))]
    {
        let _ = printer_name;
        Ok(Inspection {
            gone: tracked.iter().map(|(id, _)| *id).collect(),
            ..Default::default()
        })
    }
}

#[cfg(windows)]
mod windows_spool {
    use std::ffi::c_void;
    use std::os::raw::c_int;

    use super::{status_problem, Inspection, Staleness};
    use crate::{AppError, AppResult};

    type HANDLE = *mut c_void;
    type BOOL = c_int;

    /// GetJobW 對已不存在的工作編號回的錯誤碼
    const ERROR_INVALID_PARAMETER: u32 = 87;
    const JOB_CONTROL_DELETE: u32 = 5;
    /// 工作已印完。印表機若勾了「保留列印過的文件」,印完的工作會留在佇列,要靠這兩個旗標認出來,
    /// 否則會被誤判成卡住、把已印的件改回未印
    const JOB_STATUS_PRINTED: u32 = 0x0000_0080;
    const JOB_STATUS_COMPLETE: u32 = 0x0000_1000;
    const JOB_STATUS_PRINTING: u32 = 0x0000_0010;

    #[repr(C)]
    struct SYSTEMTIME {
        wYear: u16,
        wMonth: u16,
        wDayOfWeek: u16,
        wDay: u16,
        wHour: u16,
        wMinute: u16,
        wSecond: u16,
        wMilliseconds: u16,
    }

    /// winspool.h 的 JOB_INFO_1W(只讀 Status,其餘欄位為了位置正確照抄)
    #[repr(C)]
    pub(super) struct JOB_INFO_1W {
        JobId: u32,
        pPrinterName: *mut u16,
        pMachineName: *mut u16,
        pUserName: *mut u16,
        pDocument: *mut u16,
        pDatatype: *mut u16,
        pStatus: *mut u16,
        pub(super) Status: u32,
        Priority: u32,
        Position: u32,
        TotalPages: u32,
        PagesPrinted: u32,
        Submitted: SYSTEMTIME,
    }

    /// GetJobW 第一次(不給緩衝)在工作還在時回的錯誤碼
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    enum JobState {
        /// 已不在佇列,或還在但已標印完(保留列印過的文件)
        Done,
        /// 還在佇列、尚未開始列印
        Queued,
        /// 正在列印
        Printing,
        /// 這次查不出來
        Unknown,
    }

    unsafe fn job_state(h: HANDLE, job_id: u32) -> JobState {
        let mut needed: u32 = 0;
        if GetJobW(h, job_id, 1, std::ptr::null_mut(), 0, &mut needed) == 0 {
            match GetLastError() {
                ERROR_INVALID_PARAMETER => return JobState::Done,
                ERROR_INSUFFICIENT_BUFFER => {}
                _ => return JobState::Unknown,
            }
        }
        if (needed as usize) < std::mem::size_of::<JOB_INFO_1W>() {
            return JobState::Unknown;
        }
        // 用 u64 配置確保對齊;GetJobW 會把字串接在結構後面,所以要給足 needed 個位元組
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        if GetJobW(h, job_id, 1, buf.as_mut_ptr().cast(), needed, &mut needed) == 0 {
            return if GetLastError() == ERROR_INVALID_PARAMETER { JobState::Done } else { JobState::Unknown };
        }
        let status = (*(buf.as_ptr() as *const JOB_INFO_1W)).Status;
        if status & (JOB_STATUS_PRINTED | JOB_STATUS_COMPLETE) != 0 {
            JobState::Done
        } else if status & JOB_STATUS_PRINTING != 0 {
            JobState::Printing
        } else {
            JobState::Queued
        }
    }

    #[link(name = "winspool")]
    extern "system" {
        fn OpenPrinterW(pPrinterName: *const u16, phPrinter: *mut HANDLE, pDefault: *const c_void) -> BOOL;
        fn ClosePrinter(hPrinter: HANDLE) -> BOOL;
        fn GetPrinterW(hPrinter: HANDLE, Level: u32, pPrinter: *mut u8, cbBuf: u32, pcbNeeded: *mut u32) -> BOOL;
        fn GetJobW(hPrinter: HANDLE, JobId: u32, Level: u32, pJob: *mut u8, cbBuf: u32, pcbNeeded: *mut u32) -> BOOL;
        fn SetJobW(hPrinter: HANDLE, JobId: u32, Level: u32, pJob: *const u8, Command: u32) -> BOOL;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
    }

    pub fn inspect(printer_name: &str, tracked: &[(u32, Staleness)]) -> AppResult<Inspection> {
        let name: Vec<u16> = printer_name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut out = Inspection::default();
        unsafe {
            let mut h: HANDLE = std::ptr::null_mut();
            if OpenPrinterW(name.as_ptr(), &mut h, std::ptr::null()) == 0 {
                let code = GetLastError();
                return Err(AppError::Printer(format!(
                    "開啟印表機失敗 (OpenPrinterW,錯誤碼 {code}): {printer_name}"
                )));
            }

            // PRINTER_INFO_6 只有一個 DWORD(dwStatus),不必配置可變長度緩衝
            let mut status: u32 = 0;
            let mut needed: u32 = 0;
            if GetPrinterW(h, 6, (&mut status as *mut u32).cast(), std::mem::size_of::<u32>() as u32, &mut needed) != 0 {
                out.problem = status_problem(status);
            }

            let problem = out.problem.is_some();
            let states: Vec<(u32, bool, JobState)> =
                tracked.iter().map(|&(id, level)| (id, level.is_stale(problem), job_state(h, id))).collect();
            let progressed = states.iter().any(|(_, _, st)| matches!(st, JobState::Done));
            for (job_id, stale, state) in states {
                match state {
                    JobState::Done => out.gone.push(job_id),
                    JobState::Unknown => out.unknown.push(job_id),
                    _ if !stale || progressed => {}
                    JobState::Printing => out.stuck.push(job_id),
                    JobState::Queued => {
                        if SetJobW(h, job_id, 0, std::ptr::null(), JOB_CONTROL_DELETE) != 0 {
                            out.cancelled.push(job_id);
                        } else {
                            out.stuck.push(job_id);
                        }
                    }
                }
            }

            ClosePrinter(h);
        }
        Ok(out)
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
    pub fn print_image_bytes(printer_name: &str, bytes: &[u8]) -> AppResult<u32> {
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
            // StartDocW 成功時回的就是這張在列印佇列裡的工作編號
            let job_id = StartDocW(hdc, &docinfo);
            if job_id <= 0 {
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
            Ok(job_id as u32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{status_problem, Staleness};

    #[test]
    fn staleness_needs_a_reported_problem_unless_overdue_for_good() {
        assert!(!Staleness::Fresh.is_stale(true));
        assert!(!Staleness::IfProblem.is_stale(false), "印表機沒回報異常,先不當卡住");
        assert!(Staleness::IfProblem.is_stale(true), "印表機回報缺紙等異常就當卡住");
        assert!(Staleness::Now.is_stale(false));
    }

    #[test]
    fn status_flags_that_stop_printing_are_reported() {
        assert_eq!(status_problem(0), None);
        assert_eq!(status_problem(0x10), Some("缺紙"));
        assert_eq!(status_problem(0x08), Some("卡紙"));
        assert_eq!(status_problem(0x80), Some("離線"));
        // 同時缺紙又錯誤:講最具體的缺紙
        assert_eq!(status_problem(0x10 | 0x02), Some("缺紙"));
    }

    #[test]
    fn busy_printing_or_warming_up_is_not_a_problem() {
        // 忙碌 0x200、列印中 0x400、預熱 0x10000、碳粉偏低 0x20000、省電 0x1000000
        for ok in [0x200u32, 0x400, 0x1_0000, 0x2_0000, 0x100_0000, 0x400 | 0x200] {
            assert_eq!(status_problem(ok), None, "{ok:#x}");
        }
    }
}
