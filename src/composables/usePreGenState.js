import { ref, watch } from 'vue'

// 面單預產的輸入模式(對應四個分頁):
//   order 訂單編號 / package 袋號 / clearance 清關日期 / transfer 轉寄出貨
// 用「模組級單例」ref:換頁離開再回來會保留上次選擇(頁面 component 會卸載重建,但模組只載入一次)。
// 並寫入 localStorage,讓「重新啟動 App」也停在上次的分頁。
const STORAGE_KEY = 'cix3752iLabelPrint.preGenInputMode'
const VALID_MODES = ['order', 'package', 'clearance', 'transfer']

// 白名單防呆:localStorage 被塞髒值、或分頁代碼改名時,退回預設「訂單編號」,
// 避免停在不存在的分頁導致畫面空白。
const saved = localStorage.getItem(STORAGE_KEY)
const initial = VALID_MODES.includes(saved) ? saved : 'order'

export const preGenInputMode = ref(initial)

watch(preGenInputMode, mode => {
  localStorage.setItem(STORAGE_KEY, mode)
})
