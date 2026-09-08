<script setup>
// 多螢幕看板啟動鈕:點選後列出所有螢幕,選一個即在該螢幕全螢幕無邊框開啟本頁看板。
// 在看板子視窗內(label display-*)自身會隱藏,避免遞迴開窗。
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDisplayWindow } from '@/composables/useDisplayWindow'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { localLanIps } from '@/api/tauri'
import QRCode from 'qrcode'

const props = defineProps({
  route: { type: String, required: true },       // 如 '/print-stats'
  windowLabel: { type: String, required: true },  // 如 'display-stats'
  title: { type: String, required: true },        // 子視窗標題
  // 本機也有網頁版時填它的路徑(如 '/board'),選單會多出網址與 QR,
  // 方便用電視或另一台機器開。沒有網頁版的頁面不必傳。
  webPath: { type: String, default: '' },
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
// 網頁版連線資訊(區網網址 + QR)
const webUrls = ref([])
const webQr = ref('')
const loading = ref(false)
const launchingIdx = ref(-1)

const loadWebUrls = async () => {
  if (!props.webPath) return
  try {
    const { ips, port } = await localLanIps()
    webUrls.value = (ips || []).map(i => ({
      name: i.name,
      addr: `${i.ip}:${port}${props.webPath}`,
      url: `http://${i.ip}:${port}${props.webPath}`,
    }))
    // QR 只給第一個位址 —— 多網卡時掃哪個都通,列表仍列出全部供手動輸入
    webQr.value = webUrls.value[0] ? await QRCode.toDataURL(webUrls.value[0].url, { width: 168, margin: 1 }) : ''
  } catch {
    webUrls.value = []
    webQr.value = ''
  }
}

const copyAddr = async addr => {
  try {
    await navigator.clipboard.writeText(addr)
    toast(t('common.copied'), { type: 'success' })
  } catch (e) {
    toast(errorMessageFromException(e), { type: 'error' })
  }
}

const loadMonitors = async opened => {
  if (!opened) return
  loading.value = true
  try {
    monitors.value = await getMonitors()
    await loadWebUrls()
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

      <!-- 網頁版:給電視或另一台機器用網址開,掃 QR 免手動輸入 -->
      <template v-if="webPath && !loading">
        <VDivider class="my-1" />
        <VCardText class="pt-2 pb-3">
          <div class="text-body-small text-medium-emphasis mb-2">{{ $t('page.display.webTitle') }}</div>

          <VAlert v-if="!webUrls.length" type="warning" variant="tonal" density="compact">
            {{ $t('page.display.webNoIp') }}
          </VAlert>

          <template v-else>
            <div v-if="webQr" class="d-flex justify-center mb-2">
              <img :src="webQr" alt="QR" style="border-radius: 8px; background: #fff; padding: 6px;">
            </div>
            <div v-for="u in webUrls" :key="u.addr" class="d-flex align-center ga-1 mb-1">
              <VIcon icon="tabler-network" size="14" class="flex-shrink-0 text-medium-emphasis" />
              <code class="flex-grow-1 text-truncate text-body-small">{{ u.addr }}</code>
              <VChip size="x-small" variant="tonal">{{ u.name }}</VChip>
              <VBtn icon size="x-small" variant="text" color="default" @click="copyAddr(u.url)">
                <VIcon icon="tabler-copy" size="14" />
                <VTooltip activator="parent" location="bottom">{{ $t('common.copy') }}</VTooltip>
              </VBtn>
            </div>
          </template>
        </VCardText>
      </template>
    </VCard>
  </VMenu>
</template>
