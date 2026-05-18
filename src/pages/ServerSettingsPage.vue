<script setup>
import { getConfig, updateConfig, serverRestart, serverStatus, setAutoStart, getAutoStart } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import { useLocale } from '@/composables/useLocale'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const { currentLocale, availableLocales, setLocale } = useLocale()

const labelPathModeItems = computed(() => [
  { value: 'local', title: t('label.mode.local') },
  { value: 'share', title: t('label.mode.share') },
  { value: 'http', title: t('label.mode.http') },
])

const config = ref(null)
const status = ref({ running: false, bind_addr: '' })
const osAutoStart = ref(false)
const saving = ref(false)
const restarting = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const load = async () => {
  config.value = await getConfig()
  if (isTauriRuntime) {
    try { status.value = await serverStatus() } catch { /* 後端尚未啟動 */ }
    try { osAutoStart.value = await getAutoStart() } catch { osAutoStart.value = false }
  }
}
onMounted(load)

const save = async () => {
  errorMsg.value = ''
  saving.value = true
  try {
    const previousAutoStart = osAutoStart.value
    config.value = await updateConfig(JSON.parse(JSON.stringify(config.value)))
    // 同步 OS autostart(只在 toggle 變化時呼叫)
    if (isTauriRuntime && previousAutoStart !== config.value.server.auto_start) {
      try {
        await setAutoStart(config.value.server.auto_start)
        osAutoStart.value = config.value.server.auto_start
      } catch (e) {
        errorMsg.value = t('page.server.autoStartFailed', { reason: String(e?.message || e) })
      }
    }
    flashMsg.value = t('page.server.savedFlash')
    setTimeout(() => (flashMsg.value = ''), 3000)
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    saving.value = false
  }
}

const restart = async () => {
  restarting.value = true
  try {
    status.value = await serverRestart()
    flashMsg.value = t('page.server.restartedFlash', { addr: status.value.bind_addr })
    setTimeout(() => (flashMsg.value = ''), 3000)
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    restarting.value = false
  }
}
</script>

