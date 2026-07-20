import { invoke } from '@tauri-apps/api/core'
import { enable as autostartEnable, disable as autostartDisable, isEnabled as autostartIsEnabled } from '@tauri-apps/plugin-autostart'

const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 健康檢查
export const ping = () => invoke('ping')

// 開機自動啟動(走 tauri-plugin-autostart)
export const getAutoStart = async () => {
  if (!isTauri) return false
  return await autostartIsEnabled()
}
export const setAutoStart = async enabled => {
  if (!isTauri) return
  if (enabled) await autostartEnable()
  else await autostartDisable()
}

// 設定 — 瀏覽器預覽模式下提供 mock,以免 UI 卡住
const MOCK_CONFIG = {
  server: { listen_ip: '0.0.0.0', port: 18080, auto_start: true },
  cloud: {
    api_base: '',
    timeout_secs: 30,
    retry: 3,
    allow_invalid_certs: false,
    job_user: '物流貓',
    parcel_mode: 'forward',
    parcel_forward_path: '/api/v2/order-forward-print',
    parcel_proxy_path: '/api/v2/order-proxy-print',
    session_path: '/api/v1/local-middleware/session',
    scan_print_path: '/api/v1/local-middleware/label/scan-print',
    pre_generate_path: '/api/v1/local-middleware/label/pre-generate',
    package_orders_path: '/api/v1/local-middleware/label/package-orders',
    orders_by_date_path: '/api/v1/local-middleware/label/orders-by-date',
    cloud_print_path: '/api/v1/local-middleware/label/cloud-print',
    examine_package_path: '/api/v1/local-middleware/label/examine-package',
    clearance_options_path: '/api/v1/local-middleware/clearance/options',
    clearance_store_path: '/api/v1/local-middleware/clearance/store',
    clearance_dispatch_path: '/api/v1/local-middleware/clearance/dispatch',
    webhook_path: '/webhook/logistic-cat',
  },
  cache: { dir: '', keep_days: 5, max_size_mb: 0 },
  camera: { enabled: false, device_index: 0, jpeg_quality: 80, zoom: 1, captures_dir: '', keep_days: 90 },
  sync: { enabled: false, reverb_host: '', reverb_port: 443, reverb_scheme: 'wss', reverb_app_key: '' },
  sort_only: { enabled: false },
  network: {
    interval_secs: 15,
    degrade_interval_secs: 60,
    anchor_addr: '1.1.1.1:443',
    anchor_timeout_ms: 1500,
    cloud_timeout_secs: 3,
    cloud_interval_secs: 180,
    fail_threshold: 2,
  },
  label_path: {
    mode: 'local',
    share_root: '',
  },
  pre_gen_schedule: {
    enabled: false,
    times: ['06:00', '13:00'],
    sources: ['clearance', 'transfer'],
    date_offset_days: 0,
    catch_up_on_start: false,
  },
}
export const getConfig = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_CONFIG))
  return await invoke('get_config')
}
export const updateConfig = async newConfig => {
  if (!isTauri) return JSON.parse(JSON.stringify(newConfig))
  return await invoke('update_config', { newConfig })
}
// 即時調整數位變焦(不存檔):相機預覽對話框拖滑桿時呼叫,後端下一幀就套用到 MJPEG 串流
export const cameraSetZoom = async zoom => {
  if (!isTauri) return
  return await invoke('camera_set_zoom', { zoom })
}
// 手動拍一張:抓當下最新一幀存進存證目錄,回相對 key(相機未啟用/無幀時回 null)
export const cameraCaptureNow = async () => {
  if (!isTauri) return null
  return await invoke('camera_capture_now')
}
// 面單預產自動排程的可觀測狀態(排程啟動 / 上次執行),供前端顯示確認排程是否運作
export const getPregenStatus = async () => {
  if (!isTauri) {
    return {
      scheduler_started_at: '2026-06-17 09:00:00',
      last_run_at: '2026-06-17 13:00:12',
      last_trigger: '13:00',
      last_ok: 126, last_fail: 2, last_skipped: 4, last_empty: 0,
      last_had_error: true,
      last_failed_sns: [
        { sn: 'SF1234567890', source: 'clearance', reason: '雲端逾時' },
        { sn: 'YT9876543210', source: 'transfer', reason: '門市已關轉' },
      ],
    }
  }
  return await invoke('get_pregen_status')
}

