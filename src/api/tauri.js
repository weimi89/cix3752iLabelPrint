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
    cloud_print_path: '/api/v1/local-middleware/label/cloud-print',
    webhook_path: '/webhook/logistic-cat',
  },
  cache: { dir: '', keep_days: 14, max_size_mb: 0 },
}
export const getConfig = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_CONFIG))
  return await invoke('get_config')
}
export const updateConfig = async newConfig => {
  if (!isTauri) return JSON.parse(JSON.stringify(newConfig))
  return await invoke('update_config', { newConfig })
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
export const cloudFetchLabel = (orderSn, { printType = 'ALL', enforce = false, mode = 'web_print' } = {}) =>
  invoke('cloud_fetch_label', { req: { order_sn: orderSn, print_type: printType, enforce, mode } })
export const cloudFetchCloudPrint = (orderSn, { printType = 'ALL', enforce = false, packageSn = '', scannerUser = '', stickerUser = '' } = {}) =>
  invoke('cloud_fetch_cloud_print', { req: { order_sn: orderSn, print_type: printType, enforce, package_sn: packageSn, scanner_user: scannerUser, sticker_user: stickerUser } })
export const cloudExaminePackage = shipmentNo =>
  invoke('cloud_examine_package', { req: { shipment_no: shipmentNo } })

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

// Queue
export const queueStats = async () => {
  if (!isTauri) return { pending: 0, sending: 0, success: 0, failed: 0 }
  return await invoke('queue_stats')
}
export const queueList = ({ status = null, limit = 100, offset = 0 } = {}) =>
  invoke('queue_list', { req: { status, limit, offset } })
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
export const eventLogList = async ({ level = null, category = null, limit = 200, offset = 0 } = {}) => {
  if (!isTauri) return []
  return await invoke('event_log_list', { req: { level, category, limit, offset } })
}
export const dailyStats = async ({ days = 7 } = {}) => {
  if (!isTauri) return []
  return await invoke('daily_stats', { req: { days } })
}

// 指派物流(物流商主檔)
const MOCK_DISPATCH = [
  { code: 'SF', name: '順豐速運', sort_order: 0, print_profile: 'PROFILE_SF' },
  { code: 'BLACK', name: '黑貓宅急便', sort_order: 1, print_profile: 'PROFILE_BLACK' },
  { code: 'POST', name: '中華郵政', sort_order: 2, print_profile: 'PROFILE_POST' },
]
export const dispatchProviderList = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_DISPATCH))
  return await invoke('dispatch_provider_list')
}
export const dispatchProviderUpsert = ({ code, name, sortOrder = 0, printProfile = null }) => {
  if (!isTauri) return Promise.resolve()
  return invoke('dispatch_provider_upsert', {
    req: { code, name, sort_order: sortOrder, print_profile: printProfile || null },
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
  dispatch_code: null,
  job_sticker: null,
}))
export const sortChannelList = async () => {
  if (!isTauri) return JSON.parse(JSON.stringify(MOCK_CHANNELS))
  return await invoke('sort_channel_list')
}
export const sortChannelSave = ({ position, channelCode, dispatchCode, jobSticker }) => {
  if (!isTauri) return Promise.resolve()
  return invoke('sort_channel_save', {
    req: {
      position,
      channel_code: channelCode || null,
      dispatch_code: dispatchCode || null,
      job_sticker: jobSticker || null,
    },
  })
}
export const stickerHistoryList = async () => {
  if (!isTauri) return ['王小明', '陳大華', '林美麗']
  return await invoke('sticker_history_list')
}
export const stickerHistoryDelete = name => {
  if (!isTauri) return Promise.resolve(0)
  return invoke('sticker_history_delete', { name })
}
