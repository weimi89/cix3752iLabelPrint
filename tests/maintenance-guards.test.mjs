/**
 * 「日後容易漏掉」的地方，用測試擋下來。
 *
 * 這裡守的都不是邏輯正確性，而是**跨檔案的對應關係** —— 這種東西寫在註解裡遲早會被漏掉，
 * 而漏掉的症狀又都很難聯想到原因（桌面正常但網頁 404、手機上某一格沒有名稱），
 * 所以讓它在測試階段就失敗。
 *
 * Rust 那側對應的守門在 `src-tauri/src/server/rpc.rs` 的 `registry_sync`
 * （`generate_handler!` 與 RPC 分派表必須一對一）。
 */

import { test, describe } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(fileURLToPath(new URL('.', import.meta.url)), '..')
const read = p => readFileSync(join(root, p), 'utf8')

describe('前端與後端的 command 對應', () => {
  /** `generate_handler![...]` 裡註冊的 command 名 */
  const registered = (() => {
    const src = read('src-tauri/src/lib.rs')
    const start = src.indexOf('generate_handler![')
    const end = src.indexOf(']', start)
    return new Set(
      src.slice(start, end)
        .split('\n')
        .slice(1)
        .map(l => l.trim().replace(/,$/, ''))
        .filter(l => l && !l.startsWith('//'))
        .map(l => l.split('::').pop()),
    )
  })()

  /** `api/tauri.js` 裡實際被呼叫的 command 名 */
  const called = (() => {
    const src = read('src/api/tauri.js')
    return new Set([...src.matchAll(/\binvoke\(\s*'([a-z0-9_]+)'/g)].map(m => m[1]))
  })()

  test('後端有註冊足夠多的 command（解析沒壞掉）', () => {
    assert.ok(registered.size > 50, `只解析到 ${registered.size} 支，解析邏輯可能失效`)
    assert.ok(called.size > 40, `只解析到 ${called.size} 支前端呼叫`)
  })

  test('前端呼叫的每一支後端都要有', () => {
    // 少了會在使用者點到那一頁時才炸，而且錯誤訊息不會指向這裡
    const missing = [...called].filter(c => !registered.has(c))
    assert.deepEqual(missing, [], `這些 command 前端有呼叫但後端沒註冊：${missing.join(', ')}`)
  })
})

describe('手機版表格的欄名對應', () => {
  /**
   * `.table-cards` 在窄螢幕把表格改成一筆一張卡，欄名取自每個 `<td>` 的 `data-label`。
   * 新增欄位忘了補，手機上那一格就會沒有名稱；補錯位置則會標成別欄的名字。
   */
  const pages = readdirSync(join(root, 'src/pages')).filter(f => f.endsWith('.vue'))

  for (const file of pages) {
    const src = read(`src/pages/${file}`)
    if (!src.includes('table-cards')) continue

    test(`${file} 的每一格都對到正確的欄`, () => {
      const lines = src.split('\n')
      let heads = [], cur = [], inHead = false, inBody = false, idx = 0
      const problems = []

      lines.forEach((ln, i) => {
        if (ln.includes('<thead')) { inHead = true; cur = [] }
        if (inHead) {
          const m = ln.match(/<th\b[^>]*>(.*?)<\/th>/)
          if (m) {
            const inner = m[1].trim()
            const mm = inner.match(/^\{\{\s*(.+?)\s*\}\}$/)
            cur.push(mm ? mm[1] : (inner.replace(/<[^>]+>/g, '').trim() || null))
          } else if (/<th\b/.test(ln)) cur.push(null)
        }
        if (ln.includes('</thead>')) { inHead = false; heads = cur }

        if (ln.includes('<tbody')) { inBody = true; idx = 0 }
        if (ln.includes('</tbody>')) inBody = false

        if (inBody && /<td\b/.test(ln)) {
          if (ln.includes('colspan')) {
            // 跨欄的格子要把索引推進 N 欄，否則同一列後面全部對錯欄
            const m = ln.match(/colspan="?(\d+)/)
            idx += m ? parseInt(m[1], 10) : 1
          } else {
            const m = ln.match(/:?data-label="([^"]*)"/)
            const label = m ? m[1] : null
            const expect = idx < heads.length ? heads[idx] : undefined
            if (!label && expect) {
              problems.push(`第 ${i + 1} 行：第 ${idx + 1} 格缺 data-label（應為 ${expect}）`)
            } else if (label && expect && label !== expect) {
              problems.push(`第 ${i + 1} 行：第 ${idx + 1} 格標成 ${label}，應為 ${expect}`)
            }
            idx += 1
          }
        }
        if (inBody && ln.includes('</tr>')) idx = 0
      })

      assert.deepEqual(problems, [], `\n  ${problems.join('\n  ')}\n`)
    })
  }
})

describe('離線圖示集', () => {
  /**
   * 圖示資料要打包進來，否則 @iconify/vue 會去 api.iconify.design 線上抓 ——
   * 外網一斷，畫面上每個圖示都變空白，連導覽列有哪些按鈕都看不出來。
   * 新增圖示後要重跑 `node scripts/build-icon-subset.mjs`，這裡守著別漏掉。
   */
  test('用到的圖示都在離線集裡', async () => {
    const { collectUsedIcons } = await import('../scripts/build-icon-subset.mjs')
    const offline = JSON.parse(read('src/plugins/icons-offline.json'))

    const have = new Set()
    for (const [prefix, c] of Object.entries(offline)) {
      for (const name of Object.keys(c.icons)) have.add(`${prefix}-${name}`)
    }

    const missing = [...collectUsedIcons()].filter(n => !have.has(n))
    assert.deepEqual(
      missing,
      [],
      `這些圖示沒被打包進來，外網斷線時會變空白：${missing.join(', ')}\n` +
      `   請執行：node scripts/build-icon-subset.mjs`,
    )
  })

  test('離線集沒有空的圖示資料', () => {
    const offline = JSON.parse(read('src/plugins/icons-offline.json'))
    const empty = []
    for (const [prefix, c] of Object.entries(offline)) {
      for (const [name, data] of Object.entries(c.icons)) {
        if (!data?.body) empty.push(`${prefix}:${name}`)
      }
    }
    assert.deepEqual(empty, [], `這些圖示抽出來是空的：${empty.join(', ')}`)
  })
})

describe('雙語文案', () => {
  const zh = JSON.parse(read('src/plugins/i18n/locales/zh-Hant.json'))
  const vi = JSON.parse(read('src/plugins/i18n/locales/vi-VN.json'))

  const flat = (obj, prefix = '') =>
    Object.entries(obj).flatMap(([k, v]) => {
      const key = prefix ? `${prefix}.${k}` : k
      return typeof v === 'object' && v !== null ? flat(v, key) : [key]
    })

  test('兩個語系的鍵完全一致', () => {
    const a = new Set(flat(zh))
    const b = new Set(flat(vi))
    const onlyZh = [...a].filter(k => !b.has(k))
    const onlyVi = [...b].filter(k => !a.has(k))
    assert.deepEqual(onlyZh, [], `越南語缺這些鍵：${onlyZh.slice(0, 10).join(', ')}`)
    assert.deepEqual(onlyVi, [], `繁中缺這些鍵：${onlyVi.slice(0, 10).join(', ')}`)
  })

  /**
   * 刻意留空的鍵：兩種語言的語序不同，不是漏翻。
   * 新增項目時請一併寫清楚理由，否則這個白名單會慢慢變成漏翻的藏身處。
   */
  const INTENTIONAL_EMPTY = new Set([
    // 中文是「第 [N] 頁」，前後都有字；越南語是「Trang [N]」，後面不接東西
    'vi-VN: pagination.pageSuffix',
  ])

  test('沒有非刻意的空字串翻譯', () => {
    // 補鍵時佔位留空會讓畫面出現空白標籤，比顯示原文還難查
    const empties = []
    const walk = (obj, locale, prefix = '') => {
      for (const [k, v] of Object.entries(obj)) {
        const key = prefix ? `${prefix}.${k}` : k
        if (typeof v === 'object' && v !== null) walk(v, locale, key)
        else if (typeof v === 'string' && v.trim() === '') empties.push(`${locale}: ${key}`)
      }
    }
    walk(zh, 'zh-Hant')
    walk(vi, 'vi-VN')
    const unexpected = empties.filter(e => !INTENTIONAL_EMPTY.has(e))
    assert.deepEqual(unexpected, [], `這些翻譯是空的：${unexpected.join(', ')}`)
  })
})
