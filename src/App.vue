<script setup>
import { useTheme } from 'vuetify'
import { hexToRgb } from '@layouts/utils'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import DefaultLayout from '@/layouts/DefaultLayout.vue'
import KioskShell from '@/components/KioskShell.vue'
import { useThemeApply } from '@/composables/useThemeApply'
import { installZoomShortcuts } from '@/composables/useZoom'
import { isTauriRuntime } from '@/api/runtime'
import { useRoute } from 'vue-router'

useThemeApply()
installZoomShortcuts()

const { global } = useTheme()
const route = useRoute()

// 看板子視窗(label display-*)走 kiosk:不套 DefaultLayout(無側欄/導覽列,
// 也不重複跑全域輪詢與警示音);各看板頁自身已訂閱事件即時更新。
let isKiosk = false
if (isTauriRuntime) {
  try {
    isKiosk = getCurrentWebviewWindow().label.startsWith('display-')
  } catch { /* 取不到 label 就走一般版面 */ }
}
</script>

<template>
  <VApp :style="`--v-global-theme-primary: ${hexToRgb(global.current.value.colors.primary)}`">
    <KioskShell v-if="isKiosk">
      <RouterView />
    </KioskShell>
    <!-- 登入頁不套版面:還沒通過這道門的人不該看到側欄,也不該啟動全域輪詢 -->
    <RouterView v-else-if="route.name === 'login'" />
    <DefaultLayout v-else>
      <RouterView />
    </DefaultLayout>
  </VApp>
</template>
