import { createRouter, createWebHashHistory } from 'vue-router'

import DashboardPage from '@/pages/DashboardPage.vue'
import BagCheckPage from '@/pages/BagCheckPage.vue'
import ScanPrintPage from '@/pages/ScanPrintPage.vue'
import AutoPrintPage from '@/pages/AutoPrintPage.vue'
import PreGeneratePage from '@/pages/PreGeneratePage.vue'
import PrinterSettingsPage from '@/pages/PrinterSettingsPage.vue'
import ServerSettingsPage from '@/pages/ServerSettingsPage.vue'
import CacheSettingsPage from '@/pages/CacheSettingsPage.vue'
import CloudSettingsPage from '@/pages/CloudSettingsPage.vue'
import EventLogPage from '@/pages/EventLogPage.vue'
import QueueLogPage from '@/pages/QueueLogPage.vue'
import ParcelQueryLogPage from '@/pages/ParcelQueryLogPage.vue'
import ParcelAlertLogPage from '@/pages/ParcelAlertLogPage.vue'
import PrintStatsPage from '@/pages/PrintStatsPage.vue'
import SortChannelsPage from '@/pages/SortChannelsPage.vue'
import DispatchProvidersPage from '@/pages/DispatchProvidersPage.vue'
import ClearanceAddPage from '@/pages/ClearanceAddPage.vue'
import ClearanceDispatchPage from '@/pages/ClearanceDispatchPage.vue'
import FieldOperationMonitorPage from '@/pages/FieldOperationMonitorPage.vue'
import WarehouseScannerPage from '@/pages/WarehouseScannerPage.vue'

const routes = [
  { path: '/', name: 'dashboard', component: DashboardPage,
    meta: { title: 'nav.dashboard', icon: 'tabler-layout-dashboard' } },
  { path: '/bag-check', name: 'bag-check', component: BagCheckPage,
    meta: { title: 'nav.bagCheck', icon: 'tabler-packages', group: 'nav.section.main' } },
  { path: '/scan-print', name: 'scan-print', component: ScanPrintPage,
    meta: { title: 'nav.scanPrint', icon: 'tabler-browser', group: 'nav.section.print' } },
  { path: '/auto-print', name: 'auto-print', component: AutoPrintPage,
    meta: { title: 'nav.autoPrint', icon: 'tabler-cloud-cog', group: 'nav.section.print' } },
  { path: '/pre-generate', name: 'pre-generate', component: PreGeneratePage,
    meta: { title: 'nav.preGenerate', icon: 'tabler-photo-down', group: 'nav.section.print' } },
  { path: '/sort-channels', name: 'sort-channels', component: SortChannelsPage,
    meta: { title: 'nav.sortChannels', icon: 'tabler-route', group: 'nav.section.main' } },
  { path: '/clearance-add', name: 'clearance-add', component: ClearanceAddPage,
    meta: { title: 'nav.clearanceAdd', icon: 'tabler-scan', group: 'nav.section.clearance' } },
  { path: '/clearance-dispatch', name: 'clearance-dispatch', component: ClearanceDispatchPage,
    meta: { title: 'nav.clearanceDispatch', icon: 'tabler-truck-delivery', group: 'nav.section.clearance' } },
  { path: '/field-operation-monitor', name: 'field-operation-monitor', component: FieldOperationMonitorPage,
    meta: { title: 'nav.fieldOperationMonitor', icon: 'tabler-user-check', group: 'nav.section.clearance' } },
  { path: '/warehouse-scanner', name: 'warehouse-scanner', component: WarehouseScannerPage,
    meta: { title: 'nav.warehouseScanner', icon: 'tabler-package-import', group: 'nav.section.clearance' } },
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
]

export default createRouter({
  history: createWebHashHistory(),
  routes,
})