// 面單預產「今日已預產的 order_sn」快照(自動排程 + 手動頁共用去重,cache_day 範圍)
export const pregenDoneSnapshot = async () => {
  if (!isTauri) return { cache_day: new Date().toISOString().slice(0, 10), order_sns: [] }
  return await invoke('pregen_done_snapshot')
}

// 標記一批 order_sn 為「今日已預產」(寫回後端共用去重)
export const pregenMarkDone = async (orderSns) => {
  if (!isTauri) return
  return await invoke('pregen_mark_done', { orderSns })
}

// 清除「今日已預產」記憶(後端 DB + 記憶體)
export const pregenClearDone = async () => {
  if (!isTauri) return
  return await invoke('pregen_clear_done')
}

// 雲端
const MOCK_SESSION = { logged_in: false, api_base: '', user_label: null }
export const cloudPing = () => invoke('cloud_ping')
export const cloudLogin = (apiBase, token) => invoke('cloud_login', { req: { api_base: apiBase, token } })
export const cloudLogout = () => invoke('cloud_logout')
export const cloudSession = async () => {
  if (!isTauri) return { ...MOCK_SESSION }
  return await invoke('cloud_session')
}
export const cloudFetchLabel = (orderSn, { printTypes = ['ALL'], enforce = false, mode = 'web_print', scannerUser = '', stickerUser = '', force = false } = {}) =>
  invoke('cloud_fetch_label', { req: { order_sn: orderSn, print_type: printTypes, enforce, mode, scanner_user: scannerUser, sticker_user: stickerUser, force_refetch: force } })
export const cloudFetchCloudPrint = (orderSn, { printTypes = ['ALL'], enforce = false, packageSn = '', scannerUser = '', stickerUser = '' } = {}) =>
  invoke('cloud_fetch_cloud_print', { req: { order_sn: orderSn, print_type: printTypes, enforce, package_sn: packageSn, scanner_user: scannerUser, sticker_user: stickerUser } })
export const cloudExaminePackage = shipmentNo =>
  invoke('cloud_examine_package', { req: { shipment_no: shipmentNo } })
// 面單預產:袋號反查整袋訂單編號
export const cloudPackageOrders = async packageSn => {
  if (!isTauri) {
    return { respond_code: 'FIND-PACKAGE-ORDER', package_sn: packageSn, order_sns: ['SO-MOCK-1', 'SO-MOCK-2', 'SO-MOCK-3'] }
  }
  return await invoke('cloud_package_orders', { req: { package_sn: packageSn } })
}
// 面單預產:依日期反查整批訂單編號(source: clearance 清關 / transfer 轉寄出貨)
export const cloudOrdersByDate = async (date, source = 'clearance') => {
  if (!isTauri) {
    return { respond_code: 'FIND-PACKAGE-ORDER', date, source, package_count: source === 'clearance' ? 2 : 0, order_sns: ['SO-MOCK-1', 'SO-MOCK-2', 'SO-MOCK-3', 'SO-MOCK-4'] }
  }
  return await invoke('cloud_orders_by_date', { req: { date, source } })
}

// 清關作業 — 選項(倉庫 / 清關公司 / 司機 歷史清單,供下拉;欄位仍可自由輸入)
export const cloudClearanceOptions = async () => {
  if (!isTauri) {
    return {
      storages: [
        { value: '33843', title: '桃園' },
        { value: '41466', title: '台中' },
      ],
      clearance_companies: [
        { value: '宏遠報關', title: '宏遠報關' },
        { value: '聯興報關', title: '聯興報關' },
      ],
      drivers: [
        { value: '王大明', title: '王大明' },
        { value: '陳小華', title: '陳小華' },
      ],
    }
  }
  return await invoke('cloud_clearance_options')
}

