import { ref, watch } from 'vue'

/**
 * localStorage 後端的「單值記憶」:切換頁面(元件卸載)或重啟後沿用上次選擇,
 * 操作員不需重選。值變動即寫入,讀取失敗 / 無記錄時退回 defaultValue。
 *
 * @param {string} storageKey 記憶 key(例:'clearance.add.company')
 * @param {*} defaultValue 無記錄時的預設值
 * @returns {import('vue').Ref} 與 localStorage 同步的 ref
 */
export function useStickyValue(storageKey, defaultValue = '') {
  const KEY = `cix3752i:sticky:${storageKey}`

  let initial = defaultValue
  try {
    const raw = localStorage.getItem(KEY)
    if (raw !== null) initial = JSON.parse(raw)
  } catch {
    /* 解析失敗時退回預設 */
  }

  const value = ref(initial)

  watch(value, v => {
    try {
      // null(VCombobox 清除時)正規化成空字串,避免存進 "null"
      localStorage.setItem(KEY, JSON.stringify(v ?? ''))
    } catch {
      /* localStorage 滿或被停用時忽略 */
    }
  })

  return value
}
