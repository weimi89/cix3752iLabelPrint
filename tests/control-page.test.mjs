// 手機遙控頁(/control)「設定貼標人員 / 指派物流」的回歸測試。
//
// 這頁是後端 include_str! 進 binary 的單一 HTML,沒有前端建置流程可掛測試框架,
// 故用 jsdom 直接載入該檔、stub 掉 fetch 來驗渲染與互動。
//
// 跑法:
//   node tests/control-page.test.mjs
//
// 驗到的重點:草稿不被 3 秒輪詢沖掉(含斷線時)、輸入法組字中切換選項不吃字、
// 存檔後的重抓失敗不得報成儲存失敗、人員名稱的 HTML 跳脫、後端錯誤碼的雙語對照。
import { JSDOM } from 'jsdom'
import fs from 'fs'

const html = fs.readFileSync(new URL('../src-tauri/src/server/control_page.html', import.meta.url), 'utf8')

const CHANNELS = [
  { position: 'L1', channel_code: 'L1', enabled: true, dispatch_codes: ['7', 'F'], dispatch_names: ['7-ELEVEN', '全家'], job_sticker: '王小明', skip_count: 0, last_tracking: '1234567890' },
  { position: 'R1', channel_code: '', enabled: true, dispatch_codes: [], dispatch_names: [], job_sticker: null, skip_count: 0, last_tracking: null },
]
const PROVIDERS = [{ code: '7', name: '7-ELEVEN' }, { code: 'F', name: '全家' }, { code: 'C', name: '黑貓宅急便' }]
const NAMES = ['王小明', '陳美玲', '<img src=x onerror=alert(1)>']

const posted = []
function makeFetch(failCode) {
  return async (url, opt) => {
    if (url.startsWith('/api/channels/') && url.endsWith('/assign')) {
      posted.push({ url, body: JSON.parse(opt.body) })
      if (failCode) return { ok: false, status: 400, json: async () => ({ code: failCode, error: '後端中文原文' }) }
      return { ok: true, status: 200, json: async () => ({}) }
    }
    if (url.startsWith('/api/channels/') && url.includes('/recent')) return { ok: true, json: async () => [] }
    if (url.startsWith('/api/channels')) return { ok: true, json: async () => CHANNELS }
    if (url.startsWith('/api/dispatch-providers')) return { ok: true, json: async () => PROVIDERS }
    if (url.startsWith('/api/sticker-history')) return { ok: true, json: async () => NAMES }
    if (url.startsWith('/api/alerts')) return { ok: true, json: async () => [] }
    throw new Error('unexpected url ' + url)
  }
}

const dom = new JSDOM(html, { runScripts: 'dangerously', url: 'http://127.0.0.1:18080/control', pretendToBeVisual: true })
const w = dom.window
w.fetch = makeFetch(null)
const alerts = []
w.alert = m => alerts.push(m)
await new Promise(r => setTimeout(r, 200))
await w.refresh(); await new Promise(r => setTimeout(r, 80))

let pass = 0, fail = 0
const ok = (cond, name, extra = '') => { if (cond) { pass++; console.log('  ✅ ' + name) } else { fail++; console.log('  ❌ ' + name + (extra ? ' — ' + extra : '')) } }
const $ = id => w.document.getElementById(id)
const txt = id => $(id).textContent
const activeView = () => [...w.document.querySelectorAll('.view')].find(v => v.classList.contains('active')).id

console.log('\n【1】清單頁')
ok(txt('view-list').includes('王小明'), '清單顯示貼標人員')
ok(txt('view-list').includes('7-ELEVEN'), '清單顯示指派物流')

console.log('\n【2】詳情頁:貼標未指派也顯示一列 + 編輯入口')
w.go('detail', 'R1'); await new Promise(r => setTimeout(r, 80))
ok(txt('view-detail').includes('貼標'), 'R1(無人員)仍顯示貼標列')
ok(txt('view-detail').includes('未指派'), '無人員時顯示「未指派」')
ok(!!$('view-detail').querySelector('.btn-edit'), '有「設定貼標與物流」按鈕')

console.log('\n【3】進入設定頁:草稿帶入現值')
w.go('detail', 'L1'); await new Promise(r => setTimeout(r, 80))
w.go('assign', 'L1'); await new Promise(r => setTimeout(r, 150))
ok($('view-assign').classList.contains('active'), '設定頁被顯示')
ok(txt('title').includes('左1'), '標題帶通道名: ' + txt('title'))
ok($('stickerInput').value === '王小明', '姓名帶入現值')
const chips = [...$('view-assign').querySelectorAll('.pick .chip')]
const provChips = chips.filter(c => PROVIDERS.some(p => (p.name) === c.textContent.replace(/^✓ /, '')))
ok(provChips.length === 3, '物流選項 3 個', String(provChips.length))
ok(provChips.filter(c => c.classList.contains('sel')).length === 2, '已指派的 2 個為選取態')

console.log('\n【4】XSS:歷史名單含標記字元不得被解析')
ok(!$('view-assign').querySelector('img'), '惡意名稱未變成 <img> 元素')
ok(txt('view-assign').includes('<img src=x onerror=alert(1)>'), '惡意名稱以純文字顯示')

console.log('\n【5】互動:切換物流 / 點歷史名單')
const selNames = () => [...$('view-assign').querySelectorAll('.pick .chip.sel')].map(c => c.textContent.replace(/^✓ /, ''))
w.toggleProvider('C')
ok(selNames().includes('黑貓宅急便'), '點未選物流→加入: ' + selNames())
w.toggleProvider('7')
ok(!selNames().includes('7-ELEVEN'), '點已選物流→移除: ' + selNames())
w.pickSticker('陳美玲')
ok($('stickerInput').value === '陳美玲', '點歷史名單→填入輸入框')
w.pickSticker('陳美玲')
ok($('stickerInput').value === '', '再點同一人→清空')