// 清關進度浮動框 — 指定報關日區間 → 袋數/件數/已印/剩餘 + 各件列印狀態(供去重遞減)
export const clearanceProgress = async (from, to = '') => {
  if (!isTauri) {
    const parcels = Array.from({ length: 14 }, (_, i) => ({
      shipping_no: `74Z0100${1000 + i}`,
      package_sn: i < 8 ? 'XYE2619201' : '0Y9T5714',
      printed: i < 9,
    }))
    const printed = parcels.filter(p => p.printed).length
    const bagRemaining = new Set(parcels.filter(p => !p.printed).map(p => p.package_sn)).size
    return { respond_code: 'OK', from, to: to || from, bag_total: 2, bag_remaining: bagRemaining, parcel_total: parcels.length, printed, remaining: parcels.length - printed, parcels }
  }
  return await invoke('cloud_clearance_progress', { req: { from, to: to || from } })
}

// 清關進度浮動框 — 設定要即時追蹤的報關日(開啟/換日期傳日期陣列;關閉傳空陣列退訂)
export const progressSetDates = async (dates = []) => {
  if (!isTauri) return
  return await invoke('progress_set_dates', { dates })
}

// 清關作業 — 新增包裹(transport_package_sn 為逗號分隔多筆袋號)
export const cloudClearanceStore = async ({ transportPackageSn, clearanceCompany = '', clearanceDate = '', storageCode = '33843' }) => {
  if (!isTauri) {
    const n = transportPackageSn.split(/[,\s]+/).filter(Boolean).length
    return { success: true, upserted: n, confirm_success: n, confirm_failed: 0, message: `已新增 ${n} 個包裹，訂單自動確認成功 ${n} / 失敗 0` }
  }
  return await invoke('cloud_clearance_store', {
    req: { transport_package_sn: transportPackageSn, clearance_company: clearanceCompany, clearance_date: clearanceDate, storage_code: storageCode },
  })
}
// 清關作業 — 司機派工
export const cloudClearanceDispatch = async ({ transportPackageSn, driverName, shippingDate = '', storageCode = '33843' }) => {
  if (!isTauri) {
    const n = transportPackageSn.split(/[,\s]+/).filter(Boolean).length
    return { success: true, dispatched: n, driver_name: driverName, message: `已派工 ${n} 個包裹給司機「${driverName}」` }
  }
  return await invoke('cloud_clearance_dispatch', {
    req: { transport_package_sn: transportPackageSn, driver_name: driverName, shipping_date: shippingDate, storage_code: storageCode },
  })
}

// 入倉驗單 — 邏輯全在雲端 WarehouseScannerService,中介端透傳。
// 選項(倉庫 / 物流商)
export const warehouseOptions = async () => {
  if (!isTauri) {
    return {
      warehouses: { '41466': '台中', '33843': '桃園' },
      providers: [
        { value: '7', title: '7-ELEVEN' }, { value: 'F', title: 'FamilyMart' }, { value: 'O', title: '萊爾富' },
        { value: 'M', title: 'OK超商' }, { value: 'C', title: '黑貓宅急便' }, { value: 'H', title: '新竹物流' },
        { value: 'P', title: '宅配通' }, { value: 'E', title: '順豐速運' }, { value: 'S', title: '蝦皮店到店' },
        { value: 'B', title: '自寄' }, { value: '4', title: '四海' }, { value: '8', title: '八方' },
      ],
    }
  }
  return await invoke('warehouse_options')
}
// 建立 / 載入箱號(payload = 整個表單物件)
export const warehouseCreatePackage = async (payload) => {
  if (!isTauri) {
    const sn = (payload.return_provider || '7') + String(payload.serial_number || 1).padStart(3, '0') + (payload.return_date || '').replace(/-/g, '').slice(2)
    return { respond_code: 'FIND-PACKAGE-GOODS', respond_message: '箱號載入成功', storage_warehouse: payload.storage_warehouse, package_sn: sn, goods_list: [], goods_total: 0 }
  }
  return await invoke('warehouse_create_package', { req: payload })
}
// 驗單入倉(payload = 整個表單物件)
export const warehouseExamine = async (payload) => {
  if (!isTauri) {
    const sn = (payload.return_provider || '7') + String(payload.serial_number || 1).padStart(3, '0') + (payload.return_date || '').replace(/-/g, '').slice(2)
    return {
      respond_code: 'FIND-PACKAGE-GOODS', respond_message: '包裹入倉成功', storage_warehouse: payload.storage_warehouse, package_sn: sn,
      goods_list: [{ log_id: Date.now(), shipment_no: payload.shipment_no, suppliers_seller: '測試賣家', log_time: new Date().toISOString().slice(0, 19).replace('T', ' '), scanner_user: payload.scanner_user }],
      goods_total: 1,
    }
  }
  return await invoke('warehouse_examine', { req: payload })
}
// 移除單一商品
export const warehouseRemoveGoods = async (shipmentNo) => {
  if (!isTauri) return { respond_code: 'FIND-PACKAGE-GOODS', respond_message: '商品已移除', goods_list: [], goods_total: 0 }
  return await invoke('warehouse_remove_goods', { req: { shipment_no: shipmentNo } })
}
// 刪除整個箱號
export const warehouseRemovePackage = async (storageWarehouse, packageSn) => {
  if (!isTauri) return { respond_code: 'SUCCESS', respond_message: '箱號已刪除' }
  return await invoke('warehouse_remove_package', { req: { storage_warehouse: storageWarehouse, package_sn: packageSn } })
}
// 取箱標列印資料
export const warehouseLabelData = async (storageWarehouse, packageSn, endNum = 0, continuous = false) => {
  if (!isTauri) {
    return { labels: [{ package_sn: packageSn, provider_code: packageSn.slice(0, 1), provider_name: '7-11', serial_number: packageSn.slice(1, 4), date_part: packageSn.slice(4), package_date: '06-28', warehouse_name: '台中', package_remarks: '' }], error: null }
  }
  return await invoke('warehouse_label_data', { req: { storage_warehouse: storageWarehouse, package_sn: packageSn, end_num: endNum, continuous } })
}
// 箱標本地列印(labels 來自 warehouseLabelData)
export const warehousePrintLabels = async (printerName, labels) => {
  if (!isTauri) return labels.length
  return await invoke('warehouse_print_labels', { req: { printer_name: printerName, labels } })
}

