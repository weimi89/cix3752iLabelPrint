// 面單預產「已處理訂單」記憶 —— 供面單預產(PreGeneratePage)判斷略過用。
//
// 【2026-07-01 改為後端共用】原本這層記憶存前端 localStorage、只由手動頁自己寫,
// 與「自動排程」的後端記憶體去重各記一份、互不同步 → 自動跑完後手動又全部重打雲端、
// 全報「成功」。現在統一由後端 pregen_done(DB 持久化、cache_day 範圍)當單一來源:
//   - loadProcessed():批次開始前向後端取快照(含自動排程已預產的),載入前端記憶體。
//   - isOrderProcessed():讀前端記憶體(同步、快),決定是否略過、不重打雲端。
//   - markOrderProcessed():標記進記憶體 + pending;persistProcessed() 批次寫回後端。
// cache_day 範圍與跨日清除由後端負責(見 pregen::PregenDoneStore),前端不再自算 04:00 界。
//
// 注意:仍是「已成功預產」的樂觀記憶,不即時驗證快取檔是否仍在(快取另有 max_size_mb LRU
// 可能提早驅逐)。即使略過了卻剛好被 LRU 清掉,工控機 / 掃描 / 自動印單路徑仍會在需要時
// 即時補下載,不影響正確性,只是少省一次。

import { pregenDoneSnapshot, pregenMarkDone, pregenClearDone } from '@/api/tauri'

// 前端記憶體鏡像:sns = 後端快照 + 本批新標記;pending = 尚未寫回後端的新標記
let sns = new Set()
let pending = new Set()

/** 向後端取「今日已預產」快照載入記憶體(批次開始前呼叫,才能吃到自動排程已做的部分) */
export const loadProcessed = async () => {
  try {
    const snap = await pregenDoneSnapshot()
    sns = new Set(Array.isArray(snap?.order_sns) ? snap.order_sns : [])
    // 保留上批 persistProcessed 失敗、退回 pending 尚未寫回後端的標記 —— 否則會被判為
    // 未預產而重打雲端。合併進 sns、且不清 pending(下次 persist 仍會嘗試寫回)。
    for (const sn of pending) sns.add(sn)
  } catch {
    // 取快照失敗不阻塞預產:退回「僅含尚未寫回的 pending」(至少不丟已標記的),
    // 後端 has_local 仍會對真正已快取者免重抓。
    sns = new Set(pending)
  }
}

/** 此訂單編號今日是否已預產過(讀前端記憶體,同步) */
export const isOrderProcessed = sn => sns.has(sn)

/** 標記訂單編號為「已成功預產」(進記憶體 + pending,實際寫回由 persistProcessed 批次處理) */
export const markOrderProcessed = sn => { sns.add(sn); pending.add(sn) }

/** 把本批新標記的訂單一次寫回後端(批次結束時呼叫一次即可,避免逐筆 IPC) */
export const persistProcessed = async () => {
  if (!pending.size) return
  const batch = [...pending]
  pending = new Set()
  try {
    await pregenMarkDone(batch)
  } catch {
    // 寫回失敗:放回 pending 等下次;不影響本批已完成的預產
    for (const sn of batch) pending.add(sn)
  }
}

/** 清除「今日已預產」記憶(後端 DB + 前端記憶體),讓所有訂單下次查詢都重新抓取。
 *  供面單預產頁「清除已預產記錄」鈕,以及清空後端快取時連帶呼叫 —— 兩層一起歸零才能真正重跑。
 *  **先清後端(單一來源),成功才清前端記憶體;後端失敗則拋出讓呼叫端提示** —— 不可只清前端,
 *  否則 UI 顯示已清、下次 loadProcessed 又把 DB 舊清單載回(清了卻清不掉且無提示)。 */
export const clearProcessed = async () => {
  await pregenClearDone()
  sns = new Set()
  pending = new Set()
}

/** 目前記憶體內已預產(會被略過)的訂單數,供 UI 顯示「清除」鈕是否有意義 */
export const processedCount = () => sns.size
