//! 讀碼站快照相機
//!
//! 收到工控機 `GET /api/parcel` 當下,需要一張「讀碼站此刻畫面」做為存證,用來釐清
//! 分揀線上「沒貨卻出紙」到底有沒有實體包裹。設計重點:
//!
//! - **相機開一次、常駐取流**:USB 相機初始化要數百 ms ~ 數秒且可能撞鎖,絕不能每次請求才開。
//!   啟動時開一條背景執行緒持續抓幀,只保留「最新一幀」(JPEG)在 [`Mutex`] 裡;請求進來時
//!   直接複製當下那幀即可,毫秒級、不阻塞工控機回應(對齊設計原則 #2)。
//! - **graceful**:相機未啟用 / 未接 / 權限未給 / 索引錯時,只 log + 每 5s 重試開啟,
//!   `latest_jpeg()` 回 `None`,完全不影響正常出單流程(`photo_path` 保持 NULL)。
//! - **跨平台**:nokhwa `input-native` = macOS AVFoundation / Windows MSMF / Linux V4L2。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::config::CameraConfig;

#[derive(Clone)]
pub struct CameraManager {
    inner: Arc<Inner>,
}

struct Inner {
    /// 最新一幀(JPEG bytes);尚未取到幀時為 None
    latest: Mutex<Option<Vec<u8>>>,
    /// 目前設定世代:每次 [`apply_config`](CameraManager::apply_config) +1。擷取執行緒只在自己的
    /// 世代仍是最新時續跑;被新設定取代(換 device_index / 品質)或停用時,於下一輪檢查自行結束
    /// 並釋放相機裝置 —— 讓「設定頁切換相機開關 / 換鏡頭」即時生效而不需重啟 App。
    generation: AtomicU64,
    /// 數位變焦倍率(以 f32 bits 存)。capture_loop **每幀讀取**,故 [`set_zoom`](CameraManager::set_zoom)
    /// 可即時調整 —— 預覽串流立刻改變、不必重啟執行緒、不必存設定檔。
    zoom_bits: AtomicU32,
}

impl CameraManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                latest: Mutex::new(None),
                generation: AtomicU64::new(0),
                zoom_bits: AtomicU32::new(1.0f32.to_bits()),
            }),
        }
    }

    /// 啟動時呼叫,語意等同首次 [`apply_config`](Self::apply_config)。
    pub fn start(&self, config: &CameraConfig) {
        self.apply_config(config);
    }

    /// 套用(熱)相機設定:`enabled` 切換、`device_index` / 品質變更都即時生效,無須重啟 App。
    ///
    /// 作法:每次呼叫把世代 +1 —— 任何既有擷取執行緒在下一輪檢查發現世代已變,即自行結束並
    /// 釋放相機裝置;若新設定 `enabled`,再為新世代開一條新擷取執行緒。重複呼叫安全:舊執行緒
    /// 退出與新執行緒接手之間若短暫重疊撞到裝置鎖,新執行緒開啟失敗會自動 5s 重試,最終穩定。
    pub fn apply_config(&self, config: &CameraConfig) {
        // 世代 +1 → 通知任何舊擷取執行緒退出
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // 清掉上一台相機殘留的最新幀,避免停用 / 換機後仍服務到舊畫面
        *self.inner.latest.lock() = None;
        if !config.enabled {
            tracing::info!("讀碼站相機未啟用(camera.enabled=false),不啟動擷取");
            return;
        }
        // 變焦存進 Inner(capture_loop 每幀讀;set_zoom 可即時調整而不必重啟此執行緒)
        self.inner
            .zoom_bits
            .store(config.zoom.clamp(1.0, 4.0).to_bits(), Ordering::Relaxed);
        let inner = self.inner.clone();
        let device_index = config.device_index;
        let quality = config.jpeg_quality.clamp(1, 100);
        if let Err(e) = std::thread::Builder::new()
            .name("camera-capture".into())
            .spawn(move || capture_loop(inner, generation, device_index, quality))
        {
            tracing::warn!(?e, "啟動讀碼站相機執行緒失敗");
        }
    }

    /// **即時**調整數位變焦(不重啟擷取執行緒、不寫設定檔)—— capture_loop 下一幀就套用,
    /// 預覽串流立刻改變。供「相機預覽對話框」拖滑桿時呼叫。要永久保存仍需把 `camera.zoom` 存進設定檔。
    pub fn set_zoom(&self, zoom: f32) {
        self.inner
            .zoom_bits
            .store(zoom.clamp(1.0, 4.0).to_bits(), Ordering::Relaxed);
    }

    /// 取得當下最新一幀(JPEG bytes);相機未啟用 / 尚未取到幀時回 None。
    /// 在 HTTP handler 收到請求的當下呼叫,把「那一刻」的畫面釘住,再丟背景存檔。
    pub fn latest_jpeg(&self) -> Option<Vec<u8>> {
        self.inner.latest.lock().clone()
    }
}

