import { createRouter, createWebHashHistory } from 'vue-router'

// 頁面元件一律動態載入。
// 全部靜態匯入時整包 JS 約 1.6 MB,現場手機走 Wi-Fi 首次開要七秒以上;
// 拆開後開一頁只載那一頁需要的部分。切頁時多一次小請求,但那是有快取的。

const DashboardPage = () => import('@/pages/DashboardPage.vue')
const BagCheckPage = () => import('@/pages/BagCheckPage.vue')
const ScanPrintPage = () => import('@/pages/ScanPrintPage.vue')
const AutoPrintPage = () => import('@/pages/AutoPrintPage.vue')
const PreGeneratePage = () => import('@/pages/PreGeneratePage.vue')
const PrinterSettingsPage = () => import('@/pages/PrinterSettingsPage.vue')
const ServerSettingsPage = () => import('@/pages/ServerSettingsPage.vue')
const CacheSettingsPage = () => import('@/pages/CacheSettingsPage.vue')
const CloudSettingsPage = () => import('@/pages/CloudSettingsPage.vue')
const EventLogPage = () => import('@/pages/EventLogPage.vue')
const QueueLogPage = () => import('@/pages/QueueLogPage.vue')
const ParcelQueryLogPage = () => import('@/pages/ParcelQueryLogPage.vue')
const ParcelAlertLogPage = () => import('@/pages/ParcelAlertLogPage.vue')
const PrintStatsPage = () => import('@/pages/PrintStatsPage.vue')
const SortChannelsPage = () => import('@/pages/SortChannelsPage.vue')
const SortBoardPage = () => import('@/pages/SortBoardPage.vue')
const DispatchProvidersPage = () => import('@/pages/DispatchProvidersPage.vue')
const ClearanceAddPage = () => import('@/pages/ClearanceAddPage.vue')
const ClearanceDispatchPage = () => import('@/pages/ClearanceDispatchPage.vue')
const FieldOperationMonitorPage = () => import('@/pages/FieldOperationMonitorPage.vue')
const WarehouseScannerPage = () => import('@/pages/WarehouseScannerPage.vue')
const LoginPage = () => import('@/pages/LoginPage.vue')
import { isWebRuntime } from '@/api/runtime'
import { useWebAuth } from '@/composables/useWebAuth'

