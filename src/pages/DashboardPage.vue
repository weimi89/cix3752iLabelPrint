<script setup>
import { useRouter } from 'vue-router'
import { useStatusStore } from '@/stores/status'
import { printStatsSummary } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import { useNetworkStatus } from '@/composables/useNetworkStatus'

const router = useRouter()
const status = useStatusStore()
const printStats = ref({ today: 0, yesterday: 0 })
const goPrintStats = () => router.push({ name: 'print-stats' })
const {
  osOnline, anchor, cloudApi, checkedAtMs,
  anchorFailStreak, cloudFailStreak, failThreshold, effectiveIntervalSecs,
  anchorEffectiveOk, cloudEffectiveOk,
  overall, isChecking, checkNow,
} = useNetworkStatus()
let timer = null

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const netOverallColor = computed(() => ({
  ok: 'success',
  degraded: 'warning',
  unconfigured: 'warning',
  down: 'error',
}[overall.value]))

const netOverallIcon = computed(() => ({
  ok: 'tabler-wifi',
  degraded: 'tabler-wifi-1',
  unconfigured: 'tabler-cloud-off',
  down: 'tabler-wifi-off',
}[overall.value]))

const lastCheckedText = computed(() => {
  if (!checkedAtMs.value) return ''
  return new Date(checkedAtMs.value).toLocaleString()
})

const refreshPrintStats = async () => {
  try {
    const s = await printStatsSummary()
    if (s) printStats.value = { today: s.today || 0, yesterday: s.yesterday || 0 }
  } catch (e) {
    console.warn('印單統計載入失敗', e)
  }
}

onMounted(async () => {
  if (isTauriRuntime) {
    await status.refreshAll()
    timer = setInterval(() => status.refreshAll(), 5000)
  }
  await refreshPrintStats()
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
})

