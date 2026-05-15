// 對齐 cix3752iWeb 自動印單頁的音效對應(掃描成功/失敗/警告/未確認 …)
// 不引入 @vueuse/sound,直接用 HTML5 Audio,Tauri WebView 內運作良好
const SOUND_MAP = {
  effect_1: '/sounds/effect-10.mp3', // 列印成功
  effect_2: '/sounds/effect-05.mp3', // 一般失敗 / 異常
  effect_3: '/sounds/effect-09.mp3', // 門市關轉
  effect_4: '/sounds/effect-07.mp3', // 包裹未確認
  effect_5: '/sounds/effect-08.mp3', // 重覆列印 / 提示
  effect_6: '/sounds/effect-04.mp3', // 預留
}

// 預先 new Audio() 避免每次 play 都重新載入
const cache = {}
const getAudio = key => {
  if (!cache[key]) {
    const url = SOUND_MAP[key]
    if (!url) return null
    cache[key] = new Audio(url)
    cache[key].preload = 'auto'
  }
  return cache[key]
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