impl Default for CameraManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 常駐擷取迴圈:開相機 → 持續抓幀解碼成 JPEG 存進 latest。
/// 取幀失敗就跳出重開;開相機失敗就 5s 後重試。整個 nokhwa `Camera` 只活在本執行緒內
///(不跨執行緒搬移),避開 macOS AVFoundation 物件非 Send 的問題。
fn capture_loop(inner: Arc<Inner>, generation: u64, device_index: u32, quality: u8) {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    use nokhwa::Camera;

    // 世代被新設定取代(或已停用)就結束;否則開相機 → 持續抓幀
    while inner.generation.load(Ordering::SeqCst) == generation {
        let index = CameraIndex::Index(device_index);
        let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

        match Camera::new(index, format).and_then(|mut cam| cam.open_stream().map(|_| cam)) {
            Ok(mut cam) => {
                tracing::info!(device_index, "讀碼站相機已開啟,開始擷取");
                loop {
                    // 設定已更新 / 停用 → 結束本執行緒,cam drop 釋放裝置讓新世代接手
                    if inner.generation.load(Ordering::SeqCst) != generation {
                        tracing::info!(device_index, "讀碼站相機設定已更新或停用,結束此擷取執行緒");
                        return;
                    }
                    match cam.frame().and_then(|f| f.decode_image::<RgbFormat>()) {
                        Ok(decoded) => {
                            let (w, h) = (decoded.width(), decoded.height());
                            // 每幀讀目前變焦(set_zoom 的即時調整即在此生效);zoom>1 裁切中央區域(拉近)
                            let zoom = f32::from_bits(inner.zoom_bits.load(Ordering::Relaxed));
                            let (out_w, out_h, bytes) = apply_zoom(decoded.into_raw(), w, h, zoom);
                            if let Some(jpeg) = encode_jpeg(out_w, out_h, &bytes, quality) {
                                *inner.latest.lock() = Some(jpeg);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(?e, "讀碼站相機取幀失敗,將重開相機");
                            break;
                        }
                    }
                    // ~10fps 足夠「最新一幀」用途,避免高解析每幀解碼吃滿 CPU
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Err(e) => {
                tracing::warn!(
                    ?e,
                    device_index,
                    "開啟讀碼站相機失敗(未接 / 權限未給 / 索引錯),5s 後重試"
                );
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// 數位變焦:`zoom > 1.0` 時裁切畫面中央 `1/zoom` 的矩形(等於拉近框景),否則原樣回傳。
/// 回傳 `(寬, 高, RGB bytes)`。純裁切不放大 —— 直接存裁切後的較小影像即是「拉近」的視角,不插值、不失真。
fn apply_zoom(rgb: Vec<u8>, w: u32, h: u32, zoom: f32) -> (u32, u32, Vec<u8>) {
    if zoom <= 1.0 || w == 0 || h == 0 {
        return (w, h, rgb);
    }
    let cw = ((w as f32 / zoom) as u32).clamp(1, w);
    let ch = ((h as f32 / zoom) as u32).clamp(1, h);
    let ox = (w - cw) / 2;
    let oy = (h - ch) / 2;
    let row_bytes = cw as usize * 3;
    let mut out = Vec::with_capacity(row_bytes * ch as usize);
    for y in 0..ch {
        let start = (((oy + y) * w + ox) as usize) * 3;
        out.extend_from_slice(&rgb[start..start + row_bytes]);
    }
    (cw, ch, out)
}

/// 把 RGB raw 編成 JPEG(直接用 raw bytes + 尺寸,不經 image 版本相依的型別)
fn encode_jpeg(width: u32, height: u32, rgb: &[u8], quality: u8) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode(rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| tracing::warn!(?e, "讀碼站快照 JPEG 編碼失敗"))
        .ok()?;
    Some(out.into_inner())
}

/// 把一張快照寫到「存證目錄」(`AppConfig::resolved_captures_dir`,**獨立於面單快取**),
/// 回傳相對 key(寫回 `parcel_query_log.photo_path`,前端以 `/captures/{key}` 顯示)。
/// key 形如 `74Z01114611_20260625204729.jpg`;檔名沿用面單 `@error` 的安全字元規則。
/// 存證刻意不放面單快取下,因此**不受面單 keep_days 影響**,改由 [`cleanup_captures`] 依 camera.keep_days 清理。
pub fn save_snapshot(captures_dir: &Path, query_no: &str, jpeg: &[u8]) -> Option<String> {
    // 時間格式 YYYYMMDDHHMMSS(無分隔、無毫秒),例 20260625204729。
    // 檔名安全字元由 save_snapshot_named 統一處理,此處不需預先清洗。
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S");
    save_snapshot_named(captures_dir, &format!("{query_no}_{ts}"), jpeg)
}

/// 用「呼叫端已組好的完整檔名主幹」寫存證快照(**不附時間戳**),回傳相對 key `{stem}.jpg`。
/// 供呼叫端需要自控唯一檔名時使用:例如 NoRead 沒有真實單號、僅靠秒級時間戳會在同秒多筆讀碼失敗時
/// 互相覆蓋,故由呼叫端在 stem 內加序號保證唯一。檔名安全字元規則同 [`save_snapshot`]。
pub fn save_snapshot_named(captures_dir: &Path, file_stem: &str, jpeg: &[u8]) -> Option<String> {
    let safe: String = file_stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let key = format!("{safe}.jpg");
    if let Err(e) = std::fs::create_dir_all(captures_dir) {
        tracing::warn!(?e, "建立存證目錄失敗");
        return None;
    }
    let path: PathBuf = captures_dir.join(&key);
    match std::fs::write(&path, jpeg) {
        Ok(()) => Some(key),
        Err(e) => {
            tracing::warn!(?e, "寫入讀碼站快照失敗");
            None
        }
    }
}

/// 依保留天數清理存證目錄裡的過期快照。`keep_days = 0` 表永久保留(不清)。
/// 與面單快取的清理(cache.keep_days)完全獨立 —— 存證是爭議佐證,壽命由 camera.keep_days 單獨決定。
pub fn cleanup_captures(captures_dir: &Path, keep_days: u32) {
    if keep_days == 0 {
        return;
    }
    let threshold = match std::time::SystemTime::now()
        .checked_sub(Duration::from_secs(keep_days as u64 * 86_400))
    {
        Some(t) => t,
        None => return,
    };
    let entries = match std::fs::read_dir(captures_dir) {
        Ok(e) => e,
        Err(_) => return, // 目錄還沒建立 = 沒東西可清
    };
    let mut removed = 0u32;
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        if let Ok(modified) = meta.modified() {
            if modified < threshold && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!(removed, keep_days, "清理過期讀碼站存證");
    }
}

#[cfg(test)]
mod hw_tests {
    use super::*;
    use crate::config::CameraConfig;

    /// **硬體相依測試**:需實際接上 USB 相機。預設 `#[ignore]`,手動跑:
    /// ```bash
    /// cd src-tauri && cargo test --lib camera::hw_tests::captures_a_jpeg_frame -- --ignored --nocapture
    /// ```
    /// 驗證「收到請求 → 抓讀碼站當下一幀 → 存成 JPEG 留底」整條能力。
    /// macOS 首次跑會跳相機權限對話框,允許後重跑即過。
    #[test]
    #[ignore]
    fn captures_a_jpeg_frame() {
        let mgr = CameraManager::new();
        mgr.start(&CameraConfig {
            enabled: true,
            device_index: 0,
            jpeg_quality: 80,
            zoom: 1.0,
            captures_dir: String::new(),
            keep_days: 90,
        });

        // 等相機暖機 + 抓到第一幀(權限對話框可能在此跳出,給到 ~10 秒)
        let mut jpeg = None;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(200));
            if let Some(j) = mgr.latest_jpeg() {
                jpeg = Some(j);
                break;
            }
        }
        let jpeg = jpeg.expect("10 秒內未取到任何相機幀(相機未接 / 權限未給 / device_index 錯?)");
        assert!(jpeg.len() > 1000, "JPEG 太小,可能不是有效影像: {} bytes", jpeg.len());
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "不是 JPEG 檔頭(FFD8)");

        let dir = std::env::temp_dir().join("cix3752i_cam_test");
        let key = save_snapshot(&dir, "TEST_74Z01114611", &jpeg).expect("save_snapshot 失敗");
        let path = dir.join(&key);
        assert!(path.exists(), "快照檔不存在: {path:?}");
        eprintln!("✅ 拍照留底成功: {path:?} ({} bytes)", jpeg.len());
    }
}
