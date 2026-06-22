import { ref } from 'vue'

/**
 * 掃描重複防呆 — 防止掃描槍「一次刷出兩筆相同條碼」造成同一筆重複列印。
 * 同一個值在 windowMs(預設 5 秒)內再次出現即視為重複,isDuplicate() 回傳 true 讓呼叫端略過。
 *
 * 設計:
 * - 時間錨定在「首次有效掃描」,被擋下的重複不刷新時間戳 →
 *   連續多刷皆相對首刷判定;真正隔一段時間的重印(>windowMs)仍可正常通過。
 * - 每個掃描入口(袋號欄 / 訂單編號欄)各自持有一個實例,
 *   不同欄位的相同值不互相干擾(OFF 模式「先刷袋號載清單、再刷訂單列印」是兩種意圖)。
 * - 只在「實際要進行查詢/列印」時才呼叫 isDuplicate;
 *   被前置驗證(人員/列印類型未填)擋下的掃描不該登記,
 *   否則操作員補資料後重刷同碼會被誤判為重複。
 */
export function useScanDedup(windowMs = 5000) {
  const lastValue = ref('')
  const lastTime = ref(0)

  // 回傳 true = 此次掃描為 windowMs 內的重複,應略過;回傳 false = 首刷(或已過時窗),已登記
  const isDuplicate = value => {
    const now = Date.now()
    if (value && value === lastValue.value && now - lastTime.value < windowMs) {
      return true
    }
    lastValue.value = value
    lastTime.value = now
    return false
  }

  // 清除記錄(切換模式 / 清空清單時呼叫,讓下一刷一定通過)
  const reset = () => {
    lastValue.value = ''
    lastTime.value = 0
  }

  return { isDuplicate, reset }
}
