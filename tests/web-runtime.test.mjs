/**
 * 網頁版傳輸層的回歸測試。
 *
 * 這幾支的錯誤在畫面上不會直接顯示 —— 環境判斷錯了會整站顯示假資料、事件分發漏了
 * 會讓頁面停在舊數字、401 沒接上會讓逾期後卡在原地,都是「看起來還在動」的壞法,
 * 所以用測試守住。
 *
 * 模組經 esbuild 打包後載入(見 bundle()):原始碼用 Vite 的省略副檔名寫法,
 * Node 的 ESM 解析器吃不下。
 */

import { test, describe, before } from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(fileURLToPath(new URL('.', import.meta.url)), '..')
const outDir = mkdtempSync(join(tmpdir(), 'cix-web-test-'))

/** 把來源模組打包成單檔,讓 Node 能直接 import */
function bundle(entry, name) {
  const out = join(outDir, name)
  execFileSync(
    'npx',
    [
      'esbuild', join(root, entry),
      '--bundle', '--format=esm', '--platform=neutral',
      '--external:@tauri-apps/*', '--external:vue',
      `--outfile=${out}`,
    ],
    { cwd: root, stdio: 'pipe' },
  )
  return out
}

describe('執行環境判斷', () => {
  let runtimePath
  before(() => { runtimePath = bundle('src/api/runtime.js', 'runtime.mjs') })

  const load = async win => {
    globalThis.window = win
    // 每次帶不同 query 以繞過模組快取:旗標是模組載入當下求值的
    return await import(`${runtimePath}?v=${Math.random()}`)
  }

  test('桌面 App:走 Tauri IPC', async () => {
    const r = await load({ __TAURI_INTERNALS__: {} })
    assert.equal(r.isTauriRuntime, true)
    assert.equal(r.isWebRuntime, false)
    assert.equal(r.hasBackend, true)
  })

  test('網頁版:走 HTTP RPC', async () => {
    const r = await load({ __CIX_WEB__: true })
    assert.equal(r.isTauriRuntime, false)
    assert.equal(r.isWebRuntime, true)
    assert.equal(r.hasBackend, true)
  })

  test('純瀏覽器預覽:兩者皆無,走 mock', async () => {
    const r = await load({})
    assert.equal(r.hasBackend, false, '沒有後端時必須走 mock,不能去打不存在的端點')
  })

  test('桌面旗標優先:兩個旗標同時存在時仍走 IPC', async () => {
    // dev 模式下 Vite 會注入 __CIX_WEB__,而 Tauri 視窗載的正是同一個位址
    const r = await load({ __TAURI_INTERNALS__: {}, __CIX_WEB__: true })
    assert.equal(r.isTauriRuntime, true)
    assert.equal(r.isWebRuntime, false, '桌面誤走 HTTP 會繞過 IPC,權限與效能都不對')
  })
})

describe('RPC 通道', () => {
  let rpcPath
  before(() => { rpcPath = bundle('src/api/rpc.js', 'rpc.mjs') })

  const load = async () => await import(`${rpcPath}?v=${Math.random()}`)

  test('成功時回傳解析後的內容', async () => {
    globalThis.fetch = async () => ({ ok: true, status: 200, json: async () => ({ pong: 1 }) })
    const { rpcInvoke } = await load()
    assert.deepEqual(await rpcInvoke('ping', {}), { pong: 1 })
  })

  test('參數原樣送出,路徑帶 command 名', async () => {
    let seen
    globalThis.fetch = async (url, opt) => {
      seen = { url, body: JSON.parse(opt.body), credentials: opt.credentials }
      return { ok: true, status: 200, json: async () => null }
    }
    const { rpcInvoke } = await load()
    await rpcInvoke('update_config', { newConfig: { a: 1 } })
    assert.equal(seen.url, '/rpc/update_config')
    assert.deepEqual(seen.body, { newConfig: { a: 1 } })
    assert.equal(seen.credentials, 'same-origin', 'session cookie 沒帶就會每次都被當未登入')
  })

  test('業務錯誤帶出後端訊息', async () => {
    globalThis.fetch = async () => ({ ok: false, status: 400, json: async () => ({ error: '查無此訂單' }) })
    const { rpcInvoke } = await load()
    await assert.rejects(() => rpcInvoke('x', {}), /查無此訂單/)
  })

  test('回應不是 JSON 時仍給得出訊息', async () => {
    globalThis.fetch = async () => ({ ok: false, status: 502, json: async () => { throw new Error('not json') } })
    const { rpcInvoke } = await load()
    await assert.rejects(() => rpcInvoke('x', {}), /502/)
  })

  test('401 會通知外層去導向登入頁', async () => {
    globalThis.fetch = async () => ({ ok: false, status: 401, json: async () => ({ error: '尚未登入' }) })
    const { rpcInvoke, setUnauthorizedHandler } = await load()
    let called = false
    setUnauthorizedHandler(() => { called = true })
    await assert.rejects(() => rpcInvoke('x', {}))
    assert.equal(called, true, '沒接上的話逾期後畫面會卡在原地一直跳錯')
  })

  test('連線層失敗與業務錯誤分開報', async () => {
    globalThis.fetch = async () => { throw new Error('Failed to fetch') }
    const { rpcInvoke } = await load()
    await assert.rejects(() => rpcInvoke('x', {}), /無法連線到本機服務/)
  })
})

