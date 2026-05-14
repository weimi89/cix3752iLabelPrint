<script setup>
import { cloudLogin, cloudLogout, cloudSession, getConfig, updateConfig } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const form = reactive({ api_base: '', token: '' })
const session = ref({ logged_in: false, api_base: '', user_label: null })
const config = ref(null)
const loading = ref(false)
const errorMsg = ref('')

const refresh = async () => {
  session.value = await cloudSession()
  config.value = await getConfig()
  if (config.value?.cloud?.api_base && !form.api_base) {
    form.api_base = config.value.cloud.api_base
  }
}

onMounted(refresh)

const handleLogin = async () => {
  errorMsg.value = ''
  loading.value = true
  try {
    const cfg = JSON.parse(JSON.stringify(config.value))
    cfg.cloud.api_base = form.api_base.trim()
    await updateConfig(cfg)
    session.value = await cloudLogin(form.api_base.trim(), form.token.trim())
    form.token = ''
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

        <div class="mb-3">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">API Base URL</VLabel>
          <VTextField
            v-model="form.api_base"
            placeholder="https://your-domain.example.com"
            prepend-inner-icon="tabler-world"
            hide-details
          />
        </div>

        <div>
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">Personal Access Token</VLabel>
          <VTextField
            v-model="form.token"
            type="password"
            placeholder="Bearer Token"
            prepend-inner-icon="tabler-key"
            hide-details
          />
        </div>
      </VCardText>
    </VCard>

    <div class="d-flex justify-center mt-4">
      <VBtn color="primary" size="large" :loading="loading" :disabled="!form.api_base || !form.token" @click="handleLogin">
        <VIcon icon="tabler-login" size="18" class="me-2" />登入並驗證
      </VBtn>
    </div>
  </div>
</template>

