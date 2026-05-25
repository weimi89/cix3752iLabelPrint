<script setup>
import { useRouter } from 'vue-router'
import { useStatusStore } from '@/stores/status'
import { useZoom } from '@/composables/useZoom'
import { useUpdater } from '@/composables/useUpdater'

defineProps({
  toggleVerticalOverlayNavActive: {
    type: Function,
    required: false,
    default: () => {},
  },
})

const router = useRouter()
const status = useStatusStore()
const { zoom, zoomIn, zoomOut, zoomReset } = useZoom()
const {
  updateAvailable, updateInfo, isDownloading, downloadProgress, lastError,
  checkForUpdates, downloadAndInstall, dismissUpdate,
} = useUpdater()

const goPrintStats = () => router.push({ name: 'print-stats' })

const showUpdateDialog = ref(false)
const openUpdateDialog = () => { showUpdateDialog.value = true }
</script>

<template>
  <div class="d-flex h-100 align-center">
    <VBtn
      icon
      variant="text"
      color="default"
      class="ms-n3 d-lg-none"
      @click="toggleVerticalOverlayNavActive(true)"
    >
      <VIcon size="26" icon="tabler-menu-2" />
    </VBtn>
    <VChip
      class="cursor-pointer"
      color="primary"
      variant="tonal"
      size="small"
      @click="goPrintStats"
    >
      <VIcon icon="tabler-chart-bar" size="16" start />
      <span class="font-weight-medium">{{ $t('page.printStats.sinceReset') }} {{ status.printStats.since_reset }}</span>
      <span class="text-medium-emphasis ms-2">{{ $t('page.printStats.past24h') }} {{ status.printStats.past_24h }}</span>
      <VTooltip activator="parent" location="bottom">
        {{ $t('page.dashboard.printStatsTitle') }}
      </VTooltip>
    </VChip>
    <VSpacer />
    <VMenu :close-on-content-click="false" offset="8" location="bottom end">
      <template #activator="{ props: menuProps }">
        <VBtn icon size="small" variant="text" color="default" v-bind="menuProps">
          <VIcon icon="tabler-zoom-in-area" size="22" />
        </VBtn>
      </template>
      <VSheet rounded elevation="3" class="d-flex align-center pa-1">
        <VBtn icon size="x-small" variant="text" :disabled="zoom <= 0.5" @click="zoomOut">
          <VIcon icon="tabler-minus" size="18" />
          <VTooltip activator="parent" location="bottom">{{ $t('common.zoomOut') }}</VTooltip>
        </VBtn>
        <span class="text-body-2 font-weight-medium text-center" style="min-width: 44px;">{{ Math.round(zoom * 100) }}%</span>
        <VBtn icon size="x-small" variant="text" :disabled="zoom >= 2.0" @click="zoomIn">
          <VIcon icon="tabler-plus" size="18" />
          <VTooltip activator="parent" location="bottom">{{ $t('common.zoomIn') }}</VTooltip>
        </VBtn>
        <VDivider vertical class="mx-1" />
        <VBtn icon size="x-small" variant="text" :disabled="zoom === 1" @click="zoomReset">
          <VIcon icon="tabler-restore" size="18" />
          <VTooltip activator="parent" location="bottom">{{ $t('common.zoomReset') }}</VTooltip>
        </VBtn>
      </VSheet>
    </VMenu>
    <VBtn
      v-if="updateAvailable"
      icon
      size="small"
      variant="text"
      color="warning"
      @click="openUpdateDialog"
    >
      <VBadge dot color="warning">
        <VIcon icon="tabler-download" size="22" />
      </VBadge>
      <VTooltip activator="parent" location="bottom">
        {{ $t('updater.available', { version: updateInfo?.version }) }}
      </VTooltip>
    </VBtn>
    <NetworkStatusIndicator />
    <LocaleSwitcher />

    <VDialog v-model="showUpdateDialog" max-width="480" persistent>
      <VCard>
        <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
          <VIcon icon="tabler-arrow-up-circle" color="warning" class="me-2" />
          {{ $t('updater.title') }}
        </VCardTitle>
        <VCardText class="pa-4">
          <div class="mb-2">
            <span class="text-body-2 text-medium-emphasis">{{ $t('updater.current') }}</span>
            <strong class="ms-1">v{{ updateInfo?.currentVersion }}</strong>
            <VIcon icon="tabler-arrow-right" size="16" class="mx-2" />
            <span class="text-body-2 text-medium-emphasis">{{ $t('updater.next') }}</span>
            <strong class="ms-1 text-warning">v{{ updateInfo?.version }}</strong>
          </div>
          <div v-if="updateInfo?.notes" class="text-body-2 update-notes mt-3 pa-3 rounded bg-grey-100">
            <pre class="ma-0">{{ updateInfo.notes }}</pre>
          </div>
          <VProgressLinear
            v-if="isDownloading"
            :model-value="downloadProgress"
            color="warning"
            height="8"
            rounded
            class="mt-4"
          />
          <div v-if="isDownloading" class="text-caption text-medium-emphasis text-center mt-1">
            {{ $t('updater.downloading') }} {{ downloadProgress }}%
          </div>
          <VAlert v-if="lastError" type="error" variant="tonal" density="compact" class="mt-3">
            {{ lastError }}
          </VAlert>
        </VCardText>
        <VCardActions class="px-4 pb-3">
          <VSpacer />
          <VBtn
            variant="text"
            :disabled="isDownloading"
            @click="() => { showUpdateDialog = false; dismissUpdate() }"
          >
            {{ $t('updater.later') }}
          </VBtn>
          <VBtn
            color="warning"
            variant="elevated"
            :loading="isDownloading"
            :disabled="isDownloading"
            @click="downloadAndInstall"
          >
            <VIcon icon="tabler-download" size="18" class="me-1" />
            {{ $t('updater.install') }}
          </VBtn>
        </VCardActions>
      </VCard>
    </VDialog>
  </div>
</template>

<style scoped>
.update-notes pre {
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 13px;
}
</style>
