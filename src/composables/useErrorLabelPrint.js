// 錯誤面單列印 — 掃描列印 / 自動印單共用
//
// 查詢/印單失敗時,middleware 會產生一張「錯誤提示面單」並回傳 error_label_path(本機 /images URL)。
// 這裡用「該物流商在『印表機設定』(printerMap, localStorage)的同一台印表機」把它印出來,
// 與正常面單同一條出口 —— 避免後端 find_any_printer(dispatch_provider DB / 系統預設)與
// 前端 printerMap 印表機來源不一致,造成「正常面單印得出、錯誤面單卻印不出或印錯台」。
//
// 找不到該物流商的印表機時,退回任一已設印表機(站點多半共用同一台標籤機),
// 讓現場至少能印出錯誤面單把異常包裹撿出。完全沒設印表機時回 no_printer,由呼叫端決定是否提示。
import { printImage } from '@/api/tauri'

/**
 * @param {string} errorLabelPath middleware 回的錯誤面單 URL(可能為空)
 * @param {string} provider 物流商代碼(用來對應 printerMap)
 * @param {Record<string,string>} printerMap 物流商 → 印表機名稱
 * @returns {Promise<{printed:boolean, reason?:string}>}
 * @throws printImage 失敗時拋出(由呼叫端 try/catch)
 */
export async function printErrorLabel(errorLabelPath, provider, printerMap) {
  if (!errorLabelPath) return { printed: false, reason: 'no_path' }
  const printer = (provider && printerMap[provider]) || Object.values(printerMap || {})[0]
  if (!printer) return { printed: false, reason: 'no_printer' }
  const path = errorLabelPath.startsWith('file://') ? errorLabelPath.slice(7) : errorLabelPath
  await printImage({ printerName: printer, imagePath: path })
  return { printed: true }
}
