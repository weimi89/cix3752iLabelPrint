//! 讀碼站快照相機
//!
//! 收到工控機 `GET /api/parcel` 當下,需要一張「讀碼站此刻畫面」做為存證,用來釐清
//! 分揀線上「沒貨卻出紙」到底有沒有實體包裹。設計重點:
//!
//! - **相機開一次、常駐取流**:USB 相機初始化要數百 ms ~ 數秒且可能撞鎖,絕不能每次請求才開。
//!   啟動時開一條背景執行緒持續抓幀,只保留「最新一幀」在 [`Mutex`] 裡;請求進來時
//!   釘住當下那幀即可,毫秒級、不阻塞工控機回應(對齊設計原則 #2)。
//! - **擷取時不處理畫面**:只保存相機送來的原始畫面(MJPEG 缺霍夫曼表時補上標準表),
//!   等真的要存證或預覽時才依設定的品質解碼、變焦、壓縮(見 [`Snapshot::to_jpeg`])。
//!   400 萬畫素每秒 10 次解碼＋壓縮會長期吃掉約半顆核心,跟分揀查格口搶 CPU,實際只有包裹進來那一刻用得到。
//! - **太舊的畫面不當存證**:超過 [`FRAME_MAX_AGE`] 沒有新畫面(相機鬆脫、驅動卡住)就視為沒有畫面,
//!   照片留空,並由呼叫端記一筆告警([`CameraManager::take_stale_notice`]);拿舊畫面存證比沒有照片更糟。
//! - **graceful**:相機未啟用 / 未接 / 權限未給 / 索引錯時,只 log + 每 5s 重試開啟,
//!   [`CameraManager::snapshot`] 回 `None`,完全不影響正常出單流程(`photo_path` 保持 NULL)。
//! - **跨平台**:nokhwa `input-native` = macOS AVFoundation / Windows MSMF / Linux V4L2。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use crate::config::CameraConfig;

/// 系統偵測到的一台相機,供設定頁下拉選擇。`name` 是作業系統給的裝置名稱(設定檔存的就是它)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CameraDevice {
    pub name: String,
    pub description: String,
    pub index: u32,
}

/// 列舉目前接上的相機。走系統 API(macOS AVFoundation / Windows MSMF / Linux V4L2),
/// 可能要幾十到幾百毫秒,不要在請求路徑上同步呼叫。列舉失敗視同沒有相機,只 log。
pub fn list_devices() -> Vec<CameraDevice> {
    use nokhwa::utils::{ApiBackend, CameraIndex};
    let infos = match nokhwa::query(ApiBackend::Auto) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(?e, "列舉相機失敗");
            return Vec::new();
        }
    };
    infos
        .into_iter()
        .filter_map(|info| {
            let index = match info.index() {
                CameraIndex::Index(i) => *i,
                CameraIndex::String(s) => s.parse().ok()?,
            };
            Some(CameraDevice {
                name: info.human_name(),
                description: info.description().to_string(),
                index,
            })
        })
        .collect()
}

/// 依設定挑出要開的相機索引。名稱優先(接多台或換 USB 孔後索引會變,名稱不會);
/// 名稱留白時退回舊版設定的索引。指定了名稱但清單裡沒有 → `None`(相機沒接上或名稱已變)。
fn pick_device(devices: &[CameraDevice], name: &str, legacy_index: u32) -> Option<u32> {
    if name.is_empty() {
        return Some(legacy_index);
    }
    devices.iter().find(|d| d.name == name).map(|d| d.index)
}

/// 擷取執行緒每次開相機前用來認機的設定切片(名稱優先、索引備援,見 [`pick_device`])。
struct DeviceSelector {
    name: String,
    legacy_index: u32,
}

#[derive(Clone)]
pub struct CameraManager {
    inner: Arc<Inner>,
}

/// 最新一幀超過這麼久沒更新就不再當作「此刻畫面」(擷取迴圈約每 0.1～0.2 秒更新一次)
pub const FRAME_MAX_AGE: Duration = Duration::from_secs(2);

