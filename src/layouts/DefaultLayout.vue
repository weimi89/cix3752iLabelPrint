<script setup>
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { VerticalNavLayout } from '@layouts'
import { layoutConfig } from '@layouts'
import { useLayoutConfigStore } from '@layouts/stores/config'
import AppNavbar from '@/components/AppNavbar.vue'
import AppLogo from '@/components/AppLogo.vue'
import ClearanceProgressWidget from '@/components/ClearanceProgressWidget.vue'
import { navItems } from '@/config/navConfig'
import { useSkins } from '@core/composable/useSkins'
import { useStatusStore } from '@/stores/status'
import { useParcelAlert } from '@/composables/useParcelAlert'
import { useDeviceAlert } from '@/composables/useDeviceAlert'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { playSound } from '@/composables/useSoundEffects'

const { layoutAttrs } = useSkins()
const configStore = useLayoutConfigStore()
const status = useStatusStore()
const parcelAlert = useParcelAlert()
const deviceAlert = useDeviceAlert()
const { t } = useI18n()
const appVersion = ref('')

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__
let timer = null
let unlistenPrintStats = null
let unlistenBindFailed = null

// 本機 HTTP server 未啟動(如 18080 被占用)的主動告警:持續性 toast + 警示音。
// 後端 bootstrap 失敗時的 emit 發生在 webview 載入前、前端聽不到 → 掛載後以首查
// server 狀態補上這個告警;另掛 listen 接住日後執行期的 emit(雙保險)。
// 固定 toastId:5s 輪詢期間不重複洗版,server 恢復前保留一則常駐提示。
const alertServerDown = () => {
  playSound('effect_2')
  toast(t('serverAlert.bindFailed'), {
    type: 'error',
    toastId: 'server-bind-failed',
    autoClose: false, // 分揀線停擺級告警:不自動消失,操作員處理完自行關閉
  })
}

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch {
    // 非 Tauri 環境(如純瀏覽器預覽)取不到版本,留空即可
  }
  if (!isTauriRuntime) {
    await status.refreshPrintStats()
    return
  }
  await status.refreshAll()
  // 啟動時 server 綁定失敗(port 被占用)的死信補償:bootstrap 的 emit 前端聽不到,
  // 以掛載後首查狀態主動告警,否則工控機整線連不上、操作員卻毫無提示。
  if (!status.server.running) alertServerDown()
  try {
    unlistenBindFailed = await listen('server-bind-failed', alertServerDown)
  } catch (e) {
    console.warn('listen server-bind-failed 失敗', e)
  }
  // system status 仍 5s 輪詢(server/queue/cache/today/cloud)
  timer = setInterval(() => status.refreshAll(), 5000)
  // 印單統計改用事件驅動:任何來源(scan/auto/ipc)寫入 print_event 後立即推送
  // 工控機 IPC 請求一進來就會即時更新,不必等 5s
  try {
    unlistenPrintStats = await listen('print-stats-updated', () => {
      status.refreshPrintStats()
    })
  } catch (e) {
    console.warn('listen print-stats-updated 失敗', e)
  }
  // 工控機請求失敗的全域聲音 + toast 提示(成功不出聲)
  try {
    await parcelAlert.start()
  } catch (e) {
    console.warn('listen parcel-alert 失敗', e)
  }
  // 工控機設備異常(卡包裹 / USB 斷線 …)的雙語 TTS 廣播 + toast
  try {
    await deviceAlert.start()
  } catch (e) {
    console.warn('listen device-alert 失敗', e)
  }
})

onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
  if (unlistenPrintStats) unlistenPrintStats()
  if (unlistenBindFailed) unlistenBindFailed()
  parcelAlert.stop()
  deviceAlert.stop()
})
</script>

<template>
  <VerticalNavLayout
    :home-url="'/'"
    :nav-items="navItems"
    :vertical-nav-attrs="layoutAttrs.verticalNavAttrs"
  >
    <!-- 👉 Sidebar header (logo + 標題 + version + 收合按鈕) -->
    <template #vertical-nav-header>
      <RouterLink to="/" class="app-logo app-title-wrapper">
        <AppLogo />
        <h1 class="app-logo-title leading-normal">智配通</h1>
        <span
          v-if="appVersion"
          class="app-version-badge text-caption font-weight-medium"
        >v{{ appVersion }}</span>
      </RouterLink>
      <!-- 桌面：收合/展開按鈕 -->
      <div class="nav-collapse-btn">
        <Component
          :is="layoutConfig.app.iconRenderer || 'div'"
          v-if="configStore.isVerticalNavCollapsed"
          v-bind="layoutConfig.icons.verticalNavUnPinned"
          @click="configStore.isVerticalNavCollapsed = false"
        />
        <Component
          :is="layoutConfig.app.iconRenderer || 'div'"
          v-else
          v-bind="layoutConfig.icons.verticalNavPinned"
          @click="configStore.isVerticalNavCollapsed = true"
        />
      </div>
    </template>

    <!-- 👉 Navbar -->
    <template #navbar="{ toggleVerticalOverlayNavActive }">
      <AppNavbar :toggle-vertical-overlay-nav-active="toggleVerticalOverlayNavActive" />
    </template>

    <!-- 👉 Pages -->
    <slot />

    <!-- 全域清關進度浮動框(跨頁不消失,Teleport 到 body)-->
    <ClearanceProgressWidget />
  </VerticalNavLayout>
</template>

<style lang="scss" scoped>
.app-logo {
  display: flex;
  align-items: center;
  column-gap: 0.75rem;
  text-decoration: none;
  color: inherit;
  margin-inline-end: auto;
}

.app-logo-title {
  font-size: 1.375rem;
  font-weight: 700;
  letter-spacing: 0.25px;
  line-height: 1.5rem;
  text-transform: capitalize;
}

.app-version-badge {
  color: #fff;
  opacity: 0.85;
  margin-inline-start: 0.25rem;
  align-self: flex-end;
  padding-block-end: 2px;
}

.nav-collapse-btn {
  cursor: pointer;
  font-size: 1.25rem;
  flex-shrink: 0;
  color: rgba(var(--v-theme-on-surface), var(--v-medium-emphasis-opacity));

  &:hover {
    color: rgba(var(--v-theme-on-surface), var(--v-high-emphasis-opacity));
  }
}
</style>

<style lang="scss">
/* 載入 @layouts plugin 的 layout styles */
@use "@layouts/styles/default-layout";

/* 側邊欄收合/展開按鈕 — 不使用 header-action class 以避開模板隱藏規則 */
.layout-vertical-nav .nav-collapse-btn {
  display: flex;
  align-items: center;
  cursor: pointer;
  font-size: 1.25rem;
  flex-shrink: 0;

  > * {
    display: inline-flex !important;
  }
}
</style>
