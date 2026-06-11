// 面單預產「已處理訂單」記憶 —— 僅供面單預產(PreGeneratePage)使用。
//
// 目的:同一快取日內,已成功預產過的訂單編號不再重打雲端請求(避免大量重複請求)。
// 對齊快取機制:面單快取每日約 04:00 清理(04:00~隔日 03:59 為同一「快取日」),
// 因此記憶以「快取日」為界 —— 跨過 04:00 自動失效,讓訂單在新的快取日重新抓取。
// persist 到 localStorage,讓 App 在同一快取日內重啟也仍記得(快取檔通常還在)。
//
// 注意:這是「已成功預產」的樂觀記憶,不即時驗證快取檔是否仍在(快取另有 max_size_mb LRU
// 可能提早驅逐)。即使略過了卻剛好被 LRU 清掉,工控機 / 掃描 / 自動印單路徑仍會在需要時
// 即時補下載,不影響正確性,只是少省一次。

const STORAGE_KEY = 'cix3752iLabelPrint.preGenProcessed'
const CACHE_DAY_BOUNDARY_HOURS = 4 // 快取日界:04:00

// 快取日字串:把時間往前推 4 小時讓 04:00 對齊午夜,再取年-月-日。
//   02:00 → 推到前一天 22:00 → 算前一個快取日
//   05:00 → 推到當天 01:00 → 算當天快取日
const cacheDayKey = (now = new Date()) => {
  const d = new Date(now.getTime() - CACHE_DAY_BOUNDARY_HOURS * 60 * 60 * 1000)
  const p = n => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

const load = () => {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || 'null')
    if (saved && saved.day === cacheDayKey() && Array.isArray(saved.sns)) {
      return { day: saved.day, sns: new Set(saved.sns) }
    }
  } catch { /* 壞值忽略,重新開始 */ }
  return { day: cacheDayKey(), sns: new Set() }
}

let state = load()

// App 長開跨過 04:00 時自動重置(對齊快取已清空),讓訂單在新快取日重抓
const rollover = () => {
  const today = cacheDayKey()
  if (state.day !== today) state = { day: today, sns: new Set() }
}

/** 此訂單編號在本快取日內是否已成功預產過 */
export const isOrderProcessed = sn => { rollover(); return state.sns.has(sn) }

/** 標記訂單編號為「已成功預產」(僅進記憶,persist 由 persistProcessed 統一寫,避免逐筆寫 localStorage) */
export const markOrderProcessed = sn => { rollover(); state.sns.add(sn) }

/** 把本快取日的已處理集合寫入 localStorage(批次結束時呼叫一次即可) */
export const persistProcessed = () => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ day: state.day, sns: [...state.sns] }))
  } catch { /* localStorage 滿/不可用時略過,不影響預產 */ }
}
