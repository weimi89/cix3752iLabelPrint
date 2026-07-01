/**
 * Sidebar 導航結構 — 對齊 @layouts/components/VerticalNav 預期格式(扁平陣列)
 * { heading: '...' } — section title
 * { title, icon: {icon}, to: { name } } — link
 * { title, icon: {icon}, children: [...] } — group with nested children
 */
export const navItems = [
  { heading: 'nav.section.main' },
  { title: 'nav.dashboard', icon: { icon: 'tabler-layout-dashboard' }, to: { name: 'dashboard' } },
  { title: 'nav.preGenerate', icon: { icon: 'tabler-photo-down' }, to: { name: 'pre-generate' } },
  { title: 'nav.bagCheck', icon: { icon: 'tabler-packages' }, to: { name: 'bag-check' } },
  { title: 'nav.sortChannels', icon: { icon: 'tabler-route' }, to: { name: 'sort-channels' } },

  { heading: 'nav.section.print' },
  { title: 'nav.scanPrint', icon: { icon: 'tabler-scan' }, to: { name: 'scan-print' } },
  { title: 'nav.autoPrint', icon: { icon: 'tabler-bolt' }, to: { name: 'auto-print' } },

  { heading: 'nav.section.clearance' },
  { title: 'nav.clearanceAdd', icon: { icon: 'tabler-scan' }, to: { name: 'clearance-add' } },
  { title: 'nav.clearanceDispatch', icon: { icon: 'tabler-truck-delivery' }, to: { name: 'clearance-dispatch' } },
  { title: 'nav.warehouseScanner', icon: { icon: 'tabler-package-import' }, to: { name: 'warehouse-scanner' } },

  { heading: 'nav.section.logs' },
  { title: 'nav.printStats', icon: { icon: 'tabler-chart-bar' }, to: { name: 'print-stats' } },
  { title: 'nav.parcelQueryLog', icon: { icon: 'tabler-history' }, to: { name: 'parcel-query-log' } },
  { title: 'nav.alertLog', icon: { icon: 'tabler-alert-triangle' }, to: { name: 'parcel-alert-log' } },
  { title: 'nav.queueLog', icon: { icon: 'tabler-truck-loading' }, to: { name: 'queue-log' } },
  { title: 'nav.eventLog', icon: { icon: 'tabler-bell-ringing' }, to: { name: 'event-log' } },

  { heading: 'nav.section.settings' },
  { title: 'nav.cacheSettings', icon: { icon: 'tabler-photo' }, to: { name: 'cache-settings' } },
  { title: 'nav.dispatchProviders', icon: { icon: 'tabler-truck-delivery' }, to: { name: 'dispatch-providers' } },
  { title: 'nav.printerSettings', icon: { icon: 'tabler-printer' }, to: { name: 'printer-settings' } },
  { title: 'nav.serverSettings', icon: { icon: 'tabler-server-2' }, to: { name: 'server-settings' } },
  { title: 'nav.cloudSettings', icon: { icon: 'tabler-cloud' }, to: { name: 'cloud-settings' } },
]
