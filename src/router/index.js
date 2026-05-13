import { createRouter, createWebHashHistory } from 'vue-router'

import DashboardPage from '@/pages/DashboardPage.vue'
import ScanPrintPage from '@/pages/ScanPrintPage.vue'
import AutoPrintPage from '@/pages/AutoPrintPage.vue'
import PreGeneratePage from '@/pages/PreGeneratePage.vue'
import PrinterSettingsPage from '@/pages/PrinterSettingsPage.vue'
import ServerSettingsPage from '@/pages/ServerSettingsPage.vue'
import CloudSettingsPage from '@/pages/CloudSettingsPage.vue'
import LogsPage from '@/pages/LogsPage.vue'

const routes = [
  { path: '/', name: 'dashboard', component: DashboardPage,
    meta: { title: '首頁', icon: 'tabler-layout-dashboard' } },
  { path: '/scan-print', name: 'scan-print', component: ScanPrintPage,
    meta: { title: '掃描列印', icon: 'tabler-browser', group: '訂單列印' } },
  { path: '/auto-print', name: 'auto-print', component: AutoPrintPage,
    meta: { title: '自動印單', icon: 'tabler-cloud-cog', group: '訂單列印' } },
  { path: '/pre-generate', name: 'pre-generate', component: PreGeneratePage,
    meta: { title: '面單預產', icon: 'tabler-photo-down', group: '訂單列印' } },
  { path: '/printer-settings', name: 'printer-settings', component: PrinterSettingsPage,
    meta: { title: '印表機設定', icon: 'tabler-printer', group: '設定' } },
  { path: '/server-settings', name: 'server-settings', component: ServerSettingsPage,
    meta: { title: 'Server / Cache', icon: 'tabler-server-2', group: '設定' } },
  { path: '/cloud-settings', name: 'cloud-settings', component: CloudSettingsPage,
    meta: { title: '雲端 API', icon: 'tabler-cloud-network', group: '設定' } },
  { path: '/logs', name: 'logs', component: LogsPage,
    meta: { title: 'Log 查詢', icon: 'tabler-file-text', group: '設定' } },
]

export default createRouter({
  history: createWebHashHistory(),
  routes,
})
