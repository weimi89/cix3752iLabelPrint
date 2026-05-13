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
    // 先把 api_base / timeout / retry 寫回 config
    const cfg = JSON.parse(JSON.stringify(config.value))
    cfg.cloud.api_base = form.api_base.trim()
    await updateConfig(cfg)
    // 登入會把 token 存到系統 keyring
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
    <AppHeader title="雲端 API 設定" subtitle="API Base URL + Personal Access Token" icon="tabler-cloud-network" />

    <VCard class="mb-4">
      <VCardItem>
        <template #prepend>
          <VAvatar :color="session.logged_in ? 'success' : 'warning'" variant="tonal">
            <VIcon :icon="session.logged_in ? 'tabler-circle-check' : 'tabler-alert-triangle'" />
          </VAvatar>
        </template>
        <VCardTitle>{{ session.logged_in ? '已登入' : '尚未登入' }}</VCardTitle>
        <VCardSubtitle>
          <template v-if="session.logged_in">
            {{ session.api_base }} <span v-if="session.user_label" class="ms-2">· {{ session.user_label }}</span>
          </template>
          <template v-else>請輸入 API 位址與 Personal Access Token</template>
        </VCardSubtitle>
        <template #append>
          <VBtn v-if="session.logged_in" color="error" variant="tonal" @click="handleLogout">登出</VBtn>
        </template>
      </VCardItem>
    </VCard>

    <VCard>
      <VCardText>
        <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

        <VTextField
          v-model="form.api_base"
          label="API Base URL"
          placeholder="https://your-domain.example.com"
          prepend-inner-icon="tabler-world"
          class="mb-3"
        />
        <VTextField
          v-model="form.token"
          label="Personal Access Token"
          type="password"
          placeholder="Bearer Token"
          prepend-inner-icon="tabler-key"
          class="mb-3"
        />

        <div class="d-flex justify-end gap-2">
          <VBtn color="primary" :loading="loading" :disabled="!form.api_base || !form.token" @click="handleLogin">
            <VIcon icon="tabler-login" class="me-1" />
            登入並驗證
          </VBtn>
        </div>

        <VDivider class="my-4" />

        <p class="text-caption text-medium-emphasis">
          Token 會儲存到系統 keyring（macOS Keychain / Windows Credential Vault），不會寫入 config 檔。
        </p>
      </VCardText>
    </VCard>
  </div>
</template>
