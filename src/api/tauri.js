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
  cloud: { api_base: '', timeout_secs: 30, retry: 3, allow_invalid_certs: false },
  cache: { dir: '', keep_days: 14, max_size_mb: 0, background_prefetch: true },
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
