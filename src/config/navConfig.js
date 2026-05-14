/**
 * Sidebar 導航結構 — 對齊 @layouts/components/VerticalNav 預期格式(扁平陣列)
 * { heading: '...' } — section title
 * { title, icon: {icon}, to: { name } } — link
 * { title, icon: {icon}, children: [...] } — group with nested children
 */
export const navItems = [
  { heading: '主功能' },
  { title: '儀表板', icon: { icon: 'tabler-layout-dashboard' }, to: { name: 'dashboard' } },

  { heading: '訂單列印' },
  { title: '掃描列印', icon: { icon: 'tabler-scan' }, to: { name: 'scan-print' } },
  { title: '自動印單', icon: { icon: 'tabler-bolt' }, to: { name: 'auto-print' } },
  { title: '面單預產', icon: { icon: 'tabler-photo-down' }, to: { name: 'pre-generate' } },

  { heading: '設定' },
  { title: '印表機設定', icon: { icon: 'tabler-printer' }, to: { name: 'printer-settings' } },
  { title: 'Server', icon: { icon: 'tabler-server-2' }, to: { name: 'server-settings' } },
  { title: '圖片快取', icon: { icon: 'tabler-photo' }, to: { name: 'cache-settings' } },
  { title: '雲端 API', icon: { icon: 'tabler-cloud' }, to: { name: 'cloud-settings' } },

  { heading: '日誌' },
  { title: '事件記錄', icon: { icon: 'tabler-bell-ringing' }, to: { name: 'event-log' } },
  { title: 'Queue 歷史', icon: { icon: 'tabler-truck-loading' }, to: { name: 'queue-log' } },
]
