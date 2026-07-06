// 工控機 GET /api/parcel 失敗時的全域聲音 + toast 提示
// 後端 server 端在請求失敗時 emit 'parcel-alert' { kind, message, query_no },
// 操作員不在電腦前盯著,靠聲音分辨「這件有問題、要處理」。成功不出聲(高頻會吵)。
import { listen } from '@tauri-apps/api/event'
import { toast } from 'vue3-toastify'
import { useI18n } from 'vue-i18n'
import { playSound } from '@/composables/useSoundEffects'

// kind → 提示音(useSoundEffects 已預留各狀態音效)。值為 null = 只提示不出聲。
const KIND_SOUND = {
  store_closed: 'effect_3', // 門市關轉
  unconfirmed: 'effect_4', // 訂單未確認
  not_found: 'effect_2', // 找不到包裹 / 查無訂單
  not_proxy: 'effect_4',  // 非代寄訂單
  not_forward: 'effect_4', // 非轉寄訂單
  unauthorized: 'effect_2', // 雲端未登入
  error: 'effect_2', // 一般失敗
  noread: null, // 讀碼失敗(NoRead):只 toast 不出聲(高頻讀碼失敗不宜狂響)
}
const KIND_TYPE = {
  store_closed: 'warning',
  unconfirmed: 'warning',
  not_found: 'error',
  not_proxy: 'warning',
  not_forward: 'warning',
  unauthorized: 'error',
  error: 'error',
  noread: 'warning',
}

export function useParcelAlert() {
  const { t } = useI18n()
  let unlisten = null
  let unlistenLabelFailed = null

  const handle = payload => {
    const kind = payload?.kind || 'error'
    const message = payload?.message || ''
    const queryNo = payload?.query_no || ''

    // 已知 kind 依對應音效播放(null=不出聲,如 noread);未知 kind 退回 effect_2。
    // 用 hasOwnProperty.call:既避開 `in` 的原型鏈誤取,又不依賴 ES2022 的 Object.hasOwn
    //(舊 webview 缺 Object.hasOwn 會整個 handler 拋錯 → 所有告警靜默失聲)。
    const snd = Object.prototype.hasOwnProperty.call(KIND_SOUND, kind) ? KIND_SOUND[kind] : 'effect_2'
    if (snd) playSound(snd)

    // 標題用中文分類名;雲端原始 message 一併顯示(避免分類失準時操作員看不到真因)
    const label = t(`parcelAlert.${kind}`)
    let text
    if (message && message.trim()) {
      text = message.includes(label) ? message : `${label}:${message}`
    } else {
      text = label
    }
    if (queryNo) text += `(${queryNo})`

    // NoRead 為高頻事件:用固定 toastId 折疊成單一(就地更新)toast,避免洗版把
    // 真正的雲端失敗告警(未登入 / 門市關轉…)擠出畫面。其他 kind 維持各自堆疊。
    const opts = { type: KIND_TYPE[kind] || 'error' }
    if (kind === 'noread') opts.toastId = 'parcel-noread'
    toast(text, opts)
  }

  // 錯誤面單產生了但「印不出來」(無印表機 / 列印失敗 / 暫存失敗)。
  // 這以前只在後端 tracing::warn,操作員完全無感 → 現在 emit 事件跳 toast,避免靜默盲區。
  const handleLabelFailed = payload => {
    const reason = payload?.reason || 'print_failed'
    const queryNo = payload?.query_no || ''
    playSound('effect_2')
    let text = t(`errorLabelFailed.${reason}`)
    if (queryNo) text += `(${queryNo})`
    toast(text, { type: 'error' })
  }

  const start = async () => {
    if (!unlisten) {
      unlisten = await listen('parcel-alert', evt => handle(evt.payload))
    }
    if (!unlistenLabelFailed) {
      unlistenLabelFailed = await listen('error-label-print-failed', evt => handleLabelFailed(evt.payload))
    }
  }
  const stop = () => {
    if (unlisten) { unlisten(); unlisten = null }
    if (unlistenLabelFailed) { unlistenLabelFailed(); unlistenLabelFailed = null }
  }

  return { start, stop }
}
