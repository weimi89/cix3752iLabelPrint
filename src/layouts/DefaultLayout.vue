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
import { navItems } from '@/config/navConfig'
import { useSkins } from '@core/composable/useSkins'
import { useStatusStore } from '@/stores/status'
import { useParcelAlert } from '@/composables/useParcelAlert'
import { useDeviceAlert } from '@/composables/useDeviceAlert'

const { layoutAttrs } = useSkins()
const configStore = useLayoutConfigStore()
const status = useStatusStore()
const parcelAlert = useParcelAlert()
const deviceAlert = useDeviceAlert()
const appVersion = ref('')

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__
let timer = null
let unlistenPrintStats = null

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
