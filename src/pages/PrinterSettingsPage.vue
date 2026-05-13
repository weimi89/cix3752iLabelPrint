<script setup>
import { listPrinters } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const PROVIDERS = [
  { code: '7', name: '7-ELEVEN' },
  { code: 'F', name: '全家' },
  { code: 'O', name: '萊爾富' },
  { code: 'C', name: '黑貓' },
  { code: 'H', name: '新竹' },
  { code: 'P', name: '宅配通' },
  { code: 'E', name: '順豐速運' },
  { code: 'S', name: '蝦皮（離線）' },
  { code: 'A', name: '蝦皮（授權）' },
]

const STORAGE_KEY = 'cix3752iLabelPrint.printerMap'

const printers = ref([])
const map = reactive(JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}'))
const errorMsg = ref('')

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const refresh = async () => {
  if (!isTauriRuntime) {
    errorMsg.value = '此頁需在桌面 App 內開啟（瀏覽器無法存取本機印表機）'
    return
  }
  try {
    printers.value = await listPrinters()
    errorMsg.value = ''
  } catch (e) {
    errorMsg.value = `無法載入印表機：${String(e?.message || e)}`
  }
}
onMounted(refresh)

const persist = () => localStorage.setItem(STORAGE_KEY, JSON.stringify(map))

const reset = () => {
  for (const p of PROVIDERS) delete map[p.code]
  persist()
}
</script>

<template>
  <div>
    <AppHeader title="印表機設定" subtitle="設定每個物流商對應的本機印表機" icon="tabler-printer">
      <template #actions>
        <VBtn variant="tonal" @click="refresh">
          <VIcon icon="tabler-refresh" class="me-1" />重新載入
        </VBtn>
        <VBtn color="error" variant="tonal" @click="reset">
          <VIcon icon="tabler-restore" class="me-1" />重置
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-else-if="printers.length === 0" type="warning" variant="tonal" class="mb-3">
      尚未偵測到本機印表機；請檢查印表機是否已連線並安裝驅動。
    </VAlert>

    <VRow>
      <VCol v-for="p in PROVIDERS" :key="p.code" cols="12" md="6">
        <VCard>
          <VCardText>
            <div class="text-subtitle-1 mb-2">{{ p.name }}</div>
            <VSelect
              v-model="map[p.code]"
              :items="printers"
              item-title="name"
              item-value="system_name"
              label="選擇本機印表機"
              clearable
              @update:model-value="persist"
            />
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>