// 印表機
export const listPrinters = () => invoke('list_printers')
export const printImage = ({ printerName, imageBase64, imagePath }) =>
  invoke('print_image', { req: { printer_name: printerName, image_base64: imageBase64, image_path: imagePath } })

// Server
export const serverStatus = async () => {
  if (!isTauri) return { running: false, bind_addr: '' }
  return await invoke('server_status')
}
export const serverRestart = () => invoke('server_restart')

// 雲端查件異常記錄(門市關轉 等)
export const recentParcelAlerts = async () => {
  if (!isTauri) {
    return [
      { id: 2, kind: 'store_closed', code: 'STORE_CLOSED', query_no: 'SF1398806613', shipping_no: 'SF1398806613', message: '門市關轉,請改投其他門市', channel_code: 'L3', created_at: '2026-06-18 14:24:02' },
      { id: 1, kind: 'not_found', code: 'NOT_FOUND', query_no: 'SF0000000001', shipping_no: null, message: '查無此件', channel_code: null, created_at: '2026-06-18 13:10:55' },
    ]
  }
  return await invoke('recent_parcel_alerts')
}

// 本機 LAN IP 清單(工控機連線位址)
export const localLanIps = async () => {
  if (!isTauri) {
    return {
      ips: [
        { name: 'en0', ip: '192.168.1.50' },
        { name: 'en1', ip: '10.0.0.12' },
      ],
      port: 18080,
    }
  }
  return await invoke('local_lan_ips')
}

// Queue
export const queueStats = async () => {
  if (!isTauri) return { pending: 0, sending: 0, success: 0, failed: 0 }
  return await invoke('queue_stats')
}
export const queueList = ({ status = null, keyword = null, limit = 100, offset = 0 } = {}) =>
  invoke('queue_list', { req: { status, keyword, limit, offset } })
export const queueRetryFailed = () => invoke('queue_retry_failed')
export const queuePurge = ({ status = 'success', olderThanDays = 7 } = {}) =>
  invoke('queue_purge', { req: { status, older_than_days: olderThanDays } })

// Cache
export const cacheStats = async () => {
  if (!isTauri) return { file_count: 0, total_bytes: 0, hit_count: 0, miss_count: 0, hit_rate: 0 }
  return await invoke('cache_stats')
}
export const cacheClear = () => invoke('cache_clear')

