// 工控機 GET /api/parcel 失敗時的全域聲音 + toast 提示
// 後端 server 端在請求失敗時 emit 'parcel-alert' { kind, message, query_no },
// 操作員不在電腦前盯著,靠聲音分辨「這件有問題、要處理」。成功不出聲(高頻會吵)。
import { listen } from '@tauri-apps/api/event'
import { toast } from 'vue3-toastify'
import { useI18n } from 'vue-i18n'
import { playSound } from '@/composables/useSoundEffects'

// kind → 提示音(useSoundEffects 已預留各狀態音效)
const KIND_SOUND = {
  store_closed: 'effect_3', // 門市關轉
  unconfirmed: 'effect_4', // 訂單未確認
  not_found: 'effect_2', // 找不到包裹 / 查無訂單
  not_proxy: 'effect_4',  // 非代寄訂單
  not_forward: 'effect_4', // 非轉寄訂單
  unauthorized: 'effect_2', // 雲端未登入
  error: 'effect_2', // 一般失敗
}
const KIND_TYPE = {
  store_closed: 'warning',
  unconfirmed: 'warning',
  not_found: 'error',
  not_proxy: 'warning',
  not_forward: 'warning',
  unauthorized: 'error',
  error: 'error',
}

export function useParcelAlert() {
  const { t } = useI18n()
  let unlisten = null

  const handle = payload => {
    const kind = payload?.kind || 'error'
    const message = payload?.message || ''
    const queryNo = payload?.query_no || ''

    playSound(KIND_SOUND[kind] || 'effect_2')

    // 標題用中文分類名;雲端原始 message 一併顯示(避免分類失準時操作員看不到真因)
    const label = t(`parcelAlert.${kind}`)
    let text
    if (message && message.trim()) {
      text = message.includes(label) ? message : `${label}:${message}`
    } else {
      text = label
    }
    if (queryNo) text += `(${queryNo})`

    toast(text, { type: KIND_TYPE[kind] || 'error' })
  }

  const start = async () => {
    if (unlisten) return
    unlisten = await listen('parcel-alert', evt => handle(evt.payload))
  }
  const stop = () => {
    if (unlisten) { unlisten(); unlisten = null }
  }

  return { start, stop }
}