const formatBytes = bytes => {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
  return `${v.toFixed(i ? 1 : 0)} ${units[i]}`
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.dashboard.title')" :subtitle="$t('page.dashboard.subtitle')" icon="tabler-layout-dashboard" />

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      {{ $t('page.dashboard.previewModeAlert') }}
    </VAlert>

    <!-- 上半:即時狀態(Middleware / 雲端) -->
    <VRow dense>
      <VCol cols="12" md="6">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar :color="status.server.running ? 'success' : 'error'" variant="tonal">
                <VIcon icon="tabler-server-bolt" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.dashboard.middleware') }}</VCardTitle>
            <VCardSubtitle>{{ status.server.bind_addr || $t('page.dashboard.notStarted') }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>

      <VCol cols="12" md="6">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar :color="status.cloud.logged_in ? 'success' : 'warning'" variant="tonal">
                <VIcon icon="tabler-cloud-check" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.dashboard.cloudConnection') }}</VCardTitle>
            <VCardSubtitle>{{ status.cloud.logged_in ? status.cloud.api_base : $t('user.notLoggedIn') }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <!-- 印單統計快覽:今日 / 昨日,點卡片跳轉至完整統計頁 -->
    <VCard class="card-shadow mt-3 print-stats-card" @click="goPrintStats">
      <VCardItem>
        <template #prepend>
          <VAvatar color="primary" variant="tonal">
            <VIcon icon="tabler-chart-bar" />
          </VAvatar>
        </template>
        <VCardTitle>{{ $t('page.dashboard.printStatsTitle') }}</VCardTitle>
        <VCardSubtitle>{{ $t('page.dashboard.printStatsSubtitle') }}</VCardSubtitle>
        <template #append>
          <div class="d-flex align-center gap-4 pe-2">
            <div class="text-end">
              <div class="text-h4 text-primary">{{ printStats.today }}</div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.today') }}</div>
            </div>
            <VDivider vertical />
            <div class="text-end">
              <div class="text-h5">{{ printStats.yesterday }}</div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.yesterday') }}</div>
            </div>
            <VIcon icon="tabler-chevron-right" class="text-medium-emphasis" />
          </div>
        </template>
      </VCardItem>
    </VCard>

    <!-- 下半:本日統計(請求/成功率/cache 命中率/快取容量) -->
    <VRow dense class="mt-1">
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-arrows-exchange" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.today.request_count }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.dashboard.todayRequests') }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="success" variant="tonal">
                <VIcon icon="tabler-circle-check" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.successRate }}%</VCardTitle>
            <VCardSubtitle>{{ $t('page.dashboard.todaySuccessRate', { ok: status.today.success_count, total: status.today.request_count }) }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="warning" variant="tonal">
                <VIcon icon="tabler-target-arrow" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.cacheHitRatePct }}%</VCardTitle>
            <VCardSubtitle>{{ $t('page.dashboard.cacheHitRate', { hit: status.cache.hit_count, miss: status.cache.miss_count }) }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-database" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.cache.file_count }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.dashboard.cachedLabels', { size: formatBytes(status.cache.total_bytes) }) }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <VCard class="card-shadow mt-4">
      <VCardItem>
        <template #prepend>
          <VAvatar :color="netOverallColor" variant="tonal">
            <VIcon :icon="netOverallIcon" />
          </VAvatar>
        </template>
        <VCardTitle class="d-flex align-center">
          {{ $t('network.card.title') }}
          <VChip :color="netOverallColor" size="x-small" variant="tonal" class="ms-2">
            {{ $t(`network.overall.${overall}`) }}
          </VChip>
          <VSpacer />
          <VBtn
            variant="text"
            size="small"
            color="primary"
            :loading="isChecking"
            @click="checkNow"
          >
            <VIcon icon="tabler-refresh" size="16" class="me-1" />
            {{ $t('network.checkNow') }}
          </VBtn>
        </VCardTitle>
        <VCardSubtitle v-if="lastCheckedText" class="text-caption">
          {{ $t('network.lastCheckedAt') }} {{ lastCheckedText }}
        </VCardSubtitle>
      </VCardItem>
      <VDivider />
      <VList density="compact" class="py-1">
        <VListItem>
          <template #prepend>
            <VIcon
              :icon="osOnline ? 'tabler-circle-check' : 'tabler-circle-x'"
              :color="osOnline ? 'success' : 'error'"
              size="18"
              class="me-2"
            />
          </template>
          <VListItemTitle class="text-body-2">{{ $t('network.layer.os') }}</VListItemTitle>
          <VListItemSubtitle class="text-caption">
            {{ osOnline ? $t('network.statusKind.ok') : $t('network.osOffline') }}
          </VListItemSubtitle>
        </VListItem>
        <VListItem>
          <template #prepend>
            <VIcon
              v-if="anchor?.kind === 'ok'"
              icon="tabler-circle-check"
              color="success"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else-if="anchorEffectiveOk"
              icon="tabler-refresh-alert"
              color="warning"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else
              icon="tabler-circle-x"
              color="error"
              size="18"
              class="me-2"
            />
          </template>
          <VListItemTitle class="text-body-2">{{ $t('network.layer.anchor') }}</VListItemTitle>
          <VListItemSubtitle class="text-caption">
            <template v-if="anchor?.kind === 'ok'">{{ $t('network.latencyMs', { n: anchor.latency_ms }) }}</template>
            <template v-else-if="anchor && anchorEffectiveOk">
              {{ $t('network.retryingStreak', { n: anchorFailStreak, total: failThreshold }) }} · {{ anchor.error }}
            </template>
            <template v-else-if="anchor">{{ anchor.error }}</template>
            <template v-else>{{ $t('network.statusKind.unknown') }}</template>
          </VListItemSubtitle>
        </VListItem>
        <VListItem>
          <template #prepend>
            <VIcon
              v-if="cloudApi?.kind === 'reachable'"
              icon="tabler-circle-check"
              color="success"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else-if="cloudApi?.kind === 'unreachable' && cloudEffectiveOk !== false"
              icon="tabler-refresh-alert"
              color="warning"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else-if="cloudApi?.kind === 'unreachable'"
              icon="tabler-circle-x"
              color="error"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else
              icon="tabler-circle-dashed"
              color="grey"
              size="18"
              class="me-2"
            />
          </template>
          <VListItemTitle class="text-body-2">{{ $t('network.layer.cloudApi') }}</VListItemTitle>
          <VListItemSubtitle class="text-caption">
            <template v-if="cloudApi?.kind === 'reachable'">
              HTTP {{ cloudApi.status }} · {{ $t('network.latencyMs', { n: cloudApi.latency_ms }) }}
            </template>
            <template v-else-if="cloudApi?.kind === 'unreachable' && cloudEffectiveOk !== false">
              {{ $t('network.retryingStreak', { n: cloudFailStreak, total: failThreshold }) }} · {{ cloudApi.error }}
            </template>
            <template v-else-if="cloudApi?.kind === 'unreachable'">{{ cloudApi.error }}</template>
            <template v-else-if="cloudApi?.kind === 'not_configured'">{{ $t('network.statusKind.notConfigured') }}</template>
            <template v-else>{{ $t('network.statusKind.unknown') }}</template>
          </VListItemSubtitle>
        </VListItem>
      </VList>
      <VDivider />
      <div class="px-4 py-2 text-caption text-disabled">
        {{ $t('network.nextCheckIn', { n: effectiveIntervalSecs }) }}
      </div>
    </VCard>

    <VAlert v-if="!status.cloud.logged_in && isTauriRuntime" type="warning" variant="tonal" class="mt-4" icon="tabler-alert-triangle">
      {{ $t('page.dashboard.cloudNotLoggedInAlert') }}
    </VAlert>
  </div>
</template>

<style scoped lang="scss">
.print-stats-card {
  cursor: pointer;
  transition: transform 0.15s ease, box-shadow 0.15s ease;

  &:hover {
    transform: translateY(-1px);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.08);
  }
}
</style>
