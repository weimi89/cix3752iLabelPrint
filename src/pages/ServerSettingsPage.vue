<script setup>
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { getConfig, updateConfig, serverRestart, serverStatus } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const config = ref(null)
const status = ref({ running: false, bind_addr: '' })
const saving = ref(false)
const restarting = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const load = async () => {
  config.value = await getConfig()
  status.value = await serverStatus()
}
onMounted(load)

const save = async () => {
  errorMsg.value = ''
  saving.value = true
  try {
    config.value = await updateConfig(JSON.parse(JSON.stringify(config.value)))
    flashMsg.value = '已儲存（Server 變更需點「重啟 Server」生效）'
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
    flashMsg.value = `Server 已重啟，目前綁定 ${status.value.bind_addr}`
    setTimeout(() => (flashMsg.value = ''), 3000)
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    restarting.value = false
  }
}

const pickCacheDir = async () => {
  const dir = await openDialog({ directory: true, title: '選擇圖片快取目錄' })
  if (typeof dir === 'string' && dir) {
    config.value.cache.dir = dir
  }
}
</script>

<template>
  <div v-if="config">
    <AppHeader title="Server / Cache 設定" subtitle="本地 HTTP 服務與圖片快取行為" icon="tabler-server-2" />

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <VCard class="mb-4">
      <VCardItem>
        <VCardTitle>HTTP Server</VCardTitle>
        <VCardSubtitle>給分揀機工控機呼叫的本地 API</VCardSubtitle>
      </VCardItem>
      <VDivider />
      <VCardText>
        <VTextField
          v-model="config.server.listen_ip"
          label="Listen IP"
          hint="0.0.0.0 對外開放給工控機；127.0.0.1 僅限本機"
          persistent-hint
          class="mb-3"
        />
        <VTextField
          v-model.number="config.server.port"
          label="Port"
          type="number"
          class="mb-3"
        />
        <VSwitch
          v-model="config.server.auto_start"
          label="開機自動啟動 Server"
          color="primary"
          inset
        />
        <div class="d-flex justify-space-between align-center mt-3">
          <span>目前綁定：<code>{{ status.bind_addr }}</code></span>
          <VBtn :loading="restarting" color="warning" variant="tonal" @click="restart">
            <VIcon icon="tabler-restart" class="me-1" />重啟 Server
          </VBtn>
        </div>
      </VCardText>
    </VCard>

    <VCard class="mb-4">
      <VCardItem>
        <VCardTitle>圖片快取</VCardTitle>
        <VCardSubtitle>面單圖檔在本機保留的方式</VCardSubtitle>
      </VCardItem>
      <VDivider />
      <VCardText>
        <VTextField
          v-model="config.cache.dir"
          label="快取目錄"
          placeholder="留白代表使用預設位置（app_data/cache/labels）"
          class="mb-3"
        >
          <template #append>
            <VBtn variant="tonal" size="small" @click="pickCacheDir">
              <VIcon icon="tabler-folder-open" class="me-1" />選擇
            </VBtn>
          </template>
        </VTextField>
        <VTextField
          v-model.number="config.cache.keep_days"
          label="保留天數"
          hint="0 代表永久保留；超過天數的圖檔每小時清理一次"
          persistent-hint
          type="number"
          class="mb-3"
        />
        <VTextField
          v-model.number="config.cache.max_size_mb"
          label="最大容量 (MB)"
          hint="0 代表不限"
          persistent-hint
          type="number"
          class="mb-3"
        />
        <VSwitch
          v-model="config.cache.background_prefetch"
          label="啟用背景補下載（雲端有圖、本地沒圖時自動下載）"
          color="primary"
          inset
        />
      </VCardText>
    </VCard>

    <div class="d-flex justify-end">
      <VBtn :loading="saving" color="primary" size="large" @click="save">
        <VIcon icon="tabler-device-floppy" class="me-1" />儲存設定
      </VBtn>
    </div>
  </div>
</template>