const routes = [
  { path: '/', name: 'dashboard', component: DashboardPage,
    meta: { title: 'nav.dashboard', icon: 'tabler-layout-dashboard' } },
  { path: '/bag-check', name: 'bag-check', component: BagCheckPage,
    meta: { title: 'nav.bagCheck', icon: 'tabler-packages', group: 'nav.section.main' } },
  { path: '/scan-print', name: 'scan-print', component: ScanPrintPage,
    meta: { title: 'nav.scanPrint', icon: 'tabler-browser', group: 'nav.section.print', desktopOnly: true } },
  { path: '/auto-print', name: 'auto-print', component: AutoPrintPage,
    meta: { title: 'nav.autoPrint', icon: 'tabler-cloud-cog', group: 'nav.section.print', desktopOnly: true } },
  { path: '/pre-generate', name: 'pre-generate', component: PreGeneratePage,
    meta: { title: 'nav.preGenerate', icon: 'tabler-photo-down', group: 'nav.section.print' } },
  { path: '/sort-channels', name: 'sort-channels', component: SortChannelsPage,
    meta: { title: 'nav.sortChannels', icon: 'tabler-route', group: 'nav.section.main' } },
  { path: '/sort-board', name: 'sort-board', component: SortBoardPage,
    meta: { title: 'page.board.title', icon: 'tabler-device-tv' } },
  { path: '/clearance-add', name: 'clearance-add', component: ClearanceAddPage,
    meta: { title: 'nav.clearanceAdd', icon: 'tabler-scan', group: 'nav.section.clearance' } },
  { path: '/clearance-dispatch', name: 'clearance-dispatch', component: ClearanceDispatchPage,
    meta: { title: 'nav.clearanceDispatch', icon: 'tabler-truck-delivery', group: 'nav.section.clearance' } },
  { path: '/field-operation-monitor', name: 'field-operation-monitor', component: FieldOperationMonitorPage,
    meta: { title: 'nav.fieldOperationMonitor', icon: 'tabler-user-check', group: 'nav.section.main' } },
  { path: '/warehouse-scanner', name: 'warehouse-scanner', component: WarehouseScannerPage,
    meta: { title: 'nav.warehouseScanner', icon: 'tabler-package-import', group: 'nav.section.clearance', desktopOnly: true } },
  { path: '/dispatch-providers', name: 'dispatch-providers', component: DispatchProvidersPage,
    meta: { title: 'nav.dispatchProviders', icon: 'tabler-truck-delivery', group: 'nav.section.settings' } },
  { path: '/printer-settings', name: 'printer-settings', component: PrinterSettingsPage,
    meta: { title: 'nav.printerSettings', icon: 'tabler-printer', group: 'nav.section.settings' } },
  { path: '/server-settings', name: 'server-settings', component: ServerSettingsPage,
    meta: { title: 'nav.serverSettings', icon: 'tabler-server-2', group: 'nav.section.settings' } },
  { path: '/cache-settings', name: 'cache-settings', component: CacheSettingsPage,
    meta: { title: 'nav.cacheSettings', icon: 'tabler-photo', group: 'nav.section.settings' } },
  { path: '/cloud-settings', name: 'cloud-settings', component: CloudSettingsPage,
    meta: { title: 'nav.cloudSettings', icon: 'tabler-cloud-network', group: 'nav.section.settings' } },
  { path: '/event-log', name: 'event-log', component: EventLogPage,
    meta: { title: 'nav.eventLog', icon: 'tabler-bell-ringing', group: 'nav.section.settings' } },
  { path: '/queue-log', name: 'queue-log', component: QueueLogPage,
    meta: { title: 'nav.queueLog', icon: 'tabler-truck-loading', group: 'nav.section.settings' } },
  { path: '/parcel-query-log', name: 'parcel-query-log', component: ParcelQueryLogPage,
    meta: { title: 'nav.parcelQueryLog', icon: 'tabler-history', group: 'nav.section.logs' } },
  { path: '/parcel-alert-log', name: 'parcel-alert-log', component: ParcelAlertLogPage,
    meta: { title: 'nav.alertLog', icon: 'tabler-alert-triangle', group: 'nav.section.logs' } },
  { path: '/print-stats', name: 'print-stats', component: PrintStatsPage,
    meta: { title: 'nav.printStats', icon: 'tabler-chart-bar', group: 'nav.section.logs' } },
  // 只有網頁版走得到:桌面 App 不經過 HTTP 這道門,內網來源後端也直接放行
  { path: '/login', name: 'login', component: LoginPage, meta: { public: true } },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

// 網頁版的登入守衛。
// 桌面 App 與內網來源都不會停在這裡 —— 桌面不經過 HTTP,內網後端直接放行,
// refresh() 回報 authenticated 後就照常前往目的地。
router.beforeEach(async to => {
  if (!isWebRuntime || to.meta.public) return true

  // 現場作業頁(接掃描槍、用這台機器出單)網頁版不提供;導覽列不列之外,
  // 直接輸入網址或舊書籤進來也要擋,帶回首頁
  if (to.meta.desktopOnly) return { name: 'dashboard' }

  const { authenticated, checked, refresh } = useWebAuth()

  // 每次開頁都重查一次太浪費,但首次進站一定要問過後端才知道自己算不算內網
  if (!checked.value) await refresh()
  if (authenticated.value) return true

  // 再確認一次:session 可能在別的分頁剛登入
  if (await refresh()) return true

  return { name: 'login', query: to.fullPath === '/' ? {} : { redirect: to.fullPath } }
})

export default router
