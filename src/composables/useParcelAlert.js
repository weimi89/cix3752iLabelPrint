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
  let unlistenDirectPrint = null

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

  // DirectPrint(中介機直印)失敗:工控機已拿到成功回應、統計已記,若不主動提示,
  // 分揀線整批漏印而所有畫面顯示正常 —— 最難察覺的靜默故障,必須出聲 + toast。
  // 防洪(印表機離線時分揀線每分鐘數十件、每件都 emit):同 reason 折疊成單一常駐 toast
  //(toastId 就地更新,不堆疊),音效 20s 節流(對齊 useDeviceAlert 慣例),避免 UI 不可用。
  let lastDirectPrintSoundAt = 0
  const handleDirectPrintFailed = payload => {
    const reason = payload?.reason || 'print_failed'
    const queryNo = payload?.query_no || ''
    const now = Date.now()
    if (now - lastDirectPrintSoundAt >= 20000) {
      lastDirectPrintSoundAt = now
      playSound('effect_2')
    }
    let text = t(`directPrintFailed.${reason}`)
    if (queryNo) text += `(${queryNo})`
    // vue3-toastify 對已存在的 toastId 是「丟棄後續呼叫」而非更新 —— 必須顯式 update,
    // 否則後續失敗件的單號不會顯示,操作員只看得到第一件
    const id = `direct-print-failed:${reason}`
    if (toast.isActive(id)) {
      toast.update(id, { render: text, type: 'error', autoClose: false })
    } else {
      toast(text, { type: 'error', autoClose: false, toastId: id })
    }
  }

  const start = async () => {
    if (!unlisten) {
      unlisten = await listen('parcel-alert', evt => handle(evt.payload))
    }
    if (!unlistenLabelFailed) {
      unlistenLabelFailed = await listen('error-label-print-failed', evt => handleLabelFailed(evt.payload))
    }
    if (!unlistenDirectPrint) {
      unlistenDirectPrint = await listen('direct-print-failed', evt => handleDirectPrintFailed(evt.payload))
    }
  }
  const stop = () => {
    if (unlisten) { unlisten(); unlisten = null }
    if (unlistenLabelFailed) { unlistenLabelFailed(); unlistenLabelFailed = null }
    if (unlistenDirectPrint) { unlistenDirectPrint(); unlistenDirectPrint = null }
  }

  return { start, stop }
}