// Event log
export const eventLogList = async ({ level = null, category = null, keyword = null, limit = 200, offset = 0 } = {}) => {
  if (!isTauri) return []
  return await invoke('event_log_list', { req: { level, category, keyword, limit, offset } })
}
export const dailyStats = async ({ days = 7 } = {}) => {
  if (!isTauri) return []
  return await invoke('daily_stats', { req: { days } })
}

// 指派物流(物流商主檔)
const MOCK_DISPATCH = [
  { code: 'S', name: '順豐速運', sort_order: 0, print_profile: 'PROFILE_S', printer_name: null },
  { code: 'B', name: '黑貓宅急便', sort_order: 1, print_profile: 'PROFILE_B', printer_name: null },
  { code: 'P', name: '中華郵政', sort_order: 2, print_profile: 'PROFILE_P', printer_name: null },
]
export const dispatchProviderList = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_DISPATCH))
  return await invoke('dispatch_provider_list')
}
export const dispatchProviderUpsert = ({ code, name, sortOrder = 0, printProfile = null, printerName = null }) => {
  if (!isTauri) return Promise.resolve()
  return invoke('dispatch_provider_upsert', {
    req: { code, name, sort_order: sortOrder, print_profile: printProfile || null, printer_name: printerName || null },
  })
}
export const dispatchProviderDelete = code => {
  if (!isTauri) return Promise.resolve(0)
  return invoke('dispatch_provider_delete', { code })
}

// 分揀通道
const POSITIONS = ['L1', 'L2', 'L3', 'L4', 'R1', 'R2', 'R3', 'R4']
const MOCK_CHANNELS = POSITIONS.map(p => ({
  position: p,
  channel_code: null,
  dispatch_codes: [],
  job_sticker: null,
  enabled: true,
}))
export const sortChannelList = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_CHANNELS))
  return await invoke('sort_channel_list')
}
export const sortChannelSave = ({ position, channelCode, dispatchCodes, jobSticker }) => {
  if (!isTauri) return Promise.resolve()
  return invoke('sort_channel_save', {
    req: {
      position,
      channel_code: channelCode || null,
      dispatch_codes: Array.isArray(dispatchCodes) ? dispatchCodes : [],
      job_sticker: jobSticker || null,
    },
  })
}
export const sortChannelSetEnabled = (position, enabled) => {
  if (!isTauri) return Promise.resolve()
  return invoke('sort_channel_set_enabled', { position, enabled })
}
export const sortChannelUnassignedGet = async () => {
  if (!isTauri) return null
  return await invoke('sort_channel_unassigned_get')
}
export const sortChannelUnassignedSave = (code) => {
  if (!isTauri) return Promise.resolve()
  return invoke('sort_channel_unassigned_save', { code: code || null })
}
export const stickerHistoryList = async () => {
  if (!isTauri) return ['王小明', '陳大華', '林美麗']
  return await invoke('sticker_history_list')
}
export const stickerHistoryAdd = name => {
  if (!isTauri) return Promise.resolve()
  return invoke('sticker_history_add', { name })
}
export const stickerHistoryDelete = name => {
  if (!isTauri) return Promise.resolve(0)
  return invoke('sticker_history_delete', { name })
}

