import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { isTauriRuntime } from './runtime'

/**
 * 桌面 webview 向本機 server 載圖時要帶的權杖(`?dt=`)。
 *
 * 桌面頁面的來源是 Tauri 自己的 tauri.localhost,對瀏覽器來說與 127.0.0.1:{port} 是不同站,
 * `<img>` 一律標成跨站、被後端的跨站防護擋下(相機預覽、縮圖、存證照都會空白)。
 * 這組權杖每次啟動隨機產生、只經 Tauri IPC 拿得到,後端用它認出「這是自己的桌面畫面」。
 * 網頁版與後端同源,不需要也拿不到。
 */
let desktopToken = ''

/** 開機時(掛載前)呼叫一次;沒有它桌面上所有走本機 server 的圖都載不出來。網頁版直接略過。 */
export const loadDesktopMediaToken = async () => {
  if (!isTauriRuntime) return
  desktopToken = await tauriInvoke('desktop_media_token')
}

const withDesktopToken = url => {
  if (!isTauriRuntime || !desktopToken) return url
  return `${url}${url.includes('?') ? '&' : '?'}dt=${desktopToken}`
}

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
  return isTauriRuntime ? withDesktopToken(`http://127.0.0.1:${port}${p}`) : p
}

/**
 * 把後端回傳的面單網址轉成「這台裝置看得到」的形式。
 *
 * `cloud_fetch_label` 之類回傳的 `print_file_path` / `error_label_path` 是寫死的
 * `http://127.0.0.1:{port}/images/...`。桌面 webview 是同一台機器,補上桌面權杖即可;網頁版
 * 從別台裝置開時,瀏覽器會把 127.0.0.1 解成**它自己**,縮圖一律載不出來,要改成相對路徑。
 *
 * **只用在顯示(`<img :src>`)。** 送去列印時要保持原樣 —— 那是伺服器自己去讀,
 * 對它而言 127.0.0.1 正是自己,而且後端只放行指向本機的網址(帶了權杖的網址也不會被放行)。
 */
export const toViewableUrl = url => {
  if (!url) return ''
  if (isTauriRuntime) return withDesktopToken(String(url))
  return String(url).replace(/^https?:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):\d+/, '')
}