<template>
  <div v-if="config">
    <AppHeader :title="$t('page.server.title')" :subtitle="$t('page.server.subtitle')" icon="tabler-server-2">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn :loading="restarting" color="warning" :disabled="!isTauriRuntime" @click="restart">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('page.server.restart') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="restart">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>{{ $t('page.server.restart') }}</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <VCard class="mb-4">
      <VCardText>
        <VRow dense>
          <VCol cols="12" md="8">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.server.listenIp') }}</VLabel>
            <VTextField v-model="config.server.listen_ip" hide-details />
          </VCol>
          <VCol cols="12" md="4">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.server.port') }}</VLabel>
            <VNumberInput v-model="config.server.port" :min="1" :max="65535" />
          </VCol>
        </VRow>

        <VDivider class="my-4" />

        <div class="d-flex align-center justify-space-between">
          <div>
            <div class="text-body-1 font-weight-medium">{{ $t('page.server.autoStartTitle') }}</div>
            <div class="text-caption text-medium-emphasis">{{ $t('page.server.autoStartDesc') }}</div>
          </div>
          <VSwitch
            v-model="config.server.auto_start"
            hide-details
            color="primary"
            inset
          />
        </div>

        <VDivider class="my-4" />

        <div class="text-body-2">
          <span class="text-medium-emphasis">{{ $t('page.server.currentBind') }}</span>
          <code class="ms-2">{{ status.bind_addr || $t('page.dashboard.notStarted') }}</code>
        </div>
      </VCardText>
    </VCard>

    <VCard class="mb-4">
      <VCardItem>
        <VCardTitle class="text-body-1 font-weight-medium">{{ $t('page.server.appSection') }}</VCardTitle>
      </VCardItem>
      <VCardText>
        <div class="d-flex align-center justify-space-between">
          <div>
            <div class="text-body-1 font-weight-medium">{{ $t('locale.label') }}</div>
            <div class="text-caption text-medium-emphasis">{{ $t('page.server.localeDesc') }}</div>
          </div>
          <VSelect
            :model-value="currentLocale"
            :items="availableLocales"
            item-title="label"
            item-value="code"
            hide-details
            density="compact"
            variant="outlined"
            style="max-inline-size: 220px;"
            @update:model-value="setLocale"
          />
        </div>
      </VCardText>
    </VCard>

    <VCard class="mb-4">
      <VCardItem>
        <VCardTitle class="text-body-1 font-weight-medium">{{ $t('network.settings.section') }}</VCardTitle>
        <VCardSubtitle class="text-caption">{{ $t('network.settings.desc') }}</VCardSubtitle>
      </VCardItem>
      <VCardText>
        <VRow dense>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('network.settings.anchorAddr') }}</VLabel>
            <VTextField v-model="config.network.anchor_addr" placeholder="1.1.1.1:443" hide-details density="compact" variant="outlined" />
          </VCol>
          <VCol cols="6" md="3">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('network.settings.intervalSecs') }}</VLabel>
            <VNumberInput v-model="config.network.interval_secs" :min="5" :max="3600" density="compact" />
          </VCol>
          <VCol cols="6" md="3">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('network.settings.degradeIntervalSecs') }}</VLabel>
            <VNumberInput v-model="config.network.degrade_interval_secs" :min="5" :max="3600" density="compact" />
          </VCol>
          <VCol cols="6" md="3">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('network.settings.anchorTimeoutMs') }}</VLabel>
            <VNumberInput v-model="config.network.anchor_timeout_ms" :min="100" :max="10000" :step="100" density="compact" />
          </VCol>
          <VCol cols="6" md="3">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('network.settings.cloudTimeoutSecs') }}</VLabel>
            <VNumberInput v-model="config.network.cloud_timeout_secs" :min="1" :max="60" density="compact" />
          </VCol>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('network.settings.failThreshold') }}</VLabel>
            <VNumberInput v-model="config.network.fail_threshold" :min="1" :max="10" density="compact" />
          </VCol>
        </VRow>
      </VCardText>
    </VCard>

    <VCard class="mb-4">
      <VCardItem>
        <VCardTitle class="text-body-1 font-weight-medium">{{ $t('label.settings.section') }}</VCardTitle>
        <VCardSubtitle class="text-caption">{{ $t('label.settings.desc') }}</VCardSubtitle>
      </VCardItem>
      <VCardText>
        <VRow dense>
          <VCol cols="12" md="4">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('label.settings.mode') }}</VLabel>
            <VSelect
              v-model="config.label_path.mode"
              :items="labelPathModeItems"
              item-title="title"
              item-value="value"
              hide-details
              density="compact"
              variant="outlined"
            />
          </VCol>
          <VCol v-if="config.label_path.mode === 'share'" cols="12" md="8">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('label.settings.shareRoot') }}</VLabel>
            <VTextField
              v-model="config.label_path.share_root"
              :placeholder="$t('label.settings.shareRootPlaceholder')"
              hide-details
              density="compact"
              variant="outlined"
            />
          </VCol>
        </VRow>

        <VAlert
          type="info"
          variant="tonal"
          density="compact"
          class="mt-3"
          :icon="false"
        >
          <div class="text-caption">
            {{
              config.label_path.mode === 'share'
                ? $t('label.settings.shareRootHint')
                : config.label_path.mode === 'http'
                  ? $t('label.settings.httpHint')
                  : $t('label.settings.localHint')
            }}
          </div>
        </VAlert>
      </VCardText>
    </VCard>

    <div class="d-flex justify-center">
      <VBtn :loading="saving" color="primary" size="large" @click="save">
        <VIcon icon="tabler-device-floppy" class="me-1" />{{ $t('common.saveSettings') }}
      </VBtn>
    </div>
  </div>
</template>
