<script setup>
// 多螢幕看板啟動鈕:點選後列出所有螢幕,選一個即在該螢幕全螢幕無邊框開啟本頁看板。
// 在看板子視窗內(label display-*)自身會隱藏,避免遞迴開窗。
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDisplayWindow } from '@/composables/useDisplayWindow'
import { errorMessageFromException } from '@/composables/useLabelStatus'

const props = defineProps({
  route: { type: String, required: true },       // 如 '/print-stats'
  windowLabel: { type: String, required: true },  // 如 'display-stats'
  title: { type: String, required: true },        // 子視窗標題
})

const { t } = useI18n()
const { getMonitors, open } = useDisplayWindow()

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 若自己就是看板子視窗,隱藏此鈕(避免在看板內再開看板)
let isDisplayWindow = false
if (isTauriRuntime) {
  try {
    isDisplayWindow = getCurrentWebviewWindow().label.startsWith('display-')
  } catch { /* 取不到 label 就當作主視窗 */ }
}

const menu = ref(false)
const monitors = ref([])
const loading = ref(false)
const launchingIdx = ref(-1)

const loadMonitors = async opened => {
  if (!opened) return
  loading.value = true
  try {
    monitors.value = await getMonitors()
  } catch (e) {
    toast(`${t('page.display.failed')}: ${errorMessageFromException(e)}`, { type: 'error' })
    monitors.value = []
  } finally {
    loading.value = false
  }
}

const launch = async (mon, { fullscreen, borderless }) => {
  launchingIdx.value = mon ? mon.index : -2
  try {
    const { reused } = await open({
      label: props.windowLabel,
      route: props.route,
      title: props.title,
      monitor: mon,
      fullscreen,
      borderless,
    })
    const where = mon ? mon.name : t('page.display.windowMode')
    toast(
      reused ? t('page.display.focused') : t('page.display.opened', { screen: where }),
      { type: 'success' },
    )
    menu.value = false
  } catch (e) {
    toast(`${t('page.display.failed')}: ${errorMessageFromException(e)}`, { type: 'error' })
  } finally {
    launchingIdx.value = -1
  }
}
</script>

<template>
  <VMenu v-if="isTauriRuntime && !isDisplayWindow" v-model="menu" :close-on-content-click="false" @update:model-value="loadMonitors">
    <template #activator="{ props: act }">
      <VBtn v-bind="act" variant="outlined" color="primary">
        <VIcon icon="tabler-device-desktop" size="16" class="me-1" />
        {{ $t('page.display.open') }}
      </VBtn>
    </template>

    <VCard min-width="280" class="pa-1">
      <VCardText class="text-body-small text-medium-emphasis pb-1">
        {{ $t('page.display.pickScreen') }}
      </VCardText>

      <div v-if="loading" class="d-flex justify-center py-4">
        <VProgressCircular indeterminate size="22" color="primary" />
      </div>

      <VList v-else density="compact" nav>
        <VListItem
          v-for="mon in monitors"
          :key="mon.index"
          :disabled="launchingIdx === mon.index"
          @click="launch(mon, { fullscreen: true, borderless: true })"
        >
          <template #prepend>
            <VIcon icon="tabler-device-desktop" size="18" :color="mon.isCurrent ? 'warning' : 'primary'" />
          </template>
          <VListItemTitle>
            {{ $t('page.display.screen', { n: mon.index + 1 }) }}
            <VChip v-if="mon.isCurrent" size="x-small" color="warning" variant="tonal" class="ms-1">{{ $t('page.display.current') }}</VChip>
          </VListItemTitle>
          <VListItemSubtitle>{{ mon.width }} × {{ mon.height }} · {{ $t('page.display.fullscreenKanban') }}</VListItemSubtitle>
          <template #append>
            <VProgressCircular v-if="launchingIdx === mon.index" indeterminate size="16" width="2" />
          </template>
        </VListItem>

        <VDivider class="my-1" />

        <VListItem :disabled="launchingIdx === -2" @click="launch(null, { fullscreen: false, borderless: false })">
          <template #prepend>
            <VIcon icon="tabler-window" size="18" />
          </template>
          <VListItemTitle>{{ $t('page.display.windowMode') }}</VListItemTitle>
          <VListItemSubtitle>{{ $t('page.display.windowModeHint') }}</VListItemSubtitle>
        </VListItem>
      </VList>
    </VCard>
  </VMenu>
</template>