/// 釘住的一幀。複製只是增加參照,不複製影像;要 JPEG 時呼叫 [`Snapshot::to_jpeg`]。
#[derive(Clone)]
pub struct Snapshot {
    frame: Arc<Frame>,
}

/// 相機送來的原始畫面,加上取到當下的變焦與壓縮品質
struct Frame {
    source: FrameSource,
    zoom: f32,
    quality: u8,
}

enum FrameSource {
    /// MJPEG 幀(已補上標準霍夫曼表)
    Jpeg(Vec<u8>),
    /// 相機送來的其他格式(YUYV、NV12…)
    Camera(nokhwa::Buffer),
}

impl Snapshot {
    /// 依設定的品質轉成 JPEG(400 萬畫素解碼＋壓縮約數十毫秒),**不要在請求路徑上同步呼叫**,
    /// 放在 `spawn_blocking` 或背景工作裡。解碼失敗回 None(照片留空)。
    pub fn to_jpeg(&self) -> Option<Vec<u8>> {
        let Frame { source, zoom, quality } = self.frame.as_ref();
        let (w, h, rgb) = decode_rgb(source).map_err(|e| tracing::warn!(%e, "讀碼站畫面解碼失敗")).ok()?;
        let (out_w, out_h, bytes) = apply_zoom(rgb, w, h, *zoom);
        encode_jpeg(out_w, out_h, &bytes, *quality)
    }
}

#[derive(Default)]
struct Latest {
    /// 最新一幀;取幀失敗或換設定時清掉
    frame: Option<Snapshot>,
    /// 最後一次取到畫面的時間。取幀失敗時保留(用來判斷相機斷了多久、發過舊告警),換設定才清掉
    at: Option<Instant>,
}

struct Inner {
    latest: Mutex<Latest>,
    /// 這次「畫面過舊」已經回報過了(拿到新畫面時重置),避免每件包裹都記一次
    stale_reported: std::sync::atomic::AtomicBool,
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
                latest: Mutex::new(Latest::default()),
                stale_reported: std::sync::atomic::AtomicBool::new(false),
                generation: AtomicU64::new(0),
                zoom_bits: AtomicU32::new(1.0f32.to_bits()),
            }),
        }
    }

    /// 啟動時呼叫,語意等同首次 [`apply_config`](Self::apply_config)。
    pub fn start(&self, config: &CameraConfig) {
        self.apply_config(config);
    }

    /// 套用(熱)相機設定:`enabled` 切換、換相機 / 品質變更都即時生效,無須重啟 App。
    ///
    /// 作法:每次呼叫把世代 +1 —— 任何既有擷取執行緒在下一輪檢查發現世代已變,即自行結束並
    /// 釋放相機裝置;若新設定 `enabled`,再為新世代開一條新擷取執行緒。重複呼叫安全:舊執行緒
    /// 退出與新執行緒接手之間若短暫重疊撞到裝置鎖,新執行緒開啟失敗會自動 5s 重試,最終穩定。
    pub fn apply_config(&self, config: &CameraConfig) {
        // 世代 +1 → 通知任何舊擷取執行緒退出(舊執行緒之後也寫不進 latest,見 store_frame)
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // 清掉上一台相機殘留的最新幀與時間,避免停用 / 換機後仍服務到舊畫面或誤報過舊
        *self.inner.latest.lock() = Latest::default();
        self.inner.stale_reported.store(false, Ordering::SeqCst);
        if !config.enabled {
            tracing::info!("讀碼站相機未啟用(camera.enabled=false),不啟動擷取");
            return;
        }
        // 變焦存進 Inner(capture_loop 每幀讀;set_zoom 可即時調整而不必重啟此執行緒)
        self.inner
            .zoom_bits
            .store(config.zoom.clamp(1.0, 4.0).to_bits(), Ordering::Relaxed);
        let inner = self.inner.clone();
        let selector = DeviceSelector {
            name: config.device_name.clone(),
            legacy_index: config.device_index,
        };
        let quality = config.jpeg_quality.clamp(1, 100);
        if let Err(e) = std::thread::Builder::new()
            .name("camera-capture".into())
            .spawn(move || capture_loop(inner, generation, selector, quality))
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

    /// 釘住當下最新一幀;相機未啟用、尚未取到幀,或最新一幀已超過 [`FRAME_MAX_AGE`] 時回 None。
    /// 在 HTTP handler 收到請求的當下呼叫(只增加參照,毫秒級),再丟背景轉 JPEG 存檔。
    pub fn snapshot(&self) -> Option<Snapshot> {
        let latest = self.inner.latest.lock();
        let fresh = latest.at.is_some_and(|at| at.elapsed() <= FRAME_MAX_AGE);
        latest.frame.clone().filter(|_| fresh)
    }

    /// 這組設定取到過畫面,但已經超過 [`FRAME_MAX_AGE`] 沒有新畫面(相機鬆脫、驅動卡住、
    /// 一直重開失敗)時回傳斷了多久;同一段期間只回傳一次,給呼叫端記一筆告警。
    /// 換設定或停用後還沒取到畫面不算。
    pub fn take_stale_notice(&self) -> Option<Duration> {
        let age = self.inner.latest.lock().at?.elapsed();
        if age <= FRAME_MAX_AGE || self.inner.stale_reported.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(age)
    }

    /// 取當下畫面並轉成 JPEG(相機預覽、手動拍照用)。會解碼壓縮,要在 `spawn_blocking` 裡呼叫。
    pub fn latest_jpeg_blocking(&self) -> Option<Vec<u8>> {
        self.snapshot()?.to_jpeg()
    }
}

