// 工控機設備異常的全域語音廣播 + toast 提示。
// 後端 server 端收到 POST /api/device-alert 時 emit 'device-alert' { alert_type, message, repeat },
// 前端用「中文 + 越南語」雙語廣播 repeat 次(預設 1、上限 3),提示現場人員到場處理。
//
// 發聲策略:
//   - 已知固定類型 → 播**預錄雙語 mp3**(public/sounds/alert/{type}-zh|vi.mp3,
//     中文 HsiaoChen / 越南語 HoaiMy)。每台工控機音色一致、發音標準、離線可用,
//     **越南語免在 Windows 裝語音包**(這是改用預錄音檔的主因)。
//   - 未知類型(工控機自訂新 type、尚未錄音) → 退回系統 TTS speechSynthesis(盡力而為)。
//   - 工控機帶的自訂 `message` 補充字無法即時合成,只顯示在 toast,不唸出。
//
// 與 useParcelAlert(查件失敗音效)的差異:device-alert 是「機台/硬體出狀況,人要到場排除」,
// 用完整語音喊話;parcel-alert 只是「這件包裹有問題」,放一次提示音即可。
import { listen } from '@tauri-apps/api/event'
import { toast } from 'vue3-toastify'
import { useI18n } from 'vue-i18n'
import { speak, cancelSpeech, speechLangOf, isSpeechSupported } from '@/composables/useSpeech'

// 已預錄雙語音檔的固定異常類型(對應 public/sounds/alert/{type}-{zh|vi}.mp3)
const PRERECORDED = new Set([
  'PARCEL_JAM',
  'USB_DISCONNECT',
  'SCANNER_ERROR',
  'PRINTER_ERROR',
  'ERROR',
])
const ALERT_DIR = '/sounds/alert'
// TTS fallback(未知類型)要唸的語言,對應 App i18n locale
const BROADCAST_LOCALES = ['zh-Hant', 'vi-VN']
// 廣播次數上限(與後端 clamp 一致,前端再防一層)
const MAX_REPEAT = 3

export function useDeviceAlert() {
  const { t } = useI18n()
  let unlisten = null

  // 遞增令牌:新異常進來時讓進行中的播放序列在下一步自行中止
  let playToken = 0
  let currentAudio = null

  // 取某 locale 的異常文案(找不到對應 type 時 fallback 到通用 ERROR 文案)
  const phraseFor = (alertType, locale) => {
    const key = `deviceAlert.${alertType}`
    const text = t(key, {}, { locale })
    if (text === key) return t('deviceAlert.ERROR', {}, { locale })
    return text
  }

  const audioUrls = type => [
    `${ALERT_DIR}/${type.toLowerCase()}-zh.mp3`,
    `${ALERT_DIR}/${type.toLowerCase()}-vi.mp3`,
  ]

  // 停掉進行中的廣播(預錄音檔 + TTS 都停),避免多筆異常聲音疊在一起
  const stopCurrent = () => {
    playToken += 1
    if (currentAudio) {
      try {
        currentAudio.pause()
        currentAudio.currentTime = 0
      } catch {
        // ignore
      }
      currentAudio = null
    }
    cancelSpeech()
  }

  // 播一個音檔,等它播完(或失敗)才 resolve;令牌過期則直接略過
  const playOnce = (url, token) =>
    new Promise(resolve => {
      if (token !== playToken) return resolve()
      try {
        const a = new Audio(url)
        currentAudio = a
        a.onended = () => resolve()
        a.onerror = () => resolve()
        a.play().catch(() => resolve())
      } catch {
        resolve()
      }
    })

  // 依序播 [中文, 越南語] × repeat 次
  const playSequence = async (urls, repeat, token) => {
    for (let i = 0; i < repeat; i++) {
      for (const url of urls) {
        if (token !== playToken) return
        await playOnce(url, token)
      }
    }
    if (token === playToken) currentAudio = null
  }

  const handle = payload => {
    const alertType = (payload?.alert_type || 'ERROR').toUpperCase()
    const extra = (payload?.message || '').trim()
    let repeat = Number(payload?.repeat) || 1
    repeat = Math.min(MAX_REPEAT, Math.max(1, repeat))

    // 先停掉前一筆未播完的,再開新的
    stopCurrent()
    const token = playToken

    if (PRERECORDED.has(alertType)) {
      // 已知類型 → 預錄雙語音檔(穩定音色 + 越南語免裝語音包)
      playSequence(audioUrls(alertType), repeat, token)
    } else if (isSpeechSupported()) {
      // 未知類型 → 退回系統 TTS(越南語需機器有 vi 語音包,否則只唸得出中文)
      const segments = []
      for (let i = 0; i < repeat; i++) {
        for (const locale of BROADCAST_LOCALES) {
          segments.push({ text: phraseFor(alertType, locale), lang: speechLangOf(locale) })
        }
      }
      speak(segments)
    }

    // 視覺備援:畫面跳 toast(用目前 UI 語系),補充字一併顯示
    let toastText = phraseFor(alertType, undefined)
    if (extra) toastText += `（${extra}）`
    toast(toastText, { type: 'warning', autoClose: 6000 })
  }

  const start = async () => {
    if (!unlisten) {
      unlisten = await listen('device-alert', evt => handle(evt.payload))
    }
  }
  const stop = () => {
    if (unlisten) {
      unlisten()
      unlisten = null
    }
    stopCurrent()
  }

  return { start, stop }
}
