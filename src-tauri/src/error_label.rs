//! 工控機查詢失敗時自動產生的錯誤面單圖像
//!
//! 支援兩種紙張（203 DPI）：
//!   100mm × 150mm → 800 × 1200 px（預設）
//!   100mm × 100mm → 800 ×  800 px
//!
//! 版面：上段條碼 + 查詢號 → 中段大圖示 → 下段中文（上）越南文（下）
//!
//! 圖示依錯誤狀態個別設計：
//!   STORE_CLOSED  → ⊗ 圓圈內叉叉（封閉/拒絕）
//!   UNCONFIRMED   → ？ 巨型問號（需確認）
//!   NOT_FOUND     → 放大鏡 + 叉叉（找不到）
//!   NOT_PROXY     → ⊘ 圓圈斜線右上→左下（非代寄，斜線方向 /）
//!   NOT_FORWARD   → ⊘ 圓圈斜線左上→右下（非轉寄，斜線方向 \）
//!   ABNORMAL      → 方框內驚嘆號（包裹異常）
//!   ERROR/其他    → ⚠ 三角形 + 驚嘆號（一般警告）
//!
//! CJK 字型從系統動態載入（Windows: msjh.ttc / macOS: PingFang.ttc）；
//! 找不到時中文以 error_code ASCII 代替，越南文仍由內嵌 DejaVu 正常顯示。

use std::sync::OnceLock;

use ab_glyph::{FontVec, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::point::Point;
use imageproc::rect::Rect;

// ── 全域常數 ────────────────────────────────────────────────────────────────

const BK: Rgba<u8>   = Rgba([0u8, 0, 0, 255]);
const WH: Rgba<u8>   = Rgba([255u8, 255, 255, 255]);
const GRAY: Rgba<u8> = Rgba([160u8, 160, 160, 255]);

const DEJAVU_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");

static DEJAVU: OnceLock<FontVec> = OnceLock::new();
static CJK: OnceLock<Option<FontVec>> = OnceLock::new();

// ── 字型載入 ────────────────────────────────────────────────────────────────

fn dejavu() -> &'static FontVec {
    DEJAVU.get_or_init(|| {
        FontVec::try_from_vec(DEJAVU_BYTES.to_vec())
            .expect("DejaVu 字型解析失敗")
    })
}

fn cjk_font() -> Option<&'static FontVec> {
    CJK.get_or_init(|| {
        #[cfg(target_os = "windows")]
        let candidates: &[&str] = &[
            "C:\\Windows\\Fonts\\msjh.ttc",
            "C:\\Windows\\Fonts\\msjhl.ttc",
            "C:\\Windows\\Fonts\\kaiu.ttf",
            "C:\\Windows\\Fonts\\mingliu.ttc",
        ];
        #[cfg(target_os = "macos")]
        let candidates: &[&str] = &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/Library/Fonts/Microsoft/MicrosoftJhengHei.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode MS.ttf",
        ];
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let candidates: &[&str] = &[
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        ];

        for path in candidates {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(f) = FontVec::try_from_vec_and_index(bytes.clone(), 0) {
                    tracing::info!(path, "error_label: 已載入 CJK 字型");
                    return Some(f);
                }
                if let Ok(f) = FontVec::try_from_vec(bytes) {
                    tracing::info!(path, "error_label: 已載入 CJK 字型（單 face）");
                    return Some(f);
                }
            }
        }
        tracing::warn!("error_label: 找不到系統 CJK 字型，中文以 error_code 代替");
        None
    })
    .as_ref()
}

// ── 錯誤文字對照 ─────────────────────────────────────────────────────────────

fn error_texts(code: &str) -> (&'static str, &'static str) {
    match code {
        "STORE_CLOSED"  => ("門市關轉",    "Cửa hàng đã đóng/chuyển"),
        "UNCONFIRMED"   => ("訂單未確認",  "Đơn chưa xác nhận"),
        "NOT_FOUND"     => ("找不到包裹",  "Không tìm thấy kiện hàng"),
        "NOT_PROXY"     => ("非代寄訂單",  "Không phải đơn ủy thác"),
        "NOT_FORWARD"   => ("非轉寄訂單",  "Không phải đơn chuyển tiếp"),
        "ABNORMAL" | "STATUS_ABNORMAL" => ("包裹異常", "Kiện hàng bất thường"),
        _               => ("面單請求失敗","Yêu cầu nhãn thất bại"),
    }
}

// ── 繪圖輔助函式 ──────────────────────────────────────────────────────────────

