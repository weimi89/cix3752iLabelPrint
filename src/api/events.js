/**
 * 事件訂閱 — 介面與 Tauri 的 `listen` 完全相同,頁面程式碼兩種環境共用。
 *
 *     import { listen } from '@/api/events'
 *     const unlisten = await listen('print-stats-updated', evt => { ... })
 *
 * 桌面走 Tauri 進程內通道;網頁走 `/events/stream`(SSE)。網頁端**所有頁面共用
 * 同一條連線**,在瀏覽器端做分發 —— 每頁各開一條 EventSource 會讓瀏覽器很快
 * 撞到同網域連線數上限,症狀是後開的頁面完全收不到事件。
 */

import { isTauriRuntime, isWebRuntime } from './runtime'

/** event 名 → callback 集合 */
const subscribers = new Map()

let source = null
/** 重連退避:斷線後不要密集重試打爆自己的 server */
let retryDelay = 1000
let retryTimer = null

function dispatch(name, payload) {
  const set = subscribers.get(name)
  if (!set) return
  // 包成與 Tauri 事件相同的形狀,回呼端不必分辨自己跑在哪裡
  const evt = { event: name, payload }
  for (const cb of [...set]) {
    try {
      cb(evt)
    } catch (e) {
      console.error(`[events] ${name} 的處理函式拋出例外`, e)
    }
  }
}

function ensureSource() {
  if (source || typeof EventSource === 'undefined') return
  // 正在退避等待重連時不要另開一條。少了這行,後端重啟期間只要使用者切頁
  // (每頁 onMounted 都會 listen)就會立刻重試,退避形同虛設 ——
  // 變成密集重試打自己的 server,正是這段機制想避免的事。
  if (retryTimer) return

  source = new EventSource('/events/stream', { withCredentials: true })

  source.onmessage = e => {
    retryDelay = 1000
    try {
      const { event, payload } = JSON.parse(e.data)
      dispatch(event, payload)
    } catch (err) {
      console.error('[events] 事件解析失敗', err, e.data)
    }
  }

  source.onerror = () => {
    // EventSource 自帶重連,但 server 重啟期間會連續失敗;
    // 這裡自行關閉並退避重連,避免瀏覽器高頻重試
    source?.close()
    source = null
    if (retryTimer) return
    retryTimer = setTimeout(() => {
      retryTimer = null
      if (subscribers.size > 0) ensureSource()
    }, retryDelay)
    retryDelay = Math.min(retryDelay * 2, 30000)
  }
}

function closeIfIdle() {
  if (subscribers.size > 0) return
  source?.close()
  source = null
  if (retryTimer) {
    clearTimeout(retryTimer)
    retryTimer = null
  }
}

/**
 * 訂閱事件。回傳 unlisten 函式,介面與 Tauri 的 `listen` 一致。
 */
export async function listen(event, handler) {
  if (isTauriRuntime) {
    const { listen: tauriListen } = await import('@tauri-apps/api/event')
    return await tauriListen(event, handler)
  }

  if (!isWebRuntime) {
    // 純瀏覽器預覽:沒有事件來源,回一個空的取消函式讓頁面正常運作
    return () => {}
  }

  let set = subscribers.get(event)
  if (!set) {
    set = new Set()
    subscribers.set(event, set)
  }
  set.add(handler)
  ensureSource()

  return () => {
    const s = subscribers.get(event)
    if (!s) return
    s.delete(handler)
    if (s.size === 0) subscribers.delete(event)
    closeIfIdle()
  }
}
