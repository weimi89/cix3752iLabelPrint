import { invoke } from '@tauri-apps/api/core'

// 健康檢查
export const ping = () => invoke('ping')

// 設定
export const getConfig = () => invoke('get_config')
export const updateConfig = newConfig => invoke('update_config', { newConfig })

// 雲端
export const cloudPing = () => invoke('cloud_ping')
export const cloudLogin = (apiBase, token) => invoke('cloud_login', { req: { api_base: apiBase, token } })
export const cloudLogout = () => invoke('cloud_logout')
export const cloudSession = () => invoke('cloud_session')
export const cloudFetchLabel = (orderSn, { printType = 'ALL', enforce = false, mode = 'web_print' } = {}) =>
  invoke('cloud_fetch_label', { req: { order_sn: orderSn, print_type: printType, enforce, mode } })

// 印表機
export const listPrinters = () => invoke('list_printers')
export const printImage = ({ printerName, imageBase64, imagePath }) =>
  invoke('print_image', { req: { printer_name: printerName, image_base64: imageBase64, image_path: imagePath } })

// Server
export const serverStatus = () => invoke('server_status')
export const serverRestart = () => invoke('server_restart')

// Queue
export const queueStats = () => invoke('queue_stats')