/// 繪製有厚度的直線（垂直偏移多條細線）
fn draw_thick_line(img: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, width: f32) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 { return; }
    let nx = -dy / len;
    let ny = dx / len;
    let half = width / 2.0;
    let passes = (width as i32 * 2).max(2);
    for i in 0..=passes {
        let t = -half + width * i as f32 / passes as f32;
        imageproc::drawing::draw_line_segment_mut(
            img,
            (x0 + nx * t, y0 + ny * t),
            (x1 + nx * t, y1 + ny * t),
            BK,
        );
    }
}

/// 繪製有厚度的空心圓
fn draw_thick_circle(img: &mut RgbaImage, cx: i32, cy: i32, r: i32, t: i32) {
    for dr in 0..=t {
        let ri = (r - dr).max(0);
        imageproc::drawing::draw_hollow_circle_mut(img, (cx, cy), ri, BK);
    }
}

/// 安全填色矩形（自動裁剪避免越界）
fn fill_rect(img: &mut RgbaImage, x: i32, y: i32, w: i32, h: i32, color: Rgba<u8>) {
    let w = w.max(0) as u32;
    let h = h.max(0) as u32;
    if w == 0 || h == 0 { return; }
    if x < 0 || y < 0 { return; }
    imageproc::drawing::draw_filled_rect_mut(img, Rect::at(x, y).of_size(w, h), color);
}

/// 置中繪製文字
fn draw_centered(img: &mut RgbaImage, text: &str, font: &FontVec, scale: PxScale, y: i32) {
    let (tw, _) = imageproc::drawing::text_size(scale, font, text);
    let x = ((img.width() as i32 - tw as i32) / 2).max(8);
    imageproc::drawing::draw_text_mut(img, BK, x, y, scale, font, text);
}

/// Code 128 條碼
fn draw_barcode(img: &mut RgbaImage, text: &str, x: u32, y: u32, w: u32, h: u32) {
    use barcoders::sym::code128::Code128;

    // barcoders 1.x 要求第一個字元為 charset selector：
    //   'Ɓ'(U+0181) = Code128B（支援全部可列印 ASCII）
    let san: String = text.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').collect();
    let payload = format!("Ɓ{san}");
    let Ok(barcode) = Code128::new(&payload) else {
        tracing::warn!(text, "error_label: Code128 編碼失敗");
        return;
    };
    let encoded: Vec<u8> = barcode.encode();
    let n = encoded.len() as u32;
    if n == 0 { return; }
    let mw = (w / n).max(1);
    let sx = x + w.saturating_sub(mw * n) / 2;
    for (i, &bar) in encoded.iter().enumerate() {
        if bar == 1 {
            let bx = sx + i as u32 * mw;
            for px in bx..bx + mw {
                if px >= img.width() { break; }
                for py in y..y + h {
                    if py >= img.height() { break; }
                    img.put_pixel(px, py, BK);
                }
            }
        }
    }
}

// ── 個別狀態圖示 ──────────────────────────────────────────────────────────────

/// STORE_CLOSED — 圓圈內叉叉 ⊗（封閉/拒絕）
fn icon_x_circle(img: &mut RgbaImage, cx: i32, cy: i32, r: i32) {
    let lw = (r / 8).max(10);
    draw_thick_circle(img, cx, cy, r, lw);
    let d = (r as f32 * 0.60) as f32;
    draw_thick_line(img, cx as f32 - d, cy as f32 - d, cx as f32 + d, cy as f32 + d, lw as f32);
    draw_thick_line(img, cx as f32 + d, cy as f32 - d, cx as f32 - d, cy as f32 + d, lw as f32);
}

/// UNCONFIRMED — 巨型問號 ？
fn icon_question(img: &mut RgbaImage, cx: i32, cy: i32, r: i32) {
    let font = dejavu();
    let scale = PxScale::from(r as f32 * 2.1);
    let (tw, th) = imageproc::drawing::text_size(scale, font, "?");
    let x = cx - tw as i32 / 2;
    let y = cy - (th as f32 * 0.54) as i32;
    imageproc::drawing::draw_text_mut(img, BK, x, y, scale, font, "?");
}

