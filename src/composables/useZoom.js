/**
 * 頁面縮放(像瀏覽器 Cmd/Ctrl + +/-/0)。
 *
 * - Tauri runtime:走原生 webview setZoom(同 Chrome ⌘± 機制,完全不影響 layout 計算)
 * - 瀏覽器 preview:fallback 到 documentElement.style.zoom(CSS zoom 會干擾 popup 定位,但 dev 預覽可接受)
 * - 等級對齊 Chrome 預設 zoom levels,reset = 1.0
 * - localStorage 持久化,啟動自動套用
 * - 鍵盤快捷:macOS = ⌘±0,其他 = Ctrl±0
 * - Ctrl/⌘ + 滾輪也可縮放
 */
import { ref } from 'vue'

const STORAGE_KEY = 'cix3752iLabelPrint.zoom'
const ZOOM_LEVELS = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0]
const DEFAULT_ZOOM = 1.0

const zoom = ref(DEFAULT_ZOOM)
const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const apply = v => {
  if (isTauri) {
    // 動態 import 避免瀏覽器 preview 模式載入時 Tauri internals 還沒就緒
    import('@tauri-apps/api/webview').then(({ getCurrentWebview }) => {
      getCurrentWebview().setZoom(v).catch(e => console.warn('setZoom 失敗', e))
    })
  } else {
    document.documentElement.style.zoom = String(v)
  }
  localStorage.setItem(STORAGE_KEY, String(v))
  zoom.value = v
}

const nearestIndex = v => {
  let best = 0
  let diff = Infinity
  for (let i = 0; i < ZOOM_LEVELS.length; i++) {
    const d = Math.abs(ZOOM_LEVELS[i] - v)
    if (d < diff) { diff = d; best = i }
  }
  return best
}

export const setZoom = v => {
  const clamped = Math.max(ZOOM_LEVELS[0], Math.min(ZOOM_LEVELS[ZOOM_LEVELS.length - 1], v))
  apply(clamped)
}
export const zoomIn = () => {
  const i = nearestIndex(zoom.value)
  const next = ZOOM_LEVELS[Math.min(i + 1, ZOOM_LEVELS.length - 1)]
  apply(next)
}
export const zoomOut = () => {
  const i = nearestIndex(zoom.value)
  const next = ZOOM_LEVELS[Math.max(i - 1, 0)]
  apply(next)
}
export const zoomReset = () => apply(DEFAULT_ZOOM)

export const useZoom = () => ({ zoom, zoomIn, zoomOut, zoomReset, setZoom })

// 切換到 Tauri 原生 zoom 後,先把先前殘留在 documentElement 上的 CSS zoom 清掉,
// 否則雙重作用 + CSS zoom 會干擾 Vuetify popup 座標
if (isTauri) document.documentElement.style.zoom = ''

// 啟動時讀回上次的 zoom 並套用
const stored = parseFloat(localStorage.getItem(STORAGE_KEY) || '')
if (Number.isFinite(stored) && stored > 0) {
  apply(stored)
}

let listenersInstalled = false
export const installZoomShortcuts = () => {
  if (listenersInstalled) return
  listenersInstalled = true

  window.addEventListener('keydown', e => {
    // macOS 用 metaKey,其他平台 ctrlKey
    const isMod = e.metaKey || e.ctrlKey
    if (!isMod) return

    // + (=) / Numpad+ → zoom in
    if (e.key === '+' || e.key === '=' || e.code === 'NumpadAdd') {
      e.preventDefault()
      zoomIn()
      return
    }
    // - / Numpad- → zoom out
    if (e.key === '-' || e.code === 'NumpadSubtract') {
      e.preventDefault()
      zoomOut()
      return
    }
    // 0 / Numpad0 → reset
    if (e.key === '0' || e.code === 'Numpad0') {
      e.preventDefault()
      zoomReset()
    }
  }, { capture: true })

  // Ctrl/⌘ + 滾輪 → 縮放(避免被頁面捲動吃掉)
  window.addEventListener('wheel', e => {
    if (!(e.metaKey || e.ctrlKey)) return
    e.preventDefault()
    if (e.deltaY < 0) zoomIn()
    else if (e.deltaY > 0) zoomOut()
  }, { passive: false, capture: true })
}