console.log('\n【6】輪詢不覆蓋草稿')
$('stickerInput').value = '打到一半的名字'
$('stickerInput').dispatchEvent(new w.Event('input'))
await w.refresh(); await new Promise(r => setTimeout(r, 50))
ok($('stickerInput').value === '打到一半的名字', '3 秒輪詢後輸入框內容仍在')

console.log('\n【7】儲存送出內容')
posted.length = 0
await w.saveAssign(); await new Promise(r => setTimeout(r, 80))
ok(posted.length === 1 && posted[0].url === '/api/channels/L1/assign', 'POST 到正確位址')
ok(posted[0].body.job_sticker === '打到一半的名字', '送出姓名: ' + posted[0].body.job_sticker)
ok(JSON.stringify(posted[0].body.dispatch_codes) === '["F","C"]', '送出物流代碼: ' + JSON.stringify(posted[0].body.dispatch_codes))
await new Promise(r => setTimeout(r, 120))
ok(activeView() === 'view-detail', '存檔後自動返回詳情頁,目前在 ' + activeView())

console.log('\n【8】清空姓名送 null(而非空字串)')
w.go('assign', 'L1'); await new Promise(r => setTimeout(r, 150))
$('stickerInput').value = '   '
posted.length = 0
await w.saveAssign(); await new Promise(r => setTimeout(r, 80))
ok(posted[0].body.job_sticker === null, '只有空白→送 null')

console.log('\n【9】後端錯誤碼翻成使用者語言')
w.fetch = makeFetch('PRINTER_REQUIRED')
w.go('assign', 'L1'); await new Promise(r => setTimeout(r, 150))
alerts.length = 0
await w.saveAssign(); await new Promise(r => setTimeout(r, 80))
ok(alerts.length === 1 && alerts[0].includes('尚未設定本機印表機'), '中文錯誤說明: ' + (alerts[0] || ''))
ok(activeView() === 'view-assign', '失敗時留在設定頁不返回,目前在 ' + activeView())
ok(!$('saveBtn').disabled, '失敗後儲存鈕恢復可按')

console.log('\n【10】越南語')
w.toggleLang(); await new Promise(r => setTimeout(r, 50))
ok(txt('view-assign').includes('Người dán nhãn'), '欄位標題越南語')
alerts.length = 0
await w.saveAssign(); await new Promise(r => setTimeout(r, 80))
ok(alerts[0] && alerts[0].includes('chưa đặt máy in'), '錯誤說明越南語: ' + (alerts[0] || ''))
w.toggleLang()

console.log('\n【11】未知錯誤碼退回後端原文')
w.fetch = makeFetch('SOMETHING_NEW')
alerts.length = 0
await w.saveAssign(); await new Promise(r => setTimeout(r, 80))
ok(alerts[0] && alerts[0].includes('後端中文原文'), '未知碼顯示後端訊息')

console.log('\n【12】覆檢修正:輪詢失敗時設定頁不得重繪')
w.fetch = makeFetch(null)
w.go('assign', 'L1'); await new Promise(r => setTimeout(r, 150))
$('stickerInput').value = '斷線前打到一半'
$('stickerInput').dispatchEvent(new w.Event('input'))
const beforeEl = $('stickerInput')
w.fetch = async () => { throw new Error('offline') }
await w.refresh(); await new Promise(r => setTimeout(r, 50))
ok($('stickerInput') === beforeEl, '輸入框元件沒被砍掉重建')
ok($('stickerInput').value === '斷線前打到一半', '斷線輪詢後輸入內容仍在')

console.log('\n【13】覆檢修正:輸入法組字中點 chip 不得吃掉名字')
w.fetch = makeFetch(null)
w.go('detail', 'L1'); await new Promise(r => setTimeout(r, 80))
w.go('assign', 'L1'); await new Promise(r => setTimeout(r, 150))
$('stickerInput').value = '阮文雄'          // 只改 value,不觸發 input(等同組字尚未上字)
w.toggleProvider('C')
ok($('stickerInput').value === '阮文雄', '點物流 chip 後名字還在: ' + $('stickerInput').value)
$('stickerInput').value = '陳大文'
w.pickSticker('王小明')
ok($('stickerInput').value === '王小明', '點歷史名單覆蓋未上字的內容(預期行為)')

console.log('\n【14】覆檢修正:存檔成功但重抓失敗不得報成儲存失敗')
w.go('detail', 'L1'); await new Promise(r => setTimeout(r, 80))
w.go('assign', 'L1'); await new Promise(r => setTimeout(r, 150))
let postDone = false
w.fetch = async (url, opt) => {
  if (url.endsWith('/assign')) { postDone = true; return { ok: true, status: 200, json: async () => ({}) } }
  throw new Error('存檔後斷線')
}
alerts.length = 0
await w.saveAssign(); await new Promise(r => setTimeout(r, 150))
ok(postDone, 'POST 有送出')
ok(alerts.length === 0, '沒有跳「儲存失敗」: ' + (alerts[0] || '無'))
ok(activeView() === 'view-detail', '仍正常返回詳情頁,目前在 ' + activeView())

console.log('\n=== ' + pass + ' 通過 / ' + fail + ' 失敗 ===')
process.exit(fail ? 1 : 0)