impl Default for CameraManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 常駐擷取迴圈:開相機 → 持續抓幀存進 latest(完整 JPEG 原樣保存,其他格式等要用時才轉,見 [`keep_frame`])。
/// 取幀失敗就跳出重開;開相機失敗就 5s 後重試。整個 nokhwa `Camera` 只活在本執行緒內
///(不跨執行緒搬移),避開 macOS AVFoundation 物件非 Send 的問題。
fn capture_loop(inner: Arc<Inner>, generation: u64, selector: DeviceSelector, quality: u8) {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    use nokhwa::Camera;

    // 世代被新設定取代(或已停用)就結束;否則開相機 → 持續抓幀
    while inner.generation.load(Ordering::SeqCst) == generation {
        // 每次重開都重新認機:相機拔掉再插回(索引可能變了)也接得回來
        let devices = if selector.name.is_empty() { Vec::new() } else { list_devices() };
        let Some(device_index) = pick_device(&devices, &selector.name, selector.legacy_index) else {
            tracing::warn!(
                name = %selector.name,
                detected = ?devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
                "找不到設定的讀碼站相機(未接上或名稱已變),5s 後重試"
            );
            std::thread::sleep(Duration::from_secs(5));
            continue;
        };
        let index = CameraIndex::Index(device_index);
        let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

        match Camera::new(index, format).and_then(|mut cam| cam.open_stream().map(|_| cam)) {
            Ok(mut cam) => {
                tracing::info!(device_index, "讀碼站相機已開啟,開始擷取");
                let mut bad_frames = 0u32;
                loop {
                    // 設定已更新 / 停用 → 結束本執行緒,cam drop 釋放裝置讓新世代接手
                    if inner.generation.load(Ordering::SeqCst) != generation {
                        tracing::info!(device_index, "讀碼站相機設定已更新或停用,結束此擷取執行緒");
                        return;
                    }
                    // 每幀讀目前變焦(set_zoom 的即時調整即在此生效);zoom>1 時存證要裁切中央區域(拉近)
                    let zoom = f32::from_bits(inner.zoom_bits.load(Ordering::Relaxed));
                    let failure = match cam.frame() {
                        Ok(buffer) => match keep_frame(buffer, zoom, quality) {
                            Ok(frame) => {
                                store_frame(&inner, generation, frame);
                                bad_frames = 0;
                                None
                            }
                            // 偶爾一張讀不出來就跳過;連續太多張才當成相機出問題
                            Err(e) => {
                                bad_frames += 1;
                                (bad_frames >= MAX_BAD_FRAMES).then(|| format!("連續 {bad_frames} 張畫面讀不出來:{e}"))
                            }
                        },
                        Err(e) => Some(e.to_string()),
                    };
                    if let Some(why) = failure {
                        // 立刻清掉舊畫面:重開期間進來的包裹寧可沒有照片,也不能存到斷線前的舊畫面
                        drop_frame(&inner, generation);
                        tracing::warn!(%why, "讀碼站相機取幀失敗,將重開相機");
                        break;
                    }
                    // ~10fps 足夠「最新一幀」用途
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Err(e) => {
                tracing::warn!(
                    ?e,
                    device_index,
                    name = %selector.name,
                    "開啟讀碼站相機失敗(未接 / 權限未給 / 被別的程式占用),5s 後重試"
                );
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// 連續幾張畫面讀不出來就重開相機(約 1～2 秒)
const MAX_BAD_FRAMES: u32 = 10;

/// 存入最新一幀。設定已換(世代不同)就不寫:舊執行緒在換設定的當下才拿到的畫面,
/// 寫進去會讓停用或換機後仍服務到舊相機的畫面。
fn store_frame(inner: &Inner, generation: u64, frame: Frame) {
    let mut latest = inner.latest.lock();
    if inner.generation.load(Ordering::SeqCst) != generation {
        return;
    }
    latest.frame = Some(Snapshot { frame: Arc::new(frame) });
    latest.at = Some(Instant::now());
    inner.stale_reported.store(false, Ordering::SeqCst);
}

/// 取幀失敗:清掉最新一幀但保留取到的時間(過舊告警靠它)。設定已換就不動,以免清掉新相機的畫面。
fn drop_frame(inner: &Inner, generation: u64) {
    let mut latest = inner.latest.lock();
    if inner.generation.load(Ordering::SeqCst) == generation {
        latest.frame = None;
    }
}

/// 保存這一幀的原始畫面,不解碼。MJPEG 缺霍夫曼表時補上標準表;
/// 不完整的 MJPEG 當場解碼一次確認讀得出來,讀不出來回錯(這張跳過,連續太多張才重開相機)。
fn keep_frame(buffer: nokhwa::Buffer, zoom: f32, quality: u8) -> Result<Frame, String> {
    use nokhwa::utils::FrameFormat;
    if buffer.source_frame_format() != FrameFormat::MJPEG {
        return Ok(Frame { source: FrameSource::Camera(buffer), zoom, quality });
    }
    let jpeg = with_huffman_tables(buffer.buffer()).ok_or("不是 JPEG 畫面")?;
    let complete = is_complete_jpeg(&jpeg);
    let source = FrameSource::Jpeg(jpeg);
    if !complete {
        decode_rgb(&source)?;
    }
    Ok(Frame { source, zoom, quality })
}

/// 解成 RGB,回傳 `(寬, 高, RGB bytes)`。
///
/// MJPEG **不可用 nokhwa 的 `decode_image`**:它底層的 mozjpeg 遇到壞資料是用拋出例外的方式報錯,
/// 而正式版設定 `panic = "abort"`,一張壞幀就會讓整個程式結束。改用 image 套件(純 Rust,壞資料只回錯誤)。
/// 其他格式(YUYV、NV12…)是 nokhwa 的純 Rust 轉換,出錯一樣只回錯誤。
fn decode_rgb(source: &FrameSource) -> Result<(u32, u32, Vec<u8>), String> {
    use nokhwa::pixel_format::RgbFormat;
    match source {
        FrameSource::Jpeg(jpeg) => {
            let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
                .map_err(|e| e.to_string())?
                .to_rgb8();
            let (w, h) = img.dimensions();
            Ok((w, h, img.into_raw()))
        }
        FrameSource::Camera(buffer) => {
            let img = buffer.decode_image::<RgbFormat>().map_err(|e| e.to_string())?;
            let (w, h) = (img.width(), img.height());
            Ok((w, h, img.into_raw()))
        }
    }
}

/// JPEG 規格附錄 K.3 的標準霍夫曼表(亮度 DC/AC、色度 DC/AC),組成一個 DHT 區段。
/// 數值取自 image 套件編碼器的同名常數,由腳本產生,不要手改。
#[rustfmt::skip]
const STD_DHT_SEGMENT: [u8; 420] = [
    0xFF, 0xC4, 0x01, 0xA2, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
    0x0B, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00,
    0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51,
    0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52,
    0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47,
    0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67,
    0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
    0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3,
    0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
    0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6,
    0xF7, 0xF8, 0xF9, 0xFA, 0x01, 0x00, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
    0x0B, 0x11, 0x00, 0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01,
    0x02, 0x77, 0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07,
    0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33,
    0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19,
    0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46,
    0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66,
    0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84, 0x85,
    0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3,
    0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
    0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8,
    0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6,
    0xF7, 0xF8, 0xF9, 0xFA,
];

/// 掃 JPEG 的表頭區段(SOI 之後、影像資料 SOS 之前),回傳「有沒有霍夫曼表」與 SOS 的位置。
/// 開頭不是 SOI、區段結構壞掉時回 None。
fn scan_jpeg_headers(bytes: &[u8]) -> Option<(bool, usize)> {
    if bytes.len() < 4 || bytes[..2] != [0xFF, 0xD8] {
        return None;
    }
    let mut has_dht = false;
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        match bytes[i + 1] {
            0xFF => i += 1,                   // 填充位元組
            0xDA => return Some((has_dht, i)), // 影像資料開始
            0xD0..=0xD7 | 0x01 => i += 2,     // 沒有長度欄位的標記
            marker => {
                has_dht |= marker == 0xC4;
                let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                if len < 2 {
                    return None;
                }
                i += 2 + len;
            }
        }
    }
    None
}

/// 很多網路攝影機的 MJPEG 幀省略霍夫曼表(規定解碼時用標準表),一般的 JPEG 解碼器讀不出來。
/// 缺表就在影像資料前補上標準表,補完就是一般的 JPEG。不是 JPEG 時回 None。
fn with_huffman_tables(bytes: &[u8]) -> Option<Vec<u8>> {
    match scan_jpeg_headers(bytes) {
        Some((false, sos)) => {
            let mut out = Vec::with_capacity(bytes.len() + STD_DHT_SEGMENT.len());
            out.extend_from_slice(&bytes[..sos]);
            out.extend_from_slice(&STD_DHT_SEGMENT);
            out.extend_from_slice(&bytes[sos..]);
            Some(out)
        }
        // 有表,或表頭結構看不懂(交給解碼器判斷讀不讀得出來)
        _ if bytes.starts_with(&[0xFF, 0xD8]) => Some(bytes.to_vec()),
        _ => None,
    }
}

/// 是不是完整的 JPEG:開頭是 SOI、影像資料之前帶有霍夫曼表、結尾是 EOI。
/// 有些驅動回固定大小的緩衝區、後面補零,結尾的零先略過再看。
fn is_complete_jpeg(bytes: &[u8]) -> bool {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    bytes[..end].ends_with(&[0xFF, 0xD9]) && matches!(scan_jpeg_headers(bytes), Some((true, _)))
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
mod pick_tests {
    use super::*;

    fn devices() -> Vec<CameraDevice> {
        vec![
            CameraDevice { name: "Logitech C920".into(), description: "usb".into(), index: 1 },
            CameraDevice { name: "Integrated Camera".into(), description: "builtin".into(), index: 0 },
        ]
    }

    #[test]
    fn 依名稱認機_不看索引() {
        assert_eq!(pick_device(&devices(), "Logitech C920", 0), Some(1));
        assert_eq!(pick_device(&devices(), "Integrated Camera", 7), Some(0));
    }

    #[test]
    fn 名稱留白退回舊索引() {
        assert_eq!(pick_device(&devices(), "", 2), Some(2));
        assert_eq!(pick_device(&[], "", 0), Some(0));
    }

    #[test]
    fn 指定的相機沒接上就不開() {
        assert_eq!(pick_device(&devices(), "Logitech C270", 0), None);
        assert_eq!(pick_device(&[], "Logitech C920", 0), None);
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
            device_name: String::new(),
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
            if let Some(j) = mgr.latest_jpeg_blocking() {
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

#[cfg(test)]
mod frame_tests {
    use super::*;
    use nokhwa::utils::{FrameFormat, Resolution};

    /// 64x48 的漸層圖壓成 JPEG(image 套件會寫入霍夫曼表)
    fn sample_jpeg() -> Vec<u8> {
        let (w, h) = (64u32, 48u32);
        let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i % 256) as u8, (i / 7 % 256) as u8, 128]).collect();
        encode_jpeg(w, h, &rgb, 95).unwrap()
    }

    /// 拿掉所有霍夫曼表區段,模擬省略 DHT 的網路攝影機 MJPEG 幀
    fn strip_dht(jpeg: &[u8]) -> Vec<u8> {
        let mut out = jpeg[..2].to_vec();
        let mut i = 2;
        loop {
            let marker = jpeg[i + 1];
            if marker == 0xDA {
                out.extend_from_slice(&jpeg[i..]);
                return out;
            }
            let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
            if marker != 0xC4 {
                out.extend_from_slice(&jpeg[i..i + 2 + len]);
            }
            i += 2 + len;
        }
    }

    fn mjpeg(bytes: &[u8]) -> nokhwa::Buffer {
        nokhwa::Buffer::new(Resolution::new(64, 48), bytes, FrameFormat::MJPEG)
    }

    fn rgb_of(jpeg: &[u8]) -> image::RgbImage {
        image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).unwrap().to_rgb8()
    }

    fn stored_jpeg(frame: &Frame) -> &[u8] {
        match &frame.source {
            FrameSource::Jpeg(b) => b,
            FrameSource::Camera(_) => panic!("MJPEG 應以 JPEG 保存"),
        }
    }

    fn snap(frame: Frame) -> Snapshot {
        Snapshot { frame: Arc::new(frame) }
    }

    #[test]
    fn complete_jpeg_detection() {
        let jpeg = sample_jpeg();
        assert!(is_complete_jpeg(&jpeg));
        let mut padded = jpeg.clone();
        padded.extend_from_slice(&[0u8; 4096]);
        assert!(is_complete_jpeg(&padded), "驅動在結尾補零仍算完整,不必每幀解碼確認");
        assert!(!is_complete_jpeg(&strip_dht(&jpeg)), "缺霍夫曼表");
        assert!(!is_complete_jpeg(&jpeg[..jpeg.len() / 2]), "截斷(沒有結尾標記)");
        assert!(!is_complete_jpeg(b"not a jpeg"));
        assert!(!is_complete_jpeg(&[]));
    }

    #[test]
    fn mjpeg_frame_is_stored_untouched_and_encoded_with_configured_quality_on_demand() {
        let jpeg = sample_jpeg();
        let low = keep_frame(mjpeg(&jpeg), 1.0, 20).unwrap();
        assert_eq!(stored_jpeg(&low), jpeg.as_slice(), "擷取時原樣保存,不解碼");
        let high = keep_frame(mjpeg(&jpeg), 1.0, 95).unwrap();
        let (low, high) = (snap(low).to_jpeg().unwrap(), snap(high).to_jpeg().unwrap());
        assert!(low.len() < high.len(), "存證照片依設定的品質壓縮: {} vs {}", low.len(), high.len());
        assert_eq!(rgb_of(&low).dimensions(), (64, 48));
    }

    #[test]
    fn mjpeg_without_huffman_tables_gets_the_standard_tables() {
        let original = sample_jpeg();
        let frame = keep_frame(mjpeg(&strip_dht(&original)), 1.0, 80).unwrap();
        let fixed = stored_jpeg(&frame);
        assert!(is_complete_jpeg(fixed));
        // 補上的表正確 ⇔ 解出來的畫面與原圖逐像素相同
        assert_eq!(rgb_of(&original), rgb_of(fixed));
    }

    #[test]
    fn zoom_crops_when_converting() {
        let frame = keep_frame(mjpeg(&sample_jpeg()), 2.0, 80).unwrap();
        assert_eq!(rgb_of(&snap(frame).to_jpeg().unwrap()).dimensions(), (32, 24));
    }

    #[test]
    fn unreadable_mjpeg_frame_is_an_error_so_the_camera_reopens() {
        assert!(keep_frame(mjpeg(b"definitely not a jpeg frame"), 1.0, 80).is_err());
    }

    #[test]
    fn truncated_mjpeg_frame_that_still_decodes_is_kept() {
        let jpeg = sample_jpeg();
        let frame = keep_frame(mjpeg(&jpeg[..jpeg.len() / 2]), 1.0, 80).unwrap();
        assert!(snap(frame).to_jpeg().is_some());
    }

    fn frame() -> Frame {
        keep_frame(mjpeg(&sample_jpeg()), 1.0, 80).unwrap()
    }

    fn current_generation(cam: &CameraManager) -> u64 {
        cam.inner.generation.load(Ordering::SeqCst)
    }

    fn age_last_frame(cam: &CameraManager, age: Duration) {
        cam.inner.latest.lock().at = Some(Instant::now() - age);
    }

    #[test]
    fn fresh_frame_is_served_and_stale_frame_is_reported_once() {
        let cam = CameraManager::new();
        assert!(cam.snapshot().is_none(), "還沒取到幀");
        assert!(cam.take_stale_notice().is_none(), "從沒取到幀不算過舊");

        store_frame(&cam.inner, current_generation(&cam), frame());
        assert!(cam.snapshot().is_some());
        assert!(cam.take_stale_notice().is_none());

        age_last_frame(&cam, FRAME_MAX_AGE + Duration::from_secs(1));
        assert!(cam.snapshot().is_none(), "超過時限的舊畫面不可拿來當存證");
        assert!(cam.take_stale_notice().is_some(), "第一次發現過舊要回報");
        assert!(cam.take_stale_notice().is_none(), "同一段過舊期間只回報一次");

        store_frame(&cam.inner, current_generation(&cam), frame());
        age_last_frame(&cam, FRAME_MAX_AGE + Duration::from_secs(1));
        assert!(cam.take_stale_notice().is_some(), "恢復後再斷一次要再回報");
    }

    #[test]
    fn unplugged_camera_is_reported_after_frames_stop() {
        // 拔掉相機:取幀失敗清掉畫面,之後一直重開失敗,也要發過舊告警
        let cam = CameraManager::new();
        let g = current_generation(&cam);
        store_frame(&cam.inner, g, frame());
        drop_frame(&cam.inner, g);
        assert!(cam.snapshot().is_none(), "取幀失敗後立刻不給舊畫面");
        age_last_frame(&cam, FRAME_MAX_AGE + Duration::from_secs(1));
        assert!(cam.take_stale_notice().is_some());
    }

    #[test]
    fn old_capture_thread_cannot_write_after_settings_change() {
        let cam = CameraManager::new();
        let old = current_generation(&cam);
        store_frame(&cam.inner, old, frame());
        cam.apply_config(&CameraConfig { enabled: false, ..CameraConfig::default() });
        store_frame(&cam.inner, old, frame());
        assert!(cam.snapshot().is_none(), "停用後舊執行緒晚到的畫面不可寫回");
        assert!(cam.take_stale_notice().is_none(), "刻意停用不算相機斷線");

        let new = current_generation(&cam);
        store_frame(&cam.inner, new, frame());
        drop_frame(&cam.inner, old);
        assert!(cam.snapshot().is_some(), "舊執行緒的取幀失敗不可清掉新設定的畫面");
    }
}
