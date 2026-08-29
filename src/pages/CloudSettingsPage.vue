<script setup>
import { cloudLogin, cloudLogout, cloudSession, getConfig, updateConfig } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import SoundSettingsDialog from '@/components/SoundSettingsDialog.vue'
import { usePrintAlertSoundSettings } from '@/composables/usePrintAlertSoundSettings'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

// 提示音設定:工控機查件失敗(門市關轉/未確認/查無/一般失敗)由雲端 API 回應觸發,
// 在此設定與自動印單頁共用同一組全域 effect_*(單一 localStorage 來源,改一處全域生效)
const {
  events: printSoundEvents,
  defaults: printSoundDefaults,
  soundSettings,
  isSoundSettingsDialogVisible,
  handleSoundSettingsSave,
} = usePrintAlertSoundSettings()

const form = reactive({
  api_base: '',
  token: '',
  job_user: '物流貓',
  allow_invalid_certs: false,
  parcel_mode: 'forward',
  parcel_forward_path: '/api/v2/order-forward-print',
  parcel_proxy_path: '/api/v2/order-proxy-print',
  session_path: '/api/v1/local-middleware/session',
  scan_print_path: '/api/v1/local-middleware/label/scan-print',
  pre_generate_path: '/api/v1/local-middleware/label/pre-generate',
  cloud_print_path: '/api/v1/local-middleware/label/cloud-print',
  examine_package_path: '/api/v1/local-middleware/label/examine-package',
  webhook_path: '/webhook/logistic-cat',
})
const PARCEL_MODES = computed(() => [
  { title: t('page.cloud.parcelMode.forward'), value: 'forward' },
  { title: t('page.cloud.parcelMode.proxy'), value: 'proxy' },
])
const PATH_FIELDS = [
  { key: 'parcel_forward_path', labelKey: 'page.cloud.paths.parcelForward.label', hintKey: 'page.cloud.paths.parcelForward.hint' },
  { key: 'parcel_proxy_path', labelKey: 'page.cloud.paths.parcelProxy.label', hintKey: 'page.cloud.paths.parcelProxy.hint' },
  { key: 'session_path', labelKey: 'page.cloud.paths.session.label', hintKey: 'page.cloud.paths.session.hint' },
  { key: 'scan_print_path', labelKey: 'page.cloud.paths.scanPrint.label', hintKey: 'page.cloud.paths.scanPrint.hint' },
  { key: 'pre_generate_path', labelKey: 'page.cloud.paths.preGenerate.label', hintKey: 'page.cloud.paths.preGenerate.hint' },
  { key: 'cloud_print_path', labelKey: 'page.cloud.paths.cloudPrint.label', hintKey: 'page.cloud.paths.cloudPrint.hint' },
  { key: 'examine_package_path', labelKey: 'page.cloud.paths.examinePackage.label', hintKey: 'page.cloud.paths.examinePackage.hint' },
  { key: 'webhook_path', labelKey: 'page.cloud.paths.webhook.label', hintKey: 'page.cloud.paths.webhook.hint' },
]
const session = ref({ logged_in: false, api_base: '', user_label: null })
const config = ref(null)
const loading = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const refresh = async () => {
  session.value = await cloudSession()
  config.value = await getConfig()
  const c = config.value?.cloud
  if (c?.api_base && !form.api_base) form.api_base = c.api_base
  if (c?.job_user) form.job_user = c.job_user
  if (typeof c?.allow_invalid_certs === 'boolean') form.allow_invalid_certs = c.allow_invalid_certs
  if (c?.parcel_mode) form.parcel_mode = c.parcel_mode
  for (const f of PATH_FIELDS) {
    if (c?.[f.key]) form[f.key] = c[f.key]
  }
}

onMounted(refresh)

const flash = msg => {
  flashMsg.value = msg
  setTimeout(() => (flashMsg.value = ''), 3000)
}

const writeConfigOnly = async () => {
  const cfg = JSON.parse(JSON.stringify(config.value))
  cfg.cloud.api_base = form.api_base.trim()
  cfg.cloud.job_user = (form.job_user || '').trim() || '物流貓'
  cfg.cloud.allow_invalid_certs = !!form.allow_invalid_certs
  cfg.cloud.parcel_mode = form.parcel_mode === 'proxy' ? 'proxy' : 'forward'
  for (const f of PATH_FIELDS) {
    cfg.cloud[f.key] = (form[f.key] || '').trim()
  }
  await updateConfig(cfg)
}

const handleLogin = async () => {
  errorMsg.value = ''
  loading.value = true
  try {
    await writeConfigOnly()
    session.value = await cloudLogin(form.api_base.trim(), form.token.trim())
    form.token = ''
    flash(t('page.cloud.loginSavedFlash'))
    await refresh()
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    loading.value = false
  }
}

const handleSaveOnly = async () => {
  errorMsg.value = ''
  loading.value = true
  try {
    await writeConfigOnly()
    flash(t('page.cloud.savedFlash'))
    await refresh()
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    loading.value = false
  }
}

