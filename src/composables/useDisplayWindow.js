// 多螢幕看板:在指定螢幕開啟「子視窗」載入既有路由(如 /print-stats、/bag-check)。
// 子視窗 label 一律以 `display-` 開頭,App.vue 依此切換 kiosk(無側欄/導覽列)模式。
import { WebviewWindow, getAllWebviewWindows } from '@tauri-apps/api/webviewWindow'
import { availableMonitors, currentMonitor } from '@tauri-apps/api/window'
import { PhysicalPosition } from '@tauri-apps/api/dpi'

const sleep = ms => new Promise(r => setTimeout(r, ms))

// 看板的開啟參數要記下來,才有辦法在「兩個看板對調螢幕」時把對方原樣重開。
// 放 localStorage 而不是記憶體:主視窗重新整理後仍要認得已經開著的那些看板。
const REGISTRY_KEY = 'cix3752iLabelPrint.displayWindows'

function readRegistry() {
  try {
    return JSON.parse(localStorage.getItem(REGISTRY_KEY) || '{}')
  } catch {
    return {}
  }
}

function rememberBoard(label, info) {
  try {
    localStorage.setItem(REGISTRY_KEY, JSON.stringify({ ...readRegistry(), [label]: info }))
  } catch { /* 隱私模式等寫不進去:只影響對調功能,不影響開關看板 */ }
}

/** 視窗目前是不是落在這台螢幕上(用實際座標判斷,使用者手動拖動過也算得準) */
async function isOnMonitor(win, mon) {
  try {
    const pos = await win.outerPosition()
    return (
      pos.x >= mon.position.x && pos.x < mon.position.x + mon.size.width &&
      pos.y >= mon.position.y && pos.y < mon.position.y + mon.size.height
    )
  } catch {
    return false
  }
}

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
   * 這個 label 的看板目前開在哪:回傳 { open, monitorIndex }。
   * monitorIndex 用視窗實際座標比對各螢幕範圍推得(視窗可能被人手動拖到別台),
   * 不是記在前端的狀態 —— 記狀態會在使用者手動關窗或拖動後失準。
   */
  async function status(label, monitors) {
    const win = await WebviewWindow.getByLabel(label)
    if (!win) return { open: false, monitorIndex: null }
    try {
      const pos = await win.outerPosition()
      const hit = (monitors || []).find(m => {
        const { x, y } = m.raw.position
        return pos.x >= x && pos.x < x + m.raw.size.width && pos.y >= y && pos.y < y + m.raw.size.height
      })
      return { open: true, monitorIndex: hit ? hit.index : null }
    } catch {
      return { open: true, monitorIndex: null }
    }
  }

  /**
   * 目前所有已開啟的看板(含別的頁面開的)與它們在哪一台螢幕。
   * 用來在選單裡標出「這台螢幕已被誰佔用」,以及兩個看板對調螢幕。
   */
  async function allBoards(monitors) {
    const wins = await getAllWebviewWindows()
    const reg = readRegistry()
    const out = []
    for (const w of wins) {
      if (!w.label.startsWith('display-')) continue
      let monitorIndex = null
      for (const m of monitors || []) {
        if (await isOnMonitor(w, m.raw)) { monitorIndex = m.index; break }
      }
      out.push({
        label: w.label,
        title: reg[w.label]?.title || w.label,
        route: reg[w.label]?.route || null,
        fullscreen: reg[w.label]?.fullscreen ?? true,
        borderless: reg[w.label]?.borderless ?? true,
        monitorIndex,
      })
    }
    return out
  }

  /** 關閉這個 label 的看板;沒開著也不算錯 */
  async function close(label) {
    const win = await WebviewWindow.getByLabel(label)
    if (!win) return false
    await win.close()
    return true
  }

  /**
   * 開啟(或把既有的移到指定螢幕)看板子視窗。
   * @param {object} o
   * @param {string} o.label       視窗唯一 label(display-xxx)
   * @param {string} o.route       hash 路由,如 '/print-stats'
   * @param {string} o.title       視窗標題
   * @param {object|null} o.monitor getMonitors() 的其中一筆;null=置中視窗模式
   * @param {boolean} o.fullscreen  是否全螢幕(僅指定 monitor 時有效)
   * @param {boolean} o.borderless  是否無邊框
   */
  async function open({ label, route, title, monitor = null, fullscreen = true, borderless = true }) {
    const mon = monitor?.raw

    // 已開啟:要換螢幕就自動關掉重開。
    // 不用 setPosition 搬 —— macOS 的原生全螢幕視窗吃不到 setPosition(實測過:
    // 退出全螢幕、等動畫、再設位置都沒用,視窗仍留在原本那台)。關掉重開對使用者
    // 是同樣的一個動作,而且結果可靠;看板本來就是即時資料,重開不會遺失東西。
    const existing = await WebviewWindow.getByLabel(label)
    if (existing) {
      const sameScreen = mon ? await isOnMonitor(existing, mon) : false
      if (sameScreen || !mon) {
        try { await existing.unminimize() } catch { /* 部分平台無此能力,忽略 */ }
        await existing.setFocus()
        return { window: existing, reused: true, moved: false }
      }
      await existing.close()
      await sleep(400)
    }

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

    rememberBoard(label, { route, title, fullscreen, borderless })
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

  return { getMonitors, open, status, close, allBoards }
}