// Parcel 請求記錄 (GET /api/parcel 的查詢歷史)
const MOCK_PARCEL_QUERY_LOG = Array.from({ length: 60 }, (_, i) => ({
  response_id: 16021200 - i,
  query_no: `TPNFXLKIMG${5 - (i % 5)}`,
  tracking_no: `7108158${570 + i}`,
  shipping_provider: ['H', 'F', 'E', '7', 'C'][i % 5],
  sort_channel: ['L1', 'L2', 'R1', 'R2'][i % 4],
  print_profile: ['EPSON L6190', 'HP M404dn', null][i % 3],
  should_print: 1,
  label_key: `HCT/20260513/${(700000 + i).toString(16)}.png`,
  created_at: `2026-05-18T${10 - Math.floor(i / 12)}:${String(60 - i).padStart(2, '0')}:00`,
  // 每 3 筆有一張讀碼站快照(獨立 captures 目錄,key 無前綴;預覽模式檔案不存在,VImg 落 error 樣板)
  photo_path: i % 3 === 0 ? `${16021200 - i}_20260626001425.jpg` : null,
}))
export const parcelQueryLogList = async ({ keyword = null, msField = null, minMs = null, limit = 25, offset = 0 } = {}) => {
  if (!isTauri) {
    const kw = (keyword || '').toLowerCase()
    let list = MOCK_PARCEL_QUERY_LOG
    if (kw) {
      list = list.filter(r =>
        (r.query_no || '').toLowerCase().includes(kw) ||
        (r.tracking_no || '').toLowerCase().includes(kw) ||
        (r.label_key || '').toLowerCase().includes(kw)
      )
    }
    if (minMs > 0) {
      list = list.filter(r => {
        if (msField === 'cloud') return (r.cloud_ms ?? 0) >= minMs
        if (msField === 'label') return (r.label_ms ?? 0) >= minMs
        if (msField === 'total') return (r.total_ms ?? 0) >= minMs
        return (r.cloud_ms ?? 0) >= minMs || (r.label_ms ?? 0) >= minMs || (r.total_ms ?? 0) >= minMs
      })
    }
    return { items: list.slice(offset, offset + limit), total: list.length }
  }
  return await invoke('parcel_query_log_list', { req: { keyword, ms_field: msField, min_ms: minMs, limit, offset } })
}

// 網路健康偵測
const MOCK_NET_HEALTH = {
  anchor: { kind: 'ok', latency_ms: 23 },
  cloud_api: { kind: 'not_configured' },
  anchor_fail_streak: 0,
  cloud_fail_streak: 0,
  anchor_effective_ok: true,
  cloud_effective_ok: null,
  fail_threshold: 2,
  effective_interval_secs: 15,
  checked_at_ms: Date.now(),
}
export const networkHealthGet = async () => {
  if (!isTauri) return MOCK_NET_HEALTH
  return await invoke('network_health_get')
}
export const networkHealthCheck = async () => {
  if (!isTauri) return MOCK_NET_HEALTH
  return await invoke('network_health_check')
}

// 印單統計 — 三來源(scan/auto/ipc)的 shipping_no 去重計數
const MOCK_STATS_SUMMARY = {
  since_reset_at: new Date(Date.now() - 4 * 3600 * 1000).toISOString().slice(0, 19).replace('T', ' '),
  since_reset: 87,
  packages_since_reset: 6,
  past_24h: 124,
  packages_past_24h: 12,
  last_7_days: 1023,
  packages_last_7_days: 84,
  last_30_days: 3782,
  packages_last_30_days: 312,
  range_total: 124,
  packages_range_total: 12,
  noread_range_total: 3,
  by_source: [
    { source: 'scan', count: 36, package_count: 0 },
    { source: 'auto', count: 78, package_count: 12 },
    { source: 'ipc', count: 10, package_count: 0 },
  ],
}
const todayStr = () => new Date().toISOString().slice(0, 10)
const dateOffset = (d) => {
  const t = new Date()
  t.setDate(t.getDate() + d)
  return t.toISOString().slice(0, 10)
}
const MOCK_STATS_DAILY = Array.from({ length: 7 }, (_, i) => ({
  date: dateOffset(-6 + i),
  count: [42, 88, 120, 96, 150, 198, 124][i],
}))
const MOCK_STATS_HOURLY = (() => {
  const h = new Date().getHours()
  return Array.from({ length: 4 }, (_, i) => ({
    hour: `${String((h - 3 + i + 24) % 24).padStart(2, '0')}:00`,
    count: [12, 28, 41, 35][i],
  }))
})()
const MOCK_STATS_PROVIDERS = [
  { provider_code: 'H', count: 58 },
  { provider_code: 'C', count: 34 },
  { provider_code: '7', count: 18 },
  { provider_code: 'F', count: 9 },
  { provider_code: 'E', count: 5 },
]
const MOCK_STATS_STICKERS = [
  { sticker_user: '小美', count: 64 },
  { sticker_user: '阿明', count: 41 },
  { sticker_user: '志強', count: 19 },
]

