<script setup>
import { cloudLogin, cloudLogout, cloudSession, getConfig, updateConfig } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const form = reactive({
  api_base: '',
  token: '',
  job_user: '物流貓',
  allow_invalid_certs: false,
  parcel_mode: 'forward',
  parcel_forward_path: '/api/v2/order-forward-print',
  parcel_proxy_path: '/api/v2/order-proxy-print',
  session_path: '/api/v1/local-middleware/session',
  report_path: '/api/v1/local-middleware/report',
  scan_print_path: '/api/v1/local-middleware/label/scan-print',
  pre_generate_path: '/api/v1/local-middleware/label/pre-generate',
  cloud_print_path: '/api/v1/local-middleware/label/cloud-print',
  webhook_path: '/webhook/logistic-cat',
})
const PARCEL_MODES = [
  { title: '轉寄 (forward) — 包裹先到倉庫再轉寄', value: 'forward' },
  { title: '代寄 (proxy) — 賣家交由我方代寄', value: 'proxy' },
]
const PATH_FIELDS = [
  { key: 'parcel_forward_path', label: '包裹查詢 (forward)', hint: '會自動拼 /{queryNo}' },
  { key: 'parcel_proxy_path', label: '包裹查詢 (proxy)', hint: '會自動拼 /{queryNo}' },
  { key: 'session_path', label: '登入 Session', hint: '驗證 token 用' },
  { key: 'report_path', label: '工控機回報 (report)', hint: '推送分揀結果' },
  { key: 'scan_print_path', label: '掃描列印 (scan-print)', hint: '操作員 UI' },
  { key: 'pre_generate_path', label: '面單預產 (pre-generate)', hint: '預先產面單' },
  { key: 'cloud_print_path', label: '雲端列印 (cloud-print)', hint: '透過雲端列印' },
  { key: 'webhook_path', label: 'Webhook (logistic-cat)', hint: '分揀完成通知' },
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
    flash('已登入並儲存設定')
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
    flash('已儲存設定')
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
    <AppHeader title="雲端 API 設定" subtitle="API Base URL + Personal Access Token" icon="tabler-cloud-network">
      <template #actions>
        <VChip
          v-if="session.logged_in"
          color="success"
          variant="tonal"
          size="small"
        >
          <VIcon icon="tabler-circle-check" size="14" class="me-1" />已登入
        </VChip>
        <VChip v-else color="warning" variant="tonal" size="small">
          <VIcon icon="tabler-alert-triangle" size="14" class="me-1" />尚未登入
        </VChip>
        <div v-if="session.logged_in" class="d-none d-md-flex ga-2">
          <VBtn color="error" size="small" @click="handleLogout">
            <VIcon icon="tabler-logout" size="16" class="me-1" />登出
          </VBtn>
        </div>
        <VBtn v-if="session.logged_in" class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem @click="handleLogout">
                <template #prepend><VIcon icon="tabler-logout" size="20" /></template>
                <VListItemTitle>登出</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert
      type="info"
      variant="tonal"
      icon="tabler-shield-lock"
      class="mb-3"
      density="compact"
    >
      Token 會儲存到系統 keyring(macOS Keychain / Windows Credential Vault),不會寫入 config 檔。
    </VAlert>

    <VCard>
      <VCardText>
        <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
        <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">API Base URL</VLabel>
          <VTextField
            v-model="form.api_base"
            placeholder="https://your-domain.example.com"
            prepend-inner-icon="tabler-world"
            hide-details
          />
        </div>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">Personal Access Token</VLabel>
          <VTextField
            v-model="form.token"
            type="password"
            placeholder="Bearer Token"
            prepend-inner-icon="tabler-key"
            hide-details
          />
        </div>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">Webhook job_user(識別這台機器)</VLabel>
          <VTextField
            v-model="form.job_user"
            placeholder="例: 物流貓"
            prepend-inner-icon="tabler-user-check"
            hide-details
          />
        </div>

        <div class="mb-3">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">包裹查詢模式</VLabel>
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
            <div class="text-body-1 font-weight-medium">跳過 SSL 憑證驗證</div>
            <div class="text-caption text-medium-emphasis">
              內網 / 開發環境 (.dev、自簽憑證) 必須打開，否則 reqwest 會直接拒絕連線
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
          進階：API 路徑設定
        </VExpansionPanelTitle>
        <VExpansionPanelText>
          <div class="text-caption text-disabled mb-3">
            api_base + 以下 path 拼成完整 URL。Path 開頭可以有沒有 「/」 都行。
          </div>
          <VRow no-gutters class="mx-n2">
            <VCol v-for="f in PATH_FIELDS" :key="f.key" cols="12" md="6" class="px-2 py-1">
              <div class="search-field">
                <label>{{ f.label }}</label>
                <VTextField
                  v-model="form[f.key]"
                  :placeholder="f.hint"
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
        <VIcon icon="tabler-device-floppy" size="18" class="me-2" />僅儲存設定
      </VBtn>
      <VBtn color="primary" size="large" :loading="loading" :disabled="!form.api_base || !form.token" @click="handleLogin">
        <VIcon icon="tabler-login" size="18" class="me-2" />登入並驗證
      </VBtn>
    </div>
  </div>
</template>

