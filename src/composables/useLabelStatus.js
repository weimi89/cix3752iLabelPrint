/**
 * 對齊 Materio/resources/js/pages/order-scanner-web-print.vue 的
 * status label / icon / 顏色 map（採 tabler iconify 命名）
 */

export const STATUS_LABEL = {
  'LABEL-PROCESS': '列印成功',
  'SHIPPING-REPEAT': '重複列印',
  'LABEL-UNUSUAL': '不在列印範圍',
  'ORDER-UNUSUAL': '查無此訂單',
  'STORE-CLOSED': '門市已關轉',
  'SHIPPING-UNUSUAL': '訂單狀態異常',
  'UNCONFIRMED-SHIPMENT': '訂單未確認',
  'ERROR-SHIPMENT': '訂單錯誤',
  'ERROR': '系統錯誤',
  'CANCELLED': '已停止',
}

export const STATUS_GROUP_ICON = {
  'SHIPPING-REPEAT': 'tabler-refresh-alert',
  'ORDER-UNUSUAL': 'tabler-package-off',
  'STORE-CLOSED': 'tabler-building-off',
  'SHIPPING-UNUSUAL': 'tabler-truck-off',
  'UNCONFIRMED-SHIPMENT': 'tabler-clock-question',
  'ERROR-SHIPMENT': 'tabler-alert-octagon',
  'LABEL-UNUSUAL': 'tabler-printer-off',
  'ERROR': 'tabler-bug',
  'CANCELLED': 'tabler-player-stop',
}

export const isPrintable = code => code === 'LABEL-PROCESS' || code === 'SHIPPING-REPEAT'
export const isDownloadable = code => code === 'LABEL-PROCESS'

export const statusLabel = code => STATUS_LABEL[code] || code || ''
export const statusIcon = code => STATUS_GROUP_ICON[code] || 'tabler-alert-circle'
export const statusGroupColor = code => code === 'SHIPPING-REPEAT' ? 'warning' : 'error'

export const errorMessageFromException = e => {
  const msg = String(e?.message || e || '')
  if (msg.includes('UNAUTHORIZED') || msg.includes('未登入')) return '雲端未登入或 token 失效'
  if (msg.includes('CLOUD_ERROR')) return '雲端 API 錯誤'
  if (msg.includes('timeout')) return '請求逾時'
  return msg || '系統錯誤'
}
