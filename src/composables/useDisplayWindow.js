// 多螢幕看板:在指定螢幕開啟「子視窗」載入既有路由(如 /print-stats、/bag-check)。
// 子視窗 label 一律以 `display-` 開頭,App.vue 依此切換 kiosk(無側欄/導覽列)模式。
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors, currentMonitor } from '@tauri-apps/api/window'
import { PhysicalPosition } from '@tauri-apps/api/dpi'

export function useDisplayWindow() {
  /** 取得所有螢幕(含目前所在螢幕標記) */
  async function getMonitors() {
    const list = await availableMonitors()
    const cur = await currentMonitor().catch(() => null)
    return list.map((m, i) => ({
      raw: m,
      index: i,
      name: m.name || `#${i + 1}`,
      width: m.size.width,
      height: m.size.height,
      scale: m.scaleFactor || 1,
      isCurrent: !!cur && m.position.x === cur.position.x && m.position.y === cur.position.y,
    }))
  }

  /**
   * 開啟(或聚焦既有的)看板子視窗。
   * @param {object} o
   * @param {string} o.label       視窗唯一 label(display-xxx)
   * @param {string} o.route       hash 路由,如 '/print-stats'
   * @param {string} o.title       視窗標題
   * @param {object|null} o.monitor getMonitors() 的其中一筆;null=置中視窗模式
   * @param {boolean} o.fullscreen  是否全螢幕(僅指定 monitor 時有效)
   * @param {boolean} o.borderless  是否無邊框
   */
  async function open({ label, route, title, monitor = null, fullscreen = true, borderless = true }) {
    // 已開啟則直接聚焦,不重複開
    const existing = await WebviewWindow.getByLabel(label)
    if (existing) {
      try { await existing.unminimize() } catch { /* 部分平台無此能力,忽略 */ }
      await existing.setFocus()
      return { window: existing, reused: true }
    }

    const mon = monitor?.raw
    const opts = {
      url: `index.html#${route}`,
      title: title || '看板',
      decorations: !borderless,
      focus: true,
    }
    if (mon) {
      // 建立時先用「邏輯像素」把視窗丟到目標螢幕原點與尺寸,建立後再用實體像素精準校正
      const s = mon.scaleFactor || 1
      opts.x = Math.round(mon.position.x / s)
      opts.y = Math.round(mon.position.y / s)
      opts.width = Math.round(mon.size.width / s)
      opts.height = Math.round(mon.size.height / s)
    } else {
      opts.width = 1280
      opts.height = 800
      opts.center = true
    }

    const win = new WebviewWindow(label, opts)
    await new Promise((resolve, reject) => {
      win.once('tauri://created', () => resolve())
      win.once('tauri://error', e => reject(e?.payload || e || new Error('視窗建立失敗')))
    })

    if (mon) {
      try {
        // 實體像素定位確保落在目標螢幕(HiDPI 下邏輯像素可能有誤差)
        await win.setPosition(new PhysicalPosition(mon.position.x, mon.position.y))
        if (fullscreen) await win.setFullscreen(true)
      } catch (e) {
        console.warn('看板定位/全螢幕失敗', e)
      }
    }
    await win.setFocus()
    return { window: win, reused: false }
  }

  return { getMonitors, open }
}