describe('事件分發', () => {
  let eventsPath
  before(() => { eventsPath = bundle('src/api/events.js', 'events.mjs') })

  /** 最小可用的 EventSource 替身 */
  class FakeES {
    static last = null
    constructor(url) {
      this.url = url
      this.onmessage = null
      this.onerror = null
      this.closed = false
      FakeES.last = this
    }
    close() { this.closed = true }
    push(event, payload) { this.onmessage?.({ data: JSON.stringify({ event, payload }) }) }
  }

  const load = async () => {
    globalThis.window = { __CIX_WEB__: true }
    globalThis.EventSource = FakeES
    return await import(`${eventsPath}?v=${Math.random()}`)
  }

  test('收到的事件包成與 Tauri 相同的形狀', async () => {
    const { listen } = await load()
    let got = null
    await listen('print-stats-updated', e => { got = e })
    FakeES.last.push('print-stats-updated', { since_reset: 12 })
    assert.deepEqual(got, { event: 'print-stats-updated', payload: { since_reset: 12 } })
  })

  test('只送給該事件的訂閱者', async () => {
    const { listen } = await load()
    const hits = []
    await listen('sort-board', () => hits.push('board'))
    await listen('parcel-alert', () => hits.push('alert'))
    FakeES.last.push('sort-board', {})
    assert.deepEqual(hits, ['board'])
  })

  test('同一事件的多個訂閱者都收得到', async () => {
    const { listen } = await load()
    let n = 0
    await listen('bag-check-updated', () => n++)
    await listen('bag-check-updated', () => n++)
    FakeES.last.push('bag-check-updated', {})
    assert.equal(n, 2)
  })

  test('所有頁面共用一條連線', async () => {
    const { listen } = await load()
    await listen('a', () => {})
    const first = FakeES.last
    await listen('b', () => {})
    assert.equal(FakeES.last, first, '每頁各開一條會撞到瀏覽器的同網域連線上限')
  })

  test('unlisten 後不再收到', async () => {
    const { listen } = await load()
    let n = 0
    const un = await listen('x', () => n++)
    FakeES.last.push('x', {})
    un()
    FakeES.last.push('x', {})
    assert.equal(n, 1)
  })

  test('訂閱者全部退訂後關閉連線', async () => {
    const { listen } = await load()
    const un = await listen('x', () => {})
    const es = FakeES.last
    un()
    assert.equal(es.closed, true, '留著沒人聽的長連線會一直佔著後端資源')
  })

  test('某個處理函式拋例外不影響其他訂閱者', async () => {
    const { listen } = await load()
    let reached = false
    await listen('x', () => { throw new Error('壞掉了') })
    await listen('x', () => { reached = true })
    FakeES.last.push('x', {})
    assert.equal(reached, true, '一個頁面的錯誤不該讓整條事件流停擺')
  })

  test('壞掉的事件內容不會讓整條流中斷', async () => {
    const { listen } = await load()
    let n = 0
    await listen('x', () => n++)
    FakeES.last.onmessage({ data: '這不是 JSON' })
    FakeES.last.push('x', {})
    assert.equal(n, 1)
  })

  test('退避重連期間不會被新的訂閱繞過', async () => {
    // 後端重啟時使用者切頁,每頁 onMounted 都會 listen —— 若每次都立刻重開連線,
    // 退避等於沒有,變成密集重試打自己的 server
    const { listen } = await load()
    await listen('a', () => {})
    const first = FakeES.last
    first.onerror()                       // 模擬斷線,進入退避
    assert.equal(first.closed, true)
    await listen('b', () => {})           // 退避視窗內有人掛新監聽
    assert.equal(FakeES.last, first, '不該在退避期間另開一條連線')
  })

  test('純瀏覽器預覽下回傳可安全呼叫的取消函式', async () => {
    globalThis.window = {}
    globalThis.EventSource = FakeES
    const { listen } = await import(`${eventsPath}?v=${Math.random()}`)
    const un = await listen('x', () => {})
    assert.equal(typeof un, 'function')
    un()
  })
})
