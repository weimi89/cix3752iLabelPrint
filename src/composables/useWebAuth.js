/**
 * 網頁版的登入狀態。
 *
 * 只在瀏覽器開啟的網頁版有作用 —— 桌面 App 走 Tauri IPC,不經過 HTTP 這道門,
 * 永遠視為已通過。內網來源(現場電腦、手機)後端會直接放行,使用者不會看到登入頁;
 * 只有從外網連進來的人需要輸入共用密碼。
 */

import { ref } from 'vue'
import { isWebRuntime } from '@/api/runtime'

/** 這條連線能不能存取資料 */
const authenticated = ref(!isWebRuntime)
/** 來源是否被判定為內網(內網免登入) */
const isLan = ref(!isWebRuntime)
/** 後端是否已設定共用密碼 —— 沒設定的話外網再怎麼試都進不來 */
const passwordSet = ref(false)
/** 後端連得上嗎。連不上與「沒登入」是兩回事,混在一起會把人誤導到登入頁 */
const reachable = ref(true)
const checked = ref(!isWebRuntime)

/**
 * 向後端確認目前身分。
 *
 * 回傳「能不能存取」。**連不上後端時不會把人標成未登入** —— 後端重啟(改設定就會)
 * 期間內網使用者本來免登入,若因為一次請求失敗就把他們丟去登入頁,畫面會變成
 * 「要密碼、但其實沒有密碼」的死路。連線問題另外用 `reachable` 表示。
 */
async function refresh() {
  if (!isWebRuntime) return true

  try {
    const res = await fetch('/auth/status', { credentials: 'same-origin' })

    if (res.status === 403) {
      // 後端明確拒絕(對外開關關閉),這是答案不是故障
      reachable.value = true
      authenticated.value = false
      checked.value = true
      return false
    }
    if (!res.ok) throw new Error(`狀態 ${res.status}`)

    const s = await res.json()
    reachable.value = true
    authenticated.value = !!s.authenticated
    isLan.value = !!s.lan
    passwordSet.value = !!s.password_set
    checked.value = true
    return authenticated.value
  } catch {
    reachable.value = false
    return authenticated.value
  }
}

async function login(password) {
  const res = await fetch('/auth/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password }),
    credentials: 'same-origin',
  })

  if (!res.ok) {
    let message = `登入失敗(${res.status})`
    try {
      const b = await res.json()
      if (b?.error) message = b.error
    } catch { /* 回應不是 JSON,沿用狀態碼訊息 */ }
    throw new Error(message)
  }

  authenticated.value = true
  reachable.value = true
  return true
}

async function logout() {
  if (!isWebRuntime) return
  try {
    await fetch('/auth/logout', { method: 'POST', credentials: 'same-origin' })
  } finally {
    authenticated.value = false
  }
}

/** 資料請求收到 401 時呼叫:標記為未登入,讓路由守衛把人帶去登入頁 */
function markUnauthenticated() {
  authenticated.value = false
}

export const useWebAuth = () => ({
  authenticated,
  isLan,
  passwordSet,
  reachable,
  checked,
  refresh,
  login,
  logout,
  markUnauthenticated,
})
