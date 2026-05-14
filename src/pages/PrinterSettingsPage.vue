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

// 瀏覽器模式下用的假印表機,只為 UI 測試,實機才會看到真實印表機
const MOCK_PRINTERS = [
  { name: '(瀏覽器模式) HP LaserJet Pro M404', system_name: 'mock_hp_laserjet', driver_name: 'HP Universal', is_default: true, state: 'IDLE' },
  { name: '(瀏覽器模式) Brother QL-820NWB', system_name: 'mock_brother_ql820', driver_name: 'Brother QL Series', is_default: false, state: 'IDLE' },
  { name: '(瀏覽器模式) Zebra GK420t', system_name: 'mock_zebra_gk420', driver_name: 'ZDesigner GK420t', is_default: false, state: 'IDLE' },
]

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const printers = ref([])
const map = reactive(JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}'))
const errorMsg = ref('')
const loading = ref(false)

const refresh = async () => {
  if (!isTauriRuntime) {
    printers.value = MOCK_PRINTERS
    errorMsg.value = ''
    return
  }
  loading.value = true
  try {
    printers.value = await listPrinters()
    errorMsg.value = ''
  } catch (e) {
    errorMsg.value = `無法載入印表機:${String(e?.message || e)}`
    printers.value = []
  } finally {
    loading.value = false
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
        <div class="d-none d-md-flex ga-2">
          <VBtn color="primary" :loading="loading" :disabled="!isTauriRuntime" @click="refresh">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />重新載入
          </VBtn>
          <VBtn color="error" @click="reset">
            <VIcon icon="tabler-restore" size="16" class="me-1" />重置
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="refresh">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>重新載入</VListItemTitle>
              </VListItem>
              <VListItem @click="reset">
                <template #prepend><VIcon icon="tabler-restore" size="20" /></template>
                <VListItemTitle>重置</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      目前為瀏覽器預覽模式,顯示假印表機資料以供 UI 測試。實機請於桌面 App 內開啟,系統會自動列出本機已安裝的印表機。
    </VAlert>
    <VAlert v-else-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-else-if="printers.length === 0" type="warning" variant="tonal" class="mb-3">
      尚未偵測到本機印表機;請檢查印表機是否已連線並安裝驅動。
    </VAlert>

    <VRow dense>
      <VCol v-for="p in PROVIDERS" :key="p.code" cols="12" md="6" class="py-1">
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