const handleLogout = async () => {
  await cloudLogout()
  await refresh()
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.cloud.title')" subtitle="API Base URL + Personal Access Token" icon="tabler-cloud-network">
      <template #actions>
        <VBtn variant="text" color="default" class="d-none d-md-inline-flex" @click="isSoundSettingsDialogVisible = true">
          <VIcon icon="tabler-volume" size="18" class="me-1" />{{ $t('soundSettings.title') }}
        </VBtn>
        <VChip
          v-if="session.logged_in"
          color="success"
          variant="tonal"
          size="small"
        >
          <VIcon icon="tabler-circle-check" size="14" class="me-1" />{{ $t('page.cloud.loggedIn') }}
        </VChip>
        <VChip v-else color="warning" variant="tonal" size="small">
          <VIcon icon="tabler-alert-triangle" size="14" class="me-1" />{{ $t('user.notLoggedIn') }}
        </VChip>
        <VBtn v-if="session.logged_in" color="error" size="small" class="d-none d-md-inline-flex" @click="handleLogout">
          <VIcon icon="tabler-logout" size="16" class="me-1" />{{ $t('user.logout') }}
        </VBtn>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem @click="isSoundSettingsDialogVisible = true">
                <template #prepend><VIcon icon="tabler-volume" size="20" /></template>
                <VListItemTitle>{{ $t('soundSettings.title') }}</VListItemTitle>
              </VListItem>
              <VListItem v-if="session.logged_in" @click="handleLogout">
                <template #prepend><VIcon icon="tabler-logout" size="20" /></template>
                <VListItemTitle>{{ $t('user.logout') }}</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <!-- 提示音設定(全域 effect_*,與自動印單頁共用;工控機查件失敗提示音) -->
    <SoundSettingsDialog
      v-model:is-dialog-visible="isSoundSettingsDialogVisible"
      :sound-events="printSoundEvents"
      :settings="soundSettings"
      :defaults="printSoundDefaults"
      @save="handleSoundSettingsSave"
    />

    <VAlert
      type="info"
      variant="tonal"
      icon="tabler-shield-lock"
      class="mb-3"
      density="compact"
    >
      {{ $t('page.cloud.tokenKeyringInfo') }}
    </VAlert>

    <VCard>
      <VCardText>
        <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
        <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-medium" style="line-height: 15px;">API Base URL</VLabel>
          <VTextField
            v-model="form.api_base"
            placeholder="https://your-domain.example.com"
            prepend-inner-icon="tabler-world"
            hide-details
          />
        </div>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-medium" style="line-height: 15px;">Personal Access Token</VLabel>
          <VTextField
            v-model="form.token"
            type="password"
            placeholder="Bearer Token"
            prepend-inner-icon="tabler-key"
            hide-details
          />
        </div>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-medium" style="line-height: 15px;">{{ $t('page.cloud.jobUserLabel') }}</VLabel>
          <VTextField
            v-model="form.job_user"
            :placeholder="$t('page.cloud.jobUserPlaceholder')"
            prepend-inner-icon="tabler-user-check"
            hide-details
          />
        </div>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-medium" style="line-height: 15px;">{{ $t('page.cloud.parcelModeLabel') }}</VLabel>
          <VSelect
            v-model="form.parcel_mode"
            :items="PARCEL_MODES"
            prepend-inner-icon="tabler-route"
            hide-details
          />
        </div>

        <VDivider class="my-3" />

        <div class="d-flex align-center justify-space-between">
          <div>
            <div class="text-body-large font-weight-medium">{{ $t('page.cloud.skipSslTitle') }}</div>
            <div class="text-body-small text-medium-emphasis">
              {{ $t('page.cloud.skipSslDesc') }}
            </div>
          </div>
          <VSwitch
            v-model="form.allow_invalid_certs"
            hide-details
            color="warning"
            inset
          />
        </div>
      </VCardText>
    </VCard>

    <!-- 進階：各 endpoint path 設定 -->
    <VExpansionPanels class="mt-4 advanced-search">
      <VExpansionPanel>
        <VExpansionPanelTitle class="advanced-search__title">
          {{ $t('page.cloud.advancedPaths') }}
        </VExpansionPanelTitle>
        <VExpansionPanelText>
          <div class="text-body-small text-disabled mb-3">
            {{ $t('page.cloud.advancedPathsHint') }}
          </div>
          <VRow no-gutters class="mx-n2">
            <VCol v-for="f in PATH_FIELDS" :key="f.key" cols="12" md="6" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t(f.labelKey) }}</label>
                <VTextField
                  v-model="form[f.key]"
                  :placeholder="$t(f.hintKey)"
                  density="compact"
                  variant="outlined"
                  hide-details
                />
              </div>
            </VCol>
          </VRow>
        </VExpansionPanelText>
      </VExpansionPanel>
    </VExpansionPanels>

    <div class="d-flex justify-center ga-2 mt-4">
      <VBtn variant="outlined" size="large" :loading="loading" :disabled="!form.api_base" @click="handleSaveOnly">
        <VIcon icon="tabler-device-floppy" size="18" class="me-2" />{{ $t('page.cloud.saveOnly') }}
      </VBtn>
      <VBtn color="primary" size="large" :loading="loading" :disabled="!form.api_base || !form.token" @click="handleLogin">
        <VIcon icon="tabler-login" size="18" class="me-2" />{{ $t('page.cloud.loginAndVerify') }}
      </VBtn>
    </div>
  </div>
</template>

