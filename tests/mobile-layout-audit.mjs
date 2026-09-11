// 手機版版面稽核:把網頁版每一頁在指定寬度 / 語系下截圖,並列出「元素超出視窗」與
// 「文字被 overflow:hidden 切掉」的地方。不是自動測試,是改完版面後跑一次看結果的工具。
//
// 用法(需先 `yarn dev`,且本機 18080 後端在跑):
//   npx playwright@1.49.0 install chrome 不需要,直接用系統 Chrome
//   node tests/mobile-layout-audit.mjs <輸出目錄> [寬度=390] [語系=zh-Hant]
// 產物:<輸出目錄>/<頁面>-<寬度>[-vi]-p<n>.png(每 1200px 切一張),
// 終端列出每頁的 docW(>寬度就是整頁能橫向捲動)、OVER(超出視窗)、CLIP(被切掉)。
// 下拉選單與輸入框的內容本來就會截成刪節號,那類 CLIP 可忽略。
import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'

const require = createRequire(import.meta.url)
let chromium
try {
  ({ chromium } = require('playwright'))
} catch {
  console.error('找不到 playwright:先 `npx -y playwright@1.49.0 --version` 讓 npx 快取一份,或 `yarn add -D playwright`')
  process.exit(1)
}

const [,, OUT, W_ARG, LOC_ARG] = process.argv
if (!OUT || !existsSync(OUT)) { console.error('請指定已存在的輸出目錄'); process.exit(1) }
const W = Number(W_ARG || 390)
const LOC = LOC_ARG || 'zh-Hant'
const TAG = LOC === 'zh-Hant' ? '' : `-${LOC.split('-')[0]}`
const BASE = process.env.AUDIT_BASE || 'http://localhost:11420'

// 網頁版走得到的頁面(desktopOnly 的三頁在網頁版會被導回首頁,不列)
const routes = ['/', '/bag-check', '/pre-generate', '/sort-channels', '/sort-board', '/clearance-add', '/clearance-dispatch', '/field-operation-monitor', '/dispatch-providers', '/printer-settings', '/server-settings', '/cache-settings', '/cloud-settings', '/event-log', '/queue-log', '/parcel-query-log', '/parcel-alert-log', '/print-stats', '/login']

const browser = await chromium.launch({ channel: 'chrome' })
const ctx = await browser.newContext({ viewport: { width: W, height: 844 }, isMobile: W < 600, hasTouch: W < 600 })
await ctx.addInitScript(l => { try { localStorage.setItem('app-locale', l) } catch {} }, LOC)
const page = await ctx.newPage()

for (const r of routes) {
  await page.goto(`${BASE}/#${r}`)
  await page.waitForTimeout(2500)
  const name = r === '/' ? 'dashboard' : r.slice(1)
  const H = await page.evaluate(() => document.documentElement.scrollHeight)
  for (let i = 0, y = 0; y < H; i++, y += 1200) {
    await page.screenshot({ path: `${OUT}/${name}-${W}${TAG}-p${i}.png`, fullPage: true, clip: { x: 0, y, width: W, height: Math.min(1200, H - y) } })
  }
  const res = await page.evaluate(() => {
    const iw = innerWidth
    const out = { docW: document.documentElement.scrollWidth, over: [], clipped: [] }
    const desc = e => `${e.tagName.toLowerCase()}${e.id ? '#' + e.id : ''}.${[...e.classList].slice(0, 4).join('.')}`
    for (const e of document.querySelectorAll('body *')) {
      const cs = getComputedStyle(e)
      if (cs.display === 'none' || cs.visibility === 'hidden') continue
      const rc = e.getBoundingClientRect()
      if (rc.width === 0 || rc.height === 0) continue
      // 收合的側欄整個在畫面外,不算超出
      if (e.closest('.v-overlay-container, .v-navigation-drawer, .layout-vertical-nav')) continue
      if (rc.right > iw + 1 || rc.left < -1) {
        let a = e.parentElement, inScroll = false
        while (a && a !== document.body) {
          const o = getComputedStyle(a).overflowX
          if (o === 'auto' || o === 'scroll') { inScroll = true; break }
          a = a.parentElement
        }
        if (!inScroll) out.over.push({ el: desc(e), l: Math.round(rc.left), r: Math.round(rc.right), t: Math.round(rc.top), txt: (e.innerText || '').trim().slice(0, 30) })
      }
      const ox = cs.overflowX
      if (e.scrollWidth > e.clientWidth + 2 && e.clientWidth > 0 && (ox === 'hidden' || ox === 'clip')
        && !e.matches('.v-slide-group__container, .v-slide-group__content, .v-progress-linear, .v-progress-linear *, .v-tabs, .v-tabs *')) {
        out.clipped.push({ el: desc(e), sw: e.scrollWidth, cw: e.clientWidth, t: Math.round(rc.top), txt: (e.innerText || '').trim().slice(0, 30) })
      }
    }
    out.over = out.over.slice(0, 12)
    out.clipped = out.clipped.slice(0, 12)
    return out
  })
  console.log(`== ${name} docW=${res.docW}${res.docW > W ? '  ← 整頁可橫向捲動' : ''}`)
  for (const o of res.over) console.log('  OVER', JSON.stringify(o))
  for (const c of res.clipped) console.log('  CLIP', JSON.stringify(c))
}
await browser.close()
