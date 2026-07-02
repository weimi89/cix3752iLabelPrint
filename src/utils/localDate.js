// 本地日期字串工具。
//
// 注意:`new Date().toISOString().slice(0,10)` 取的是 **UTC** 日期,在 UTC+7/+8(越南/台灣)
// 當地凌晨時段會算成前一天。入倉建箱、清關作業的日期屬「營運日 / 本地日」,必須用本地時區,
// 否則夜班掃描的 return_date / clearance_date 會錯位、箱號(內嵌 YYMMDD)與雲端對不上。
//
// (印單統計 daily_stats 另有刻意保留 UTC 邊界的設計,不使用此工具。)

/** 今天(本地時區)的 YYYY-MM-DD 字串 */
export function localTodayStr(now = new Date()) {
  const p = n => String(n).padStart(2, '0')
  return `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`
}
