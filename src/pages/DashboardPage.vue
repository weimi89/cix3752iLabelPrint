<script setup>
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useStatusStore } from '@/stores/status'
import AppHeader from '@/components/AppHeader.vue'
import { useNetworkStatus } from '@/composables/useNetworkStatus'
import { workSessionReset, localLanIps } from '@/api/tauri'
import { errorMessageFromException } from '@/composables/useLabelStatus'

const { t } = useI18n()
const router = useRouter()
const status = useStatusStore()
const goPrintStats = () => router.push({ name: 'print-stats' })

const resetDialog = ref(false)
const resetError = ref('')
const confirmReset = async () => {
  try {
    await workSessionReset()
    resetDialog.value = false
    resetError.value = ''
    await status.refreshPrintStats()
  } catch (e) {
    resetError.value = errorMessageFromException(e)
  }
}

// 起算時間 fallback「—」,避免首次載入空字串時版型塌掉
const summarySinceLabel = computed(() => status.printStats.since_reset_at || '—')
const {
  osOnline, anchor, cloudApi, checkedAtMs,
  anchorFailStreak, cloudFailStreak, failThreshold, effectiveIntervalSecs,
  anchorEffectiveOk, cloudEffectiveOk,
  overall, isChecking, checkNow,
} = useNetworkStatus()

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 本機 LAN IP(工控機連線位址);IP 不常變,進頁抓一次即可
const lanInfo = ref({ ips: [], port: 18080 })
onMounted(async () => {
  try {
    lanInfo.value = await localLanIps()
  } catch (e) {
    // 取不到網卡 IP 不影響其他儀表板資訊
    console.warn('localLanIps 失敗', e)
  }
})

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

    <!-- 本場累計 banner — 跨日 / 換班 / 計件結算的核心指標,永遠醒目 -->
    <VCard class="mb-2 card-shadow session-banner">
      <VCardText class="d-flex align-center gap-4">
        <VAvatar color="primary" variant="flat" size="56">
          <VIcon icon="tabler-restore" size="32" />
        </VAvatar>
        <div class="flex-grow-1 d-flex flex-column" style="min-width: 0;">
          <div class="text-body-small text-medium-emphasis">{{ $t('page.printStats.sinceReset') }}</div>
          <div class="text-display-large font-weight-bold text-primary" style="line-height: 1.1;">{{ status.printStats.since_reset }}</div>
          <div class="text-body-small text-medium-emphasis mt-1">
            <VIcon icon="tabler-clock-play" size="14" class="me-1" />{{ $t('page.printStats.sinceLabel') }} {{ summarySinceLabel }}
          </div>
        </div>
        <VBtn
          color="warning"
          variant="flat"
          prepend-icon="tabler-refresh-dot"
          size="large"
          @click="resetDialog = true"
        >
          {{ $t('page.printStats.resetBtn') }}
        </VBtn>
      </VCardText>
    </VCard>

    <!-- 上半:即時狀態(中介服務 / 雲端 / 印單統計)— 單行等高 -->
    <VRow density="compact">
      <VCol cols="12" md="4">
        <VCard class="card-shadow h-100">
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

      <VCol cols="12" md="4">
        <VCard class="card-shadow h-100">
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

      <VCol cols="12" md="4">
        <VCard class="card-shadow print-stats-card h-100" @click="goPrintStats">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-chart-bar" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.dashboard.printStatsTitle') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.dashboard.printStatsSubtitle') }}</VCardSubtitle>
            <template #append>
              <div class="d-flex align-center gap-3 pe-1">
                <div class="text-end">
                  <div class="text-title-large">{{ status.printStats.past_24h }}</div>
                  <div class="text-body-small text-medium-emphasis">{{ $t('page.printStats.past24h') }}</div>
                </div>
                <VIcon icon="tabler-chevron-right" class="text-medium-emphasis" />
              </div>
            </template>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <!-- 本日統計(請求/成功率/cache 命中率/快取容量) -->
    <VRow density="compact" class="mt-1">
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-arrows-exchange" />
              </VAvatar>
            </template>
            <VCardTitle class="d-flex align-center ga-2">
              {{ status.today.request_count }}
              <!-- NoRead(相機讀不到單號)件數:請求數的失敗細分,>0 才顯示 -->
              <VChip v-if="status.today.noread_count > 0" color="warning" size="x-small" label>
                {{ $t('page.dashboard.todayNoRead', { n: status.today.noread_count }) }}
              </VChip>
            </VCardTitle>
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
            <VCardSubtitle>{{ $t('page.dashboard.todaySuccessRate', { ok: status.today.success_count, total: status.effectiveTodayRequests }) }}</VCardSubtitle>
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

    <!-- 工控機連線位址(LAN IP,部署時工控機要連的位址) -->
    <VRow density="compact" class="mt-1">
      <VCol cols="12">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-network" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.dashboard.lanIpLabel') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.dashboard.lanIpHint') }}</VCardSubtitle>
          </VCardItem>
          <VCardText class="pt-0 d-flex flex-wrap ga-2">
            <VChip
              v-for="ip in lanInfo.ips"
              :key="ip.ip"
              color="info"
              variant="tonal"
              label
            >
              <VIcon icon="tabler-router" start size="16" />
              {{ ip.ip }}:{{ lanInfo.port }}
              <span class="text-disabled ms-1">{{ ip.name }}</span>
            </VChip>
            <span v-if="!lanInfo.ips.length" class="text-disabled">—</span>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <VCard class="card-shadow mt-2 network-status-card">
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
        <VCardSubtitle v-if="lastCheckedText" class="text-body-small">
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
          <VListItemTitle class="text-body-medium">{{ $t('network.layer.os') }}</VListItemTitle>
          <VListItemSubtitle class="text-body-small">
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
          <VListItemTitle class="text-body-medium">{{ $t('network.layer.anchor') }}</VListItemTitle>
          <VListItemSubtitle class="text-body-small">
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
          <VListItemTitle class="text-body-medium">{{ $t('network.layer.cloudApi') }}</VListItemTitle>
          <VListItemSubtitle class="text-body-small">
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
      <div class="px-4 py-2 text-body-small text-disabled">
        {{ $t('network.nextCheckIn', { n: effectiveIntervalSecs }) }}
      </div>
    </VCard>

    <VAlert v-if="!status.cloud.logged_in && isTauriRuntime" type="warning" variant="tonal" class="mt-2" icon="tabler-alert-triangle">
      {{ $t('page.dashboard.cloudNotLoggedInAlert') }}
    </VAlert>

    <!-- 重置本場累計 — 與 PrintStatsPage 同對話框語意 -->
    <VDialog v-model="resetDialog" max-width="420">
      <VCard>
        <VCardTitle>{{ $t('page.printStats.resetDialogTitle') }}</VCardTitle>
        <VCardText>
          {{ $t('page.printStats.resetDialogBody') }}
          <VAlert v-if="resetError" type="error" variant="tonal" class="mt-2">{{ resetError }}</VAlert>
        </VCardText>
        <VCardActions>
          <VSpacer />
          <VBtn variant="text" @click="resetDialog = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="primary" variant="flat" @click="confirmReset">{{ $t('page.printStats.resetConfirm') }}</VBtn>
        </VCardActions>
      </VCard>
    </VDialog>
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

// 本場累計 banner — 主色描邊 + 些微背景強調,讓它「跳出」儀表板首屏
.session-banner {
  border-block-start: 3px solid rgb(var(--v-theme-primary));
  background: linear-gradient(
    to right,
    rgba(var(--v-theme-primary), 0.04),
    rgba(var(--v-theme-primary), 0)
  );
}

// Vuetify VListItem 在 #prepend slot 下會塞一個 .v-list-item__spacer (預設 16px) 把圖示推離 content
// 這裡縮小 spacer,讓圖示貼近文字
.network-status-card :deep(.v-list-item__spacer) {
  inline-size: 8px;
}
</style>
