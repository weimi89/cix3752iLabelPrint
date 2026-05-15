import { createRouter, createWebHashHistory } from 'vue-router'

import DashboardPage from '@/pages/DashboardPage.vue'
import ScanPrintPage from '@/pages/ScanPrintPage.vue'
import AutoPrintPage from '@/pages/AutoPrintPage.vue'
import PreGeneratePage from '@/pages/PreGeneratePage.vue'
import PrinterSettingsPage from '@/pages/PrinterSettingsPage.vue'
import ServerSettingsPage from '@/pages/ServerSettingsPage.vue'
import CacheSettingsPage from '@/pages/CacheSettingsPage.vue'
import CloudSettingsPage from '@/pages/CloudSettingsPage.vue'
import EventLogPage from '@/pages/EventLogPage.vue'
import QueueLogPage from '@/pages/QueueLogPage.vue'
import SortChannelsPage from '@/pages/SortChannelsPage.vue'
import DispatchProvidersPage from '@/pages/DispatchProvidersPage.vue'

const routes = [
  { path: '/', name: 'dashboard', component: DashboardPage,
    meta: { title: '儀表板', icon: 'tabler-layout-dashboard' } },
  { path: '/scan-print', name: 'scan-print', component: ScanPrintPage,
    meta: { title: '掃描列印', icon: 'tabler-browser', group: '訂單列印' } },
  { path: '/auto-print', name: 'auto-print', component: AutoPrintPage,
    meta: { title: '自動印單', icon: 'tabler-cloud-cog', group: '訂單列印' } },
  { path: '/pre-generate', name: 'pre-generate', component: PreGeneratePage,
    meta: { title: '面單預產', icon: 'tabler-photo-down', group: '訂單列印' } },
  { path: '/sort-channels', name: 'sort-channels', component: SortChannelsPage,
    meta: { title: '分揀通道', icon: 'tabler-route', group: '設定' } },
  { path: '/dispatch-providers', name: 'dispatch-providers', component: DispatchProvidersPage,
    meta: { title: '指派物流', icon: 'tabler-truck-delivery', group: '設定' } },
  { path: '/printer-settings', name: 'printer-settings', component: PrinterSettingsPage,
    meta: { title: '印表機設定', icon: 'tabler-printer', group: '設定' } },
  { path: '/server-settings', name: 'server-settings', component: ServerSettingsPage,
    meta: { title: '服務設定', icon: 'tabler-server-2', group: '設定' } },
  { path: '/cache-settings', name: 'cache-settings', component: CacheSettingsPage,
    meta: { title: '圖片快取', icon: 'tabler-photo', group: '設定' } },
  { path: '/cloud-settings', name: 'cloud-settings', component: CloudSettingsPage,
    meta: { title: '雲端 API', icon: 'tabler-cloud-network', group: '設定' } },
  { path: '/event-log', name: 'event-log', component: EventLogPage,
    meta: { title: '事件記錄', icon: 'tabler-bell-ringing', group: '設定' } },
  { path: '/queue-log', name: 'queue-log', component: QueueLogPage,
    meta: { title: '佇列歷史', icon: 'tabler-truck-loading', group: '設定' } },
]

export default createRouter({
  history: createWebHashHistory(),
  routes,
})
