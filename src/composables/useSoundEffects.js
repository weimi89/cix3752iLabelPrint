// 對齐 cix3752iWeb 自動印單頁的音效對應(掃描成功/失敗/警告/未確認 …)
// 不引入 @vueuse/sound,直接用 HTML5 Audio,Tauri WebView 內運作良好
//
// 提示音可自訂:對照表以 localStorage(PRINT_ALERT_SOUND_STORAGE_KEY)覆寫預設值,
// AutoPrintPage 與全域 parcel-alert(useParcelAlert)共用同一組 effect key,改一處兩邊同時生效。
import { loadStoredSoundMap } from '@/composables/useSoundSettings'

export const PRINT_ALERT_SOUND_STORAGE_KEY = 'cix3752iLabelPrint.printAlertSounds'

export const PRINT_ALERT_SOUND_DEFAULTS = {
  effect_1: '/sounds/effect-10.mp3', // 列印成功
  effect_2: '/sounds/effect-05.mp3', // 一般失敗 / 異常(查無、未登入、列印失敗)
  effect_3: '/sounds/effect-09.mp3', // 門市關轉
  effect_4: '/sounds/effect-07.mp3', // 包裹未確認 / 非代寄 / 非轉寄
}

// 模組載入時即套用已儲存的自訂值:parcel-alert 全域監聽不經過設定頁也要用對的音效
const soundMap = loadStoredSoundMap(PRINT_ALERT_SOUND_STORAGE_KEY, PRINT_ALERT_SOUND_DEFAULTS)

// 預先 new Audio() 避免每次 play 都重新載入
const cache = {}
const getAudio = key => {
  if (!cache[key]) {
    const url = soundMap[key]
    if (!url) return null
    cache[key] = new Audio(url)
    cache[key].preload = 'auto'
  }
  return cache[key]
}

/**
 * 更換單一提示音(SoundSettingsDialog 儲存時呼叫);作廢舊 Audio 快取,下次播放載入新檔。
 * 持久化由 useSoundSettings 統一寫 localStorage,這裡只更新記憶體對照表。
 */
export function setPrintAlertSound(key, path) {
  if (!(key in PRINT_ALERT_SOUND_DEFAULTS) || soundMap[key] === path) return
  soundMap[key] = path
  delete cache[key]
}

export function playSound(key) {
  const audio = getAudio(key)
  if (!audio) return
  try {
    audio.currentTime = 0
    audio.play().catch(() => {
      // 多筆連續播放時 Promise reject 是正常的,吞掉避免 console noise
    })
  } catch {
    // ignore
  }
}