/// NOT_FOUND — 放大鏡 + 叉叉
fn icon_magnifier(img: &mut RgbaImage, cx: i32, cy: i32, r: i32) {
    let lw = (r / 8).max(10) as f32;
    let lr = (r as f32 * 0.57) as i32;
    let lx = cx - r / 6;
    let ly = cy - r / 6;

    // 鏡框圓圈
    draw_thick_circle(img, lx, ly, lr, lw as i32);

    // 手柄（右下方粗線）
    let angle = std::f32::consts::FRAC_PI_4;
    let hx0 = lx as f32 + lr as f32 * angle.cos();
    let hy0 = ly as f32 + lr as f32 * angle.sin();
    let hx1 = cx as f32 + r as f32 * 0.70;
    let hy1 = cy as f32 + r as f32 * 0.70;
    draw_thick_line(img, hx0, hy0, hx1, hy1, lw + 8.0);

    // 鏡面內叉叉
    let xd = (lr as f32 * 0.50) as f32;
    draw_thick_line(img, lx as f32 - xd, ly as f32 - xd, lx as f32 + xd, ly as f32 + xd, lw - 2.0);
    draw_thick_line(img, lx as f32 + xd, ly as f32 - xd, lx as f32 - xd, ly as f32 + xd, lw - 2.0);
}

/// NOT_PROXY / NOT_FORWARD — 禁止圓圈 ⊘（slash_forward=true → /，false → \）
fn icon_prohibited(img: &mut RgbaImage, cx: i32, cy: i32, r: i32, slash_forward: bool) {
    let lw = (r / 8).max(10);
    draw_thick_circle(img, cx, cy, r, lw);
    let d = (r as f32 * 0.707) as f32;
    if slash_forward {
        // NOT_PROXY：/ 方向（右上 → 左下）
        draw_thick_line(img, cx as f32 + d, cy as f32 - d, cx as f32 - d, cy as f32 + d, (lw + 4) as f32);
    } else {
        // NOT_FORWARD：\ 方向（左上 → 右下）
        draw_thick_line(img, cx as f32 - d, cy as f32 - d, cx as f32 + d, cy as f32 + d, (lw + 4) as f32);
    }
}

/// ABNORMAL — 方框（包裹）內驚嘆號
fn icon_abnormal(img: &mut RgbaImage, cx: i32, cy: i32, r: i32) {
    let lw = (r / 8).max(10);
    let bw = (r as f32 * 1.6) as i32;
    let bh = (r as f32 * 1.5) as i32;
    let bx = cx - bw / 2;
    let by = cy - bh / 2;

    // 包裹外框（厚邊框矩形）
    fill_rect(img, bx,           by,           bw, lw, BK);
    fill_rect(img, bx,           by + bh - lw, bw, lw, BK);
    fill_rect(img, bx,           by,            lw, bh, BK);
    fill_rect(img, bx + bw - lw, by,            lw, bh, BK);

    // 驚嘆號置中
    let font = dejavu();
    let scale = PxScale::from(r as f32 * 1.1);
    let (ew, eh) = imageproc::drawing::text_size(scale, font, "!");
    let ex = cx - ew as i32 / 2;
    let ey = cy - (eh as f32 * 0.52) as i32;
    imageproc::drawing::draw_text_mut(img, BK, ex, ey, scale, font, "!");
}

/// ERROR / 其他 — 警告三角形 ⚠
fn icon_warning(img: &mut RgbaImage, cx: i32, cy: i32, r: i32) {
    // 等腰三角形（頂點向上）
    let top_y = cy - r;
    let bot_y = cy + r * 10 / 13;
    let bot_dx = (r as f32 * 0.87) as i32;

    let outer = &[
        Point::new(cx, top_y),
        Point::new(cx - bot_dx, bot_y),
        Point::new(cx + bot_dx, bot_y),
    ];
    imageproc::drawing::draw_polygon_mut(img, outer, BK);

    // 內部挖白（留三角形邊框）
    let m = (r / 9).max(8);
    let inner = &[
        Point::new(cx, top_y + m * 2 + 4),
        Point::new(cx - bot_dx + m + 2, bot_y - m),
        Point::new(cx + bot_dx - m - 2, bot_y - m),
    ];
    imageproc::drawing::draw_polygon_mut(img, inner, WH);

    // 驚嘆號（文字）
    let font = dejavu();
    let scale = PxScale::from(r as f32 * 0.82);
    let (ew, eh) = imageproc::drawing::text_size(scale, font, "!");
    let ex = cx - ew as i32 / 2;
    let ey = cy - (eh as f32 * 0.52) as i32;
    imageproc::drawing::draw_text_mut(img, BK, ex, ey, scale, font, "!");
}

