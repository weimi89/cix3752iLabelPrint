/**
 * Sidebar 導航結構 — 對齊 cix3752iWeb 的 nested 父子節點
 * 每個 section 含 title 與 items；item 可有 children（展開後變子節點 + dot）
 */
export const navSections = [
  {
    title: '主功能',
    items: [
      { title: '首頁', icon: 'tabler-layout-dashboard', to: { name: 'dashboard' } },
    ],
  },
  {
    title: '訂單列印',
    items: [
      {
        title: '訂單列印',
        icon: 'tabler-printer',
        children: [
          { title: '掃描列印', to: { name: 'scan-print' } },
          { title: '自動印單', to: { name: 'auto-print' } },
          { title: '面單預產', to: { name: 'pre-generate' } },
        ],
      },
    ],
  },
  {
    title: '設定',
    items: [
      { title: '印表機設定', icon: 'tabler-printer', to: { name: 'printer-settings' } },
      { title: 'Server / Cache', icon: 'tabler-server-2', to: { name: 'server-settings' } },
      { title: '雲端 API', icon: 'tabler-cloud-network', to: { name: 'cloud-settings' } },
      { title: 'Log 查詢', icon: 'tabler-file-text', to: { name: 'logs' } },
    ],
  },
]
