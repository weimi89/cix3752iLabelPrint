<script setup>
import { useNetworkStatus } from '@/composables/useNetworkStatus'
import { useI18n } from 'vue-i18n'

const {
  osOnline, anchor, cloudApi, checkedAtMs,
  anchorFailStreak, cloudFailStreak, failThreshold, effectiveIntervalSecs,
  anchorEffectiveOk, cloudEffectiveOk,
  overall, isChecking, checkNow,
} = useNetworkStatus()

const { t } = useI18n()

const overallColor = computed(() => ({
  ok: 'success',
  degraded: 'warning',
  unconfigured: 'warning',
  down: 'error',
}[overall.value]))

const overallIcon = computed(() => ({
  ok: 'tabler-wifi',
  degraded: 'tabler-wifi-1',
  unconfigured: 'tabler-cloud-off',
  down: 'tabler-wifi-off',
}[overall.value]))

const overallTooltip = computed(() => t(`network.overall.${overall.value}`))

const lastCheckedText = computed(() => {
  if (!checkedAtMs.value) return t('network.neverChecked')
  const d = new Date(checkedAtMs.value)
  return d.toLocaleString()
})

function osLine() {
  return {
    label: t('network.layer.os'),
    ok: osOnline.value,
    detail: osOnline.value ? t('network.statusKind.ok') : t('network.osOffline'),
  }
}

function anchorLine() {
  const a = anchor.value
  if (!a) return { label: t('network.layer.anchor'), ok: false, detail: t('network.statusKind.unknown') }
  if (a.kind === 'ok') {
    return { label: t('network.layer.anchor'), ok: true, detail: t('network.latencyMs', { n: a.latency_ms }) }
  }
  // 失敗但抖動緩衝中
  if (anchorEffectiveOk.value) {
    return {
      label: t('network.layer.anchor'),
      ok: 'retry',
      detail: t('network.retryingStreak', { n: anchorFailStreak.value, total: failThreshold.value }) + ' · ' + a.error,
    }
  }
  return { label: t('network.layer.anchor'), ok: false, detail: a.error }
}

function cloudLine() {
  const c = cloudApi.value
  if (!c) return { label: t('network.layer.cloudApi'), ok: false, detail: t('network.statusKind.unknown') }
  if (c.kind === 'not_configured') {
    return { label: t('network.layer.cloudApi'), ok: null, detail: t('network.statusKind.notConfigured') }
  }
  if (c.kind === 'reachable') {
    const lat = t('network.latencyMs', { n: c.latency_ms })
    // 4xx:server 通但業務不可用(401/403 = 未登入;404 等其他)
    if (c.status >= 400) {
      const hintKey = (c.status === 401 || c.status === 403)
        ? 'network.statusKind.unauthorized'
        : 'network.statusKind.businessError'
      return {
        label: t('network.layer.cloudApi'),
        ok: 'warn',
        detail: `${t(hintKey)} · HTTP ${c.status} · ${lat}`,
      }
    }
    return {
      label: t('network.layer.cloudApi'),
      ok: true,
      detail: `HTTP ${c.status} · ${lat}`,
    }
  }
  // unreachable but in buffer
  if (cloudEffectiveOk.value !== false) {
    return {
      label: t('network.layer.cloudApi'),
      ok: 'retry',
      detail: t('network.retryingStreak', { n: cloudFailStreak.value, total: failThreshold.value }) + ' · ' + c.error,
    }
  }
  return { label: t('network.layer.cloudApi'), ok: false, detail: c.error }
}

const lines = computed(() => [osLine(), anchorLine(), cloudLine()])
</script>

<template>
  <VMenu offset="8" :close-on-content-click="false">
    <template #activator="{ props: actv }">
      <VBtn
        v-bind="actv"
        icon
        variant="text"
        :color="overallColor"
        density="comfortable"
        :aria-label="overallTooltip"
      >
        <VIcon :icon="overallIcon" size="22" />
      </VBtn>
    </template>
    <VCard min-width="280" class="network-popup">
      <VCardItem class="pb-2">
        <VCardTitle class="text-body-large font-weight-medium d-flex align-center">
          <VIcon :icon="overallIcon" :color="overallColor" size="20" class="me-2" />
          {{ overallTooltip }}
        </VCardTitle>
      </VCardItem>
      <VDivider />
      <VList density="compact" class="py-1">
        <VListItem v-for="ln in lines" :key="ln.label">
          <template #prepend>
            <VIcon
              v-if="ln.ok === true"
              icon="tabler-circle-check"
              color="success"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else-if="ln.ok === 'retry'"
              icon="tabler-refresh-alert"
              color="warning"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else-if="ln.ok === 'warn'"
              icon="tabler-alert-circle"
              color="warning"
              size="18"
              class="me-2"
            />
            <VIcon
              v-else-if="ln.ok === false"
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
          <VListItemTitle class="text-body-medium">{{ ln.label }}</VListItemTitle>
          <VListItemSubtitle class="text-body-small">{{ ln.detail }}</VListItemSubtitle>
        </VListItem>
      </VList>
      <VDivider />
      <div class="pa-2 d-flex align-center">
        <div class="flex-grow-1">
          <div class="text-body-small text-medium-emphasis">
            {{ $t('network.lastCheckedAt') }} {{ lastCheckedText }}
          </div>
          <div class="text-body-small text-disabled">
            {{ $t('network.nextCheckIn', { n: effectiveIntervalSecs }) }}
          </div>
        </div>
        <VBtn
          size="small"
          variant="text"
          color="primary"
          :loading="isChecking"
          @click="checkNow"
        >
          <VIcon icon="tabler-refresh" size="16" class="me-1" />
          {{ $t('network.checkNow') }}
        </VBtn>
      </div>
    </VCard>
  </VMenu>
</template>