/// 依 error_code 分派對應圖示
fn draw_icon(img: &mut RgbaImage, cx: i32, cy: i32, r: i32, code: &str) {
    match code {
        "STORE_CLOSED" => icon_x_circle(img, cx, cy, r),
        "UNCONFIRMED"  => icon_question(img, cx, cy, r),
        "NOT_FOUND"    => icon_magnifier(img, cx, cy, r),
        "NOT_PROXY"    => icon_prohibited(img, cx, cy, r, true),
        "NOT_FORWARD"  => icon_prohibited(img, cx, cy, r, false),
        "ABNORMAL" | "STATUS_ABNORMAL" => icon_abnormal(img, cx, cy, r),
        _              => icon_warning(img, cx, cy, r),
    }
}

// ── 公開 API ──────────────────────────────────────────────────────────────────

/// 紙張高度規格（寬固定 100mm＝800px @ 203 DPI）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelHeight {
    /// 100mm × 150mm → 800 × 1200 px（預設）
    H150mm,
    /// 100mm × 100mm → 800 × 800 px
    H100mm,
}

impl LabelHeight {
    fn px(self) -> u32 {
        match self { LabelHeight::H150mm => 1200, LabelHeight::H100mm => 800 }
    }
}

impl Default for LabelHeight {
    fn default() -> Self { LabelHeight::H150mm }
}

/// 產生錯誤面單 PNG bytes
///
/// - `query_no`   : 工控機掃描的查詢號（條碼 + 文字）
/// - `error_code` : 雲端回的 business code（STORE_CLOSED / NOT_FOUND / …）
/// - `height`     : 紙張高度規格，預設 100×150mm
pub fn generate(query_no: &str, error_code: &str, height: LabelHeight) -> Vec<u8> {
    let (zh_text, vi_text) = error_texts(error_code);
    let font = dejavu();

    const W: u32 = 800;
    let h = height.px();
    // 以 1200px 為基準，等比縮放所有座標
    let sf = h as f32 / 1200.0;

    // ── 座標定義 ──
    // 上段：條碼 + 查詢號 + 分隔線（壓縮，讓下方內容有足夠空間置中）
    // 上段：條碼上方留頂距，條碼 + 查詢號 + 分隔線
    let barcode_y = pf(180.0, sf);
    let barcode_h = pf(185.0, sf);
    let qn_y      = pf(380.0, sf);  // 條碼底(365) + 15px
    let sep_y     = pf(460.0, sf);  // qn 底(~435) + 25px

    // 下段：圖示 + 中文 + 越南文，以分隔線以下的空間垂直置中
    // 可用空間(H100mm) = 800 - 307(sep) - 4 = 489px；內容 306px → 上邊距 91px
    let icon_cy   = pf(742.0, sf);
    let icon_r    = pf(145.0, sf);
    let zh_y      = pf(933.0, sf);
    let vi_y      = pf(1017.0, sf);

    let zh_scale  = PxScale::from(sf * 66.0);
    let vi_scale  = PxScale::from(sf * 40.0);
    let qn_scale  = PxScale::from(sf * 50.0);  // 查詢號字體再加大

    let mut img = RgbaImage::from_pixel(W, h, WH);

    // 外框（4px）
    for i in 0..4u32 {
        for x in 0..W { img.put_pixel(x, i, BK); img.put_pixel(x, h - 1 - i, BK); }
        for y in 0..h { img.put_pixel(i, y, BK); img.put_pixel(W - 1 - i, y, BK); }
    }

    // ── 條碼 ──
    draw_barcode(&mut img, query_no, 50, barcode_y, 700, barcode_h);

    // ── 查詢號文字 ──
    draw_centered(&mut img, query_no, font, qn_scale, qn_y as i32);

    // ── 分隔線（雙線）──
    for y in sep_y..sep_y + 3 { for x in 20..W - 20 { img.put_pixel(x, y, GRAY); } }

    // ── 大圖示 ──
    draw_icon(&mut img, W as i32 / 2, icon_cy as i32, icon_r as i32, error_code);

    // ── 中文說明（CJK 字型，置中）──
    if let Some(cjk) = cjk_font() {
        draw_centered(&mut img, zh_text, cjk, zh_scale, zh_y as i32);
    } else {
        draw_centered(&mut img, error_code, font, PxScale::from(sf * 44.0), zh_y as i32);
    }

    // ── 越南文說明（DejaVu，置中）──
    draw_centered(&mut img, vi_text, font, vi_scale, vi_y as i32);

    // ── 輸出 PNG ──
    let mut buf = Vec::new();
    let _ = img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png);
    buf
}

/// 比例換算：以基準值 * scale_factor，回傳像素（u32）
#[inline]
fn pf(base: f32, sf: f32) -> u32 { (base * sf) as u32 }
