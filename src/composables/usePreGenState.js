import { ref } from 'vue'

// 面單預產的輸入模式(order 訂單編號 / package 袋號 / date 日期)。
// 用「模組級單例」ref:換頁離開再回來會保留上次選擇(頁面 component 會卸載重建,
// 但模組只載入一次),且不寫 localStorage —— 重開 App 即回預設「訂單編號」。
export const preGenInputMode = ref('order')
