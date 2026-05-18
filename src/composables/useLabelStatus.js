import { getI18n } from '@/plugins/i18n'

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

const STATUS_CODES = new Set([
  'LABEL-PROCESS', 'SHIPPING-REPEAT', 'LABEL-UNUSUAL', 'ORDER-UNUSUAL',
  'STORE-CLOSED', 'SHIPPING-UNUSUAL', 'UNCONFIRMED-SHIPMENT', 'ERROR-SHIPMENT',
  'ERROR', 'CANCELLED',
])

export const isPrintable = code => code === 'LABEL-PROCESS' || code === 'SHIPPING-REPEAT'
export const isDownloadable = code => code === 'LABEL-PROCESS'

export const statusLabel = code => {
  if (!code) return ''
  const t = getI18n().global.t
  return STATUS_CODES.has(code) ? t(`status.${code}`) : code
}

export const statusIcon = code => STATUS_GROUP_ICON[code] || 'tabler-alert-circle'
export const statusGroupColor = code => code === 'SHIPPING-REPEAT' ? 'warning' : 'error'

export const errorMessageFromException = e => {
  const t = getI18n().global.t
  const msg = String(e?.message || e || '')
  if (msg.includes('UNAUTHORIZED') || msg.includes('未登入')) return t('error.cloudNotAuthed')
  if (msg.includes('CLOUD_ERROR')) return t('error.cloudApi')
  if (msg.includes('timeout')) return t('error.timeout')
  return msg || t('status.ERROR')
}