export const printStatsSummary = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_SUMMARY
  return await invoke('print_stats_summary', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsDaily = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_DAILY
  return await invoke('print_stats_daily', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsHourly = async () => {
  if (!isTauri) return MOCK_STATS_HOURLY
  return await invoke('print_stats_hourly')
}
// 指定區間逐小時(單日趨勢用):選取區間為 1 天時,每日趨勢改顯示該日小時趨勢
export const printStatsHourlyRange = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_HOURLY
  return await invoke('print_stats_hourly_range', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsByProvider = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_PROVIDERS
  return await invoke('print_stats_by_provider', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsBySticker = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_STICKERS
  return await invoke('print_stats_by_sticker', { req: { start_date: startDate, end_date: endDate } })
}

// 階段 2-5 擴充統計 + 重置 ===================================================
const MOCK_STATS_SCANNERS = [
  { scanner_user: '阿仁', count: 92 },
  { scanner_user: '小婷', count: 48 },
]
const MOCK_STATS_CHANNELS = [
  { channel_code: 'A01', position: 'L1', dispatch_code: 'H', count: 64 },
  { channel_code: 'A02', position: 'R2', dispatch_code: 'C', count: 41 },
  { channel_code: 'A03', position: 'L3', dispatch_code: '7', count: 23 },
  { channel_code: 'B99', position: null, dispatch_code: null, count: 9 },
]
const MOCK_STATS_HEATMAP = Array.from({ length: 7 }, (_, w) =>
  Array.from({ length: 24 }, (_, h) => ({ weekday: w, hour: h, count: Math.floor(Math.random() * 30) })),
).flat()
const MOCK_STATS_REPRINT = [
  { print_times: 1, shipment_count: 412 },
  { print_times: 2, shipment_count: 38 },
  { print_times: 3, shipment_count: 6 },
]
const MOCK_STATS_PROVIDER_SOURCE = [
  { provider_code: 'H', source: 'auto', count: 40 },
  { provider_code: 'H', source: 'scan', count: 18 },
  { provider_code: 'C', source: 'auto', count: 30 },
  { provider_code: 'C', source: 'ipc', count: 4 },
]
const MOCK_STATS_FAILURE = {
  failure_count: 7,
  success_count: 124,
  failure_rate: 0.0534,
  by_code: [
    { respond_code: 'NO-DATA', count: 3 },
    { respond_code: 'PRINT-ERROR', count: 2 },
    { respond_code: 'ABNORMAL-SHIPMENT', count: 2 },
  ],
}
const MOCK_STATS_COMPARE = [
  { label: 'week', current: 1023, previous: 932, delta_ratio: 0.0976 },
  { label: 'month', current: 3782, previous: 3520, delta_ratio: 0.0744 },
]

export const printStatsByScanner = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_SCANNERS
  return await invoke('print_stats_by_scanner', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsByChannel = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_CHANNELS
  return await invoke('print_stats_by_channel', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsHeatmap = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_HEATMAP
  return await invoke('print_stats_heatmap', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsReprint = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_REPRINT
  return await invoke('print_stats_reprint', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsProviderSource = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_PROVIDER_SOURCE
  return await invoke('print_stats_provider_source', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsFailure = async ({ startDate = '', endDate = '' } = {}) => {
  if (!isTauri) return MOCK_STATS_FAILURE
  return await invoke('print_stats_failure', { req: { start_date: startDate, end_date: endDate } })
}
export const printStatsCompare = async () => {
  if (!isTauri) return MOCK_STATS_COMPARE
  return await invoke('print_stats_compare')
}
export const workSessionReset = async () => {
  if (!isTauri) return new Date().toISOString().slice(0, 19).replace('T', ' ')
  return await invoke('work_session_reset')
}

// 分揀袋件核對 — 後端常駐記憶體狀態(由工控機 GET /api/parcel 事件驅動更新,
// 並 emit 'bag-check-updated' 推播)。前端只讀快照(切頁保留)與清除。
// 假資料設計成新保留邏輯的「prune 後快照」:未印件袋(missing>0)全部保留(超過舊 3 上限)、
// 已完成袋(missing==0)只留最新一個。front = 最新。
const MOCK_BAG_CHECK = [
  {
    package_sn: 'BAG20260606-006',
    total: 6,
    printed: 3,
    missing: 3,
    last_request_at: '2026-06-06 22:41:12',
    orders: [
      { shipping_no: '7108158660', order_sn: 'SO2606060061', shipping_provider: 'H', last_print_time: '2026-06-06 22:40:30' },
      { shipping_no: '7108158661', order_sn: 'SO2606060062', shipping_provider: 'H', last_print_time: '2026-06-06 22:40:48' },
      { shipping_no: '7108158662', order_sn: 'SO2606060063', shipping_provider: 'H', last_print_time: '2026-06-06 22:41:12' },
      { shipping_no: '7108158663', order_sn: 'SO2606060064', shipping_provider: 'C', last_print_time: null },
      { shipping_no: '7108158664', order_sn: 'SO2606060065', shipping_provider: 'C', last_print_time: null },
      { shipping_no: '7108158665', order_sn: 'SO2606060066', shipping_provider: 'E', last_print_time: null },
    ],
  },
  {
    package_sn: 'BAG20260606-005',
    total: 3,
    printed: 2,
    missing: 1,
    interrupted: true, // 中途被打斷示範:還缺 1 件就出現別的袋號(web preview 展示紅徽章)
    last_request_at: '2026-06-06 22:38:40',
    orders: [
      { shipping_no: '7108158650', order_sn: 'SO2606060051', shipping_provider: 'F', last_print_time: '2026-06-06 22:38:05' },
      { shipping_no: '7108158651', order_sn: 'SO2606060052', shipping_provider: 'F', last_print_time: '2026-06-06 22:38:40' },
      { shipping_no: '7108158652', order_sn: 'SO2606060053', shipping_provider: 'O', last_print_time: null },
    ],
  },
  {
    package_sn: 'BAG20260606-003',
    total: 5,
    printed: 3,
    missing: 2,
    last_request_at: '2026-06-06 22:34:50',
    orders: [
      { shipping_no: '7108158630', order_sn: 'SO2606060031', shipping_provider: '7', last_print_time: '2026-06-06 22:34:10' },
      { shipping_no: '7108158631', order_sn: 'SO2606060032', shipping_provider: '7', last_print_time: '2026-06-06 22:34:32' },
      { shipping_no: '7108158632', order_sn: 'SO2606060033', shipping_provider: '7', last_print_time: '2026-06-06 22:34:50' },
      { shipping_no: '7108158633', order_sn: 'SO2606060034', shipping_provider: 'S', last_print_time: null },
      { shipping_no: '7108158634', order_sn: 'SO2606060035', shipping_provider: 'S', last_print_time: null },
    ],
  },
  {
    package_sn: 'BAG20260606-002',
    total: 4,
    printed: 2,
    missing: 2,
    last_request_at: '2026-06-06 22:33:08',
    orders: [
      { shipping_no: '7108158620', order_sn: 'SO2606060021', shipping_provider: 'H', last_print_time: '2026-06-06 22:32:50' },
      { shipping_no: '7108158621', order_sn: 'SO2606060022', shipping_provider: 'H', last_print_time: '2026-06-06 22:33:08' },
      { shipping_no: '7108158622', order_sn: 'SO2606060023', shipping_provider: 'C', last_print_time: null },
      { shipping_no: '7108158623', order_sn: 'SO2606060024', shipping_provider: 'C', last_print_time: null },
    ],
  },
  {
    package_sn: 'BAG20260606-001',
    total: 5,
    printed: 5,
    missing: 0,
    last_request_at: '2026-06-06 22:30:55',
    orders: [
      { shipping_no: '7108158570', order_sn: 'SO2606060011', shipping_provider: 'F', last_print_time: '2026-06-06 22:30:01' },
      { shipping_no: '7108158571', order_sn: 'SO2606060012', shipping_provider: 'F', last_print_time: '2026-06-06 22:30:14' },
      { shipping_no: '7108158572', order_sn: 'SO2606060013', shipping_provider: '7', last_print_time: '2026-06-06 22:30:33' },
      { shipping_no: '7108158573', order_sn: 'SO2606060014', shipping_provider: '7', last_print_time: '2026-06-06 22:30:44' },
      { shipping_no: '7108158574', order_sn: 'SO2606060015', shipping_provider: 'E', last_print_time: '2026-06-06 22:30:55' },
    ],
  },
]
export const bagCheckSnapshot = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_BAG_CHECK))
  return await invoke('bag_check_snapshot')
}
export const bagCheckClear = async () => {
  if (!isTauri) return Promise.resolve()
  return await invoke('bag_check_clear')
}

