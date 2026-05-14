<script setup>
import { getConfig, updateConfig, serverRestart, serverStatus, setAutoStart, getAutoStart } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

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
        errorMsg.value = `自動啟動切換失敗:${String(e?.message || e)}`
      }
    }
    flashMsg.value = '已儲存(IP/Port 變更需點「重啟 Server」生效)'
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
    flashMsg.value = `Server 已重啟,目前綁定 ${status.value.bind_addr}`
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
    <AppHeader title="Server 設定" subtitle="給分揀機工控機呼叫的本地 HTTP API" icon="tabler-server-2">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn :loading="restarting" color="warning" :disabled="!isTauriRuntime" @click="restart">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />重啟 Server
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="restart">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>重啟 Server</VListItemTitle>
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
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">Listen IP</VLabel>
            <VTextField v-model="config.server.listen_ip" hide-details />
          </VCol>
          <VCol cols="12" md="4">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">Port</VLabel>
            <VTextField v-model.number="config.server.port" type="number" hide-details />
          </VCol>
        </VRow>

        <VDivider class="my-4" />

        <div class="d-flex align-center justify-space-between">
          <div>
            <div class="text-body-1 font-weight-medium">開機自動啟動本程式</div>
            <div class="text-caption text-medium-emphasis">加入作業系統登入啟動清單,含本地 Server</div>
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
          <span class="text-medium-emphasis">目前綁定:</span>
          <code class="ms-2">{{ status.bind_addr || '尚未啟動' }}</code>
        </div>
      </VCardText>
    </VCard>

    <div class="d-flex justify-center">
      <VBtn :loading="saving" color="primary" size="large" @click="save">
        <VIcon icon="tabler-device-floppy" class="me-1" />儲存設定
      </VBtn>
    </div>
  </div>
</template>
