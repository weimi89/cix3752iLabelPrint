<script setup>
import { useRouter } from 'vue-router'
import { useStatusStore } from '@/stores/status'

defineProps({
  toggleVerticalOverlayNavActive: {
    type: Function,
    required: false,
    default: () => {},
  },
})

const router = useRouter()
const status = useStatusStore()

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
    <NetworkStatusIndicator />
    <LocaleSwitcher />
  </div>
</template>
