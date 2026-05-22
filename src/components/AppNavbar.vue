<script setup>
import { useRouter } from 'vue-router'
import { useStatusStore } from '@/stores/status'
import { useZoom } from '@/composables/useZoom'

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

const goPrintStats = () => router.push({ name: 'print-stats' })
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
      <span class="font-weight-medium">{{ $t('page.printStats.today') }} {{ status.printStats.today }}</span>
      <span class="text-medium-emphasis ms-2">{{ $t('page.printStats.yesterday') }} {{ status.printStats.yesterday }}</span>
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
    <NetworkStatusIndicator />
    <LocaleSwitcher />
  </div>
</template>
