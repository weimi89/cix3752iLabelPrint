// 瀏覽器 TTS(SpeechSynthesis)語音廣播 — 給設備異常雙語喊話用。
// Tauri WebView(WKWebView / WebView2)內建 window.speechSynthesis,毋須額外部署。
//
// ⚠️ Windows 注意:越南語(vi-VN)語音包預設常未安裝,偵測不到時會 fallback 成系統預設嗓音,
// 唸越南語會腔調怪甚至唸不出。部署機若需越南語廣播,請在 Windows「設定 → 時間與語言 →
// 語言 → 新增越南語 → 語音」安裝 vi 語音包。偵測結果見 hasVoiceFor()。

// 把 App 的 locale 代碼對應到 BCP-47 語音語言碼
const LOCALE_TO_SPEECH_LANG = {
  'zh-Hant': 'zh-TW',
  'vi-VN': 'vi-VN',
}

export function speechLangOf(locale) {
  return LOCALE_TO_SPEECH_LANG[locale] || locale
}

const isSupported = () =>
  typeof window !== 'undefined' && 'speechSynthesis' in window

// voices 在部分平台是非同步載入(首次為空,稍後觸發 voiceschanged)。
// 這裡快取一份,並在 voiceschanged 時更新。
let cachedVoices = []
const refreshVoices = () => {
  if (!isSupported()) return
  const v = window.speechSynthesis.getVoices()
  if (v && v.length) cachedVoices = v
}
if (isSupported()) {
  refreshVoices()
  // 不同瀏覽器以事件或輪詢補齊;掛一次即可
  window.speechSynthesis.addEventListener?.('voiceschanged', refreshVoices)
}

// 依語音語言碼挑一個最合適的 voice(精準 > 前綴 > 無)
const pickVoice = lang => {
  if (!lang || !cachedVoices.length) return null
  const lower = lang.toLowerCase()
  const prefix = lower.split('-')[0]
  return (
    cachedVoices.find(v => v.lang?.toLowerCase() === lower) ||
    cachedVoices.find(v => v.lang?.toLowerCase().startsWith(prefix)) ||
    null
  )
}

// 該語言是否有可用語音包(供呼叫端決定是否退場 / 提示)
export function hasVoiceFor(lang) {
  refreshVoices()
  return !!pickVoice(lang)
}

// 立即停止並清空待播佇列(避免多筆異常時聲音疊一起)
export function cancelSpeech() {
  if (!isSupported()) return
  try {
    window.speechSynthesis.cancel()
  } catch {
    // ignore
  }
}

/**
 * 依序唸出多段語音。每段 { text, lang } 會排入 speechSynthesis 佇列循序播放。
 * @param {Array<{text: string, lang?: string}>} segments
 * @param {{ rate?: number, volume?: number }} [opts]
 */
export function speak(segments, opts = {}) {
  if (!isSupported() || !Array.isArray(segments)) return
  const rate = opts.rate ?? 1
  const volume = opts.volume ?? 1
  for (const seg of segments) {
    const text = (seg?.text || '').trim()
    if (!text) continue
    try {
      const u = new SpeechSynthesisUtterance(text)
      if (seg.lang) u.lang = seg.lang
      const voice = pickVoice(seg.lang)
      if (voice) u.voice = voice
      u.rate = rate
      u.volume = volume
      window.speechSynthesis.speak(u)
    } catch {
      // 單段失敗不影響其他段
    }
  }
}

export function isSpeechSupported() {
  return isSupported()
}
