import { isTauriRuntime } from './runtime'

/**
 * 面單圖 / 讀碼站存證照 / 相機預覽串流的網址。
 *
 * 桌面 webview 的頁面來源不是本機 server,相對路徑接不到,必須指名 127.0.0.1;
 * 網頁版本身就由那台 server 提供,**一定要走相對路徑** —— 寫死 127.0.0.1 的話,
 * 主管在自己電腦上開網頁時會去連自己那台的 18080,圖全部載不出來。
 *
 * @param {string} path  以 / 開頭的路徑,例 `/images/xxx.jpg`
 * @param {number} port  桌面模式用的本機 server 埠號
 */
export const mediaUrl = (path, port) => {
  if (!path) return ''
  const p = path.startsWith('/') ? path : `/${path}`
  return isTauriRuntime ? `http://127.0.0.1:${port}${p}` : p
}

/**
 * 把後端回傳的面單網址轉成「這台裝置看得到」的形式。
 *
 * `cloud_fetch_label` 之類回傳的 `print_file_path` / `error_label_path` 是寫死的
 * `http://127.0.0.1:{port}/images/...`。桌面 webview 用沒問題(同一台機器),但網頁版
 * 從別台裝置開時,瀏覽器會把 127.0.0.1 解成**它自己**,縮圖一律載不出來。
 *
 * **只用在顯示(`<img :src>`)。** 送去列印時要保持原樣 —— 那是伺服器自己去讀,
 * 對它而言 127.0.0.1 正是自己,而且後端只放行指向本機的網址。
 */
export const toViewableUrl = url => {
  if (!url || isTauriRuntime) return url || ''
  return String(url).replace(/^https?:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):\d+/, '')
}
