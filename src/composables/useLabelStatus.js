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

// 把後端例外轉成「操作員看得懂的一句話」。
//
// 後端 AppError 序列化成 { kind, message, detail }:`kind` 才是可靠依據 ——
// `detail` 是給工程診斷用的完整訊息(夾雜 reqwest 英文與完整雲端網址),直接印在畫面上
// 現場只會看到一大片紅字卻不知道要做什麼。原始訊息一律另外進 console,不進畫面。
export const errorMessageFromException = e => {
  const t = getI18n().global.t
  const kind = e?.kind
  if (kind) {
    if (e.detail) console.warn('[api]', kind, e.detail)
    // cloud=雲端回的業務錯誤、input=本機自己擋下的:兩者的 message 本來就是給人看的中文
    if (kind === 'cloud' || kind === 'input') return e.message || t('status.ERROR')
    return t(`error.${kind}`)
  }
  // 沒有 kind:不是 AppError(前端自己丟的、Tauri runtime 的),仍只能靠訊息判斷
  const msg = String(e?.message || e || '')
  if (msg.includes('UNAUTHORIZED') || msg.includes('未登入')) return t('error.cloudNotAuthed')
  if (msg.includes('CLOUD_ERROR')) return t('error.cloudApi')
  if (msg.includes('timeout')) return t('error.timeout')
  return msg || t('status.ERROR')
}
