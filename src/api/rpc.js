/**
 * 網頁版的資料通道 — 對應桌面的 Tauri `invoke`。
 *
 * 參數格式與 invoke 完全相同(第二個參數原樣送出),後端 `/rpc/{cmd}` 會做
 * camelCase → snake_case 對應,因此 `api/tauri.js` 的呼叫端兩種環境共用一份。
 */

/** 收到 401 時通知外層(路由守衛)導向登入頁 */
let onUnauthorized = null

export const setUnauthorizedHandler = fn => {
  onUnauthorized = fn
}

export async function rpcInvoke(command, args) {
  let res
  try {
    res = await fetch(`/rpc/${encodeURIComponent(command)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(args ?? {}),
      // session cookie 要跟著送,否則每次呼叫都會被當成未登入
      credentials: 'same-origin',
    })
  } catch (e) {
    // 連線層失敗(server 沒開、網路斷)與業務錯誤分開報,現場才知道該查哪邊
    throw new Error(`無法連線到本機服務:${e.message}`)
  }

  if (res.status === 401) {
    onUnauthorized?.()
    throw new Error('尚未登入或登入已逾期')
  }

  if (!res.ok) {
    let message = `伺服器回應 ${res.status}`
    try {
      const body = await res.json()
      if (body?.error) message = body.error
    } catch {
      // 回應不是 JSON(例如反向代理吐的錯誤頁),沿用狀態碼訊息
    }
    throw new Error(message)
  }

  return await res.json()
}
