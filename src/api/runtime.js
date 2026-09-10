/**
 * 執行環境判斷 — 同一份前端會跑在三種地方,資料來源各不相同。
 *
 * | 環境          | 判斷依據                | 資料來源           |
 * |---------------|-------------------------|--------------------|
 * | 桌面 App      | `__TAURI_INTERNALS__`   | Tauri IPC          |
 * | 網頁版        | `__CIX_WEB__`(後端注入) | HTTP RPC           |
 * | 純瀏覽器預覽  | 兩者皆無                | 內建 mock          |
 *
 * 三者刻意用「明確旗標」而非「試打後端失敗就退 mock」來分辨:後者會把
 * 真正的連線錯誤吞成假資料,讓壞掉的頁面看起來像正常的。
 */

const w = typeof window !== 'undefined' ? window : undefined

/** 桌面 App(Tauri webview)內 */
export const isTauriRuntime = !!w?.__TAURI_INTERNALS__

/** 由本機 server 提供的網頁版(index.html 由後端注入旗標) */
export const isWebRuntime = !isTauriRuntime && !!w?.__CIX_WEB__

/** 有真正的後端可呼叫;兩者皆無時走 mock,供 `npm run preview` 做版面迭代 */
export const hasBackend = isTauriRuntime || isWebRuntime
