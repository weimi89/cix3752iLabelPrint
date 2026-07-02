// 放大音效 composable — 忠實移植 cix3752iWeb 入倉頁 useSoundEffects。
// 倉庫現場吵雜,普通 HTMLAudioElement 音量上限 1.0 不夠;這裡用 Web Audio API + GainNode
// 把音量放大到 volume 倍(預設 8.0),再串一顆 limiter 防爆音。
//
// 入箱成功時 playSound('success', serial) 會播「預錄人聲」box-{serial}.mp3(唸「入第N箱」),
// 缺檔(序號 > 500)才退回 speakFallback(通常是瀏覽器 TTS)。這與設備異常廣播同一設計理念:
// 工控機 TTS 音色/語言包不可靠,預錄人聲離線可用、音色一致。
import { ref, onMounted, onBeforeUnmount } from 'vue'

/**
 * @param {Object} soundMap key→音檔路徑,例:{ success: '/sounds/effect-10.mp3', error: '/sounds/effect-05.mp3' }
 * @param {Object} [options]
 * @param {number} [options.volume=8.0] 音量倍率(>1.0 為放大)
 * @param {string} [options.boxSpeechPath='/sounds/speech'] 箱號語音目錄
 * @param {(boxNumber:number)=>void} [options.speakFallback] 缺預錄檔時的退回播報
 * @returns {{ playSound: Function, playBoxNumber: Function, setVolume: Function }}
 */
export function useAmplifiedSound(soundMap, options = {}) {
  const {
    volume = 8.0,
    boxSpeechPath = '/sounds/speech',
    speakFallback = null,
  } = options

  const buffers = ref({})
  const boxBuffers = ref({})
  let audioCtx = null
  let gainNode = null
  let limiterNode = null

  const ensureContext = () => {
    if (!audioCtx) {
      const AudioCtor = window.AudioContext || window.webkitAudioContext
      if (!AudioCtor) return null

      audioCtx = new AudioCtor()
      gainNode = audioCtx.createGain()
      gainNode.gain.value = volume

      // 純 limiter:threshold 近 0dBFS + 超強 ratio + 極短 attack,只在峰值即將爆音時瞬間壓住,
      // 平時不作用 → gain 放大能實際送到輸出,不被壓縮抵消。
      limiterNode = audioCtx.createDynamicsCompressor()
      limiterNode.threshold.value = -1
      limiterNode.knee.value = 0
      limiterNode.ratio.value = 20
      limiterNode.attack.value = 0.001
      limiterNode.release.value = 0.05

      gainNode.connect(limiterNode)
      limiterNode.connect(audioCtx.destination)

      // 一次性使用者手勢解鎖:macOS(WKWebView)/ Linux(WebKitGTK)要求 AudioContext.resume()
      // 必須由使用者手勢觸發才生效;而播放都在網路 await 之後(手勢已失效)。故在建立當下掛一次
      // pointerdown/keydown 監聽,首次互動即 resume,之後 await 後的播放才出得了聲(否則跨平台靜默無音)。
      const unlock = () => {
        audioCtx?.resume().catch(() => {})
        window.removeEventListener('pointerdown', unlock)
        window.removeEventListener('keydown', unlock)
      }
      window.addEventListener('pointerdown', unlock)
      window.addEventListener('keydown', unlock)
    }

    // 播放前也嘗試恢復(掃描槍 Enter 本身即互動;搭配上面的手勢解鎖雙保險)
    if (audioCtx.state === 'suspended')
      audioCtx.resume().catch(() => {})

    return audioCtx
  }

  const loadBuffer = async (name, path) => {
    const ctx = ensureContext()
    if (!ctx) return
    try {
      const response = await fetch(path)
      if (!response.ok) {
        // 缺檔 / 路徑錯:404 的 fetch 仍 resolve,不檢查會拿到 HTML 錯誤頁去 decode 丟例外被吞,
        // buffer 永不設定 → 之後 playSound 靜默無音。與 loadBoxBuffer 一致先擋掉。
        console.warn(`音效載入失敗 [${name}]: HTTP ${response.status} ${path}`)
        return
      }
      const arrayBuffer = await response.arrayBuffer()
      buffers.value[name] = await ctx.decodeAudioData(arrayBuffer)
    } catch (e) {
      console.warn(`音效載入失敗 [${name}]:`, e)
    }
  }

  onMounted(() => {
    for (const [name, path] of Object.entries(soundMap))
      loadBuffer(name, path)
  })

  onBeforeUnmount(() => {
    buffers.value = {}
    boxBuffers.value = {}
    if (audioCtx) {
      audioCtx.close().catch(() => {})
      audioCtx = null
      gainNode = null
      limiterNode = null
    }
  })

  const setVolume = (value) => {
    if (gainNode) gainNode.gain.value = Math.max(0, value)
  }

  // 播放單一 buffer(共用 gain pipeline)
  const playBuffer = (buffer) => {
    const ctx = ensureContext()
    if (!ctx || !buffer) return
    const source = ctx.createBufferSource()
    source.buffer = buffer
    source.connect(gainNode)
    source.start(0)
  }

  // lazy load 箱號語音 buffer(首次才 fetch,之後快取)
  const loadBoxBuffer = async (boxNumber) => {
    if (boxBuffers.value[boxNumber]) return boxBuffers.value[boxNumber]
    const ctx = ensureContext()
    if (!ctx) return null
    try {
      const response = await fetch(`${boxSpeechPath}/box-${boxNumber}.mp3`)
      if (!response.ok) return null
      const arrayBuffer = await response.arrayBuffer()
      const audioBuffer = await ctx.decodeAudioData(arrayBuffer)
      boxBuffers.value[boxNumber] = audioBuffer
      return audioBuffer
    } catch (e) {
      console.warn(`箱號語音載入失敗 [box-${boxNumber}]:`, e)
      return null
    }
  }

  // 播放箱號預錄人聲;缺檔退回 speakFallback
  const playBoxNumber = async (boxNumber) => {
    const buffer = await loadBoxBuffer(boxNumber)
    if (!buffer) {
      speakFallback?.(boxNumber)
      return
    }
    playBuffer(buffer)
  }

  /**
   * @param {string} type soundMap 的 key
   * @param {number|null} boxNumber 帶箱號且 type==='success' 時,改播箱號預錄人聲(不播 beep)
   */
  const playSound = (type, boxNumber = null) => {
    try {
      if (type === 'success' && boxNumber) {
        playBoxNumber(boxNumber)
        return
      }
      playBuffer(buffers.value[type])
    } catch (e) {
      console.warn('音效播放失敗:', e)
    }
  }

  return { playSound, playBoxNumber, setVolume }
}
