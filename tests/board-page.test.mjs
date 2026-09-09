// 分揀看板網頁版(/board)的回歸測試。
//
// 這頁和手機遙控頁一樣是後端 include_str! 進 binary 的單一 HTML,沒有前端建置流程可掛測試框架,
// 故用 jsdom 載入該檔、stub 掉 fetch 與 EventSource 來驗渲染。
//
// 跑法:
//   node tests/board-page.test.mjs
//
// 驗到的重點:三種狀態的字色 class、單號後四碼要單獨放大、暫停的通道亮黃燈、
// 分到的通道亮綠燈且持續到下一件進來、物流名與異常訊息的顯示位置。
import { JSDOM } from 'jsdom'
import fs from 'fs'

const html = fs.readFileSync(new URL('../src-tauri/src/server/board_page.html', import.meta.url), 'utf8')

const CHANNELS = [
  { position: 'L1', channel_code: 'L1', enabled: true },
  { position: 'L2', channel_code: 'L2', enabled: false }, // 暫停中 → 黃燈
  { position: 'L3', channel_code: 'L3', enabled: true },
  { position: 'L4', channel_code: 'L4', enabled: true },
  { position: 'L5', channel_code: '', enabled: true },
  { position: 'R1', channel_code: 'R1', enabled: true },
  { position: 'R2', channel_code: 'R2', enabled: true },
  { position: 'R3', channel_code: 'R3', enabled: true },
  { position: 'R4', channel_code: 'R4', enabled: true },
  { position: 'R5', channel_code: '', enabled: true },
]

// stub 必須在頁面腳本執行前就位 —— 它在解析當下就會呼叫 fetch 與 EventSource
const dom = new JSDOM(html, {
  runScripts: 'dangerously',
  url: 'http://127.0.0.1:18080/board',
  pretendToBeVisual: true,
  beforeParse(window) {
    window.fetch = async url => {
      if (String(url).startsWith('/api/channels')) return { ok: true, json: async () => CHANNELS }
      throw new Error('unexpected url ' + url)
    }
    // jsdom 沒有 EventSource(真瀏覽器有);看板只透過 onmessage 進資料,測試直接呼叫 applyEvent
    window.EventSource = function () { this.close = () => {} }
  },
})
const w = dom.window

await new Promise(r => setTimeout(r, 200))

let pass = 0, fail = 0
const ok = (cond, name, extra = '') => {
  if (cond) { pass++; console.log('  ✅ ' + name) }
  else { fail++; console.log('  ❌ ' + name + (extra ? ' — ' + extra : '')) }
}
const $ = id => w.document.getElementById(id)
const slots = side => [...w.document.querySelectorAll('#' + side + ' .slot')]
const headText = () => w.document.querySelector('#no .head').textContent
const tailText = () => w.document.querySelector('#no .tail').textContent

console.log('\n【1】通道燈:左右各五格,暫停的亮黃燈')
ok(slots('left').length === 5 && slots('right').length === 5, '左右各五格(只有燈,不再顯示位置名)', `左${slots('left').length} 右${slots('right').length}`)
ok(slots('left')[1].classList.contains('paused'), '左2 暫停 → 黃燈')
ok(!slots('left')[0].classList.contains('paused'), '左1 啟用中 → 不是黃燈')
ok(slots('left').map(s => s.textContent.trim()).join(',') === 'L1,L2,L3,L4,L5', '燈圓內顯示位置代碼', slots('left').map(s => s.textContent.trim()).join(','))

console.log('\n【2】正常件:白字 + 綠燈 + 後四碼單獨放大')
w.applyEvent({ position: 'R3', no: '74Z01083017', provider: '7-ELEVEN', status: 'ok', message: null, seq: 1 })
ok($('center').classList.contains('ok'), '正常件用 ok 樣式(白字)')
ok(headText() === '74Z0108' && tailText() === '3017', '單號拆成前段 + 後四碼', `head=${headText()} tail=${tailText()}`)
ok(slots('right')[2].classList.contains('active'), '分到的 R3 亮綠燈')
ok($('provider').textContent === '7-ELEVEN', '單號下方顯示物流名')

console.log('\n【3】查件異常:紅字 + 顯示異常訊息')
w.applyEvent({ position: null, no: '74Z01082485', provider: '7-ELEVEN', status: 'error', message: '訂單當前狀態異常', seq: 2 })
ok($('center').classList.contains('error'), '異常件用 error 樣式(紅字)')
ok($('note').textContent === '訂單當前狀態異常', '異常訊息顯示在物流名下方')
ok(slots('right').every(s => !s.classList.contains('active')), '沒有通道時不亮綠燈')

console.log('\n【4】未指派通道:黃字')
w.applyEvent({ position: null, no: 'TW00000000002', provider: '黑貓宅急便', status: 'unassigned', message: null, seq: 3 })
ok($('center').classList.contains('unassigned'), '未指派通道用 unassigned 樣式(黃字)')
ok($('note').textContent.length > 0, '註記說明未指派通道')

console.log('\n【5】綠燈一直亮到下一件進來(不自動熄滅)')
w.applyEvent({ position: 'L1', no: '74Z01010866', provider: '7-ELEVEN', status: 'ok', message: null, seq: 4 })
ok(slots('left')[0].classList.contains('active'), '剛分到時 L1 是綠燈')
await new Promise(r => setTimeout(r, 120))
ok(slots('left')[0].classList.contains('active'), '過一段時間後 L1 仍亮著(停機時也看得到上一件去向)')
// 下一件進了別的格口,前一格才熄
w.applyEvent({ position: 'R4', no: '74Z01010446', provider: '7-ELEVEN', status: 'ok', message: null, seq: 5 })
ok(!slots('left')[0].classList.contains('active'), '下一件進來後 L1 熄滅')
ok(slots('right')[3].classList.contains('active'), '換成 R4 亮綠燈')

console.log(`\n=== ${pass} 通過 / ${fail} 失敗 ===`)
process.exit(fail ? 1 : 0)
