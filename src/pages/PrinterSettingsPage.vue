<script setup>
import { listPrinters } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const PROVIDERS = [
  { code: '7', nameKey: 'provider.7eleven' },
  { code: 'F', nameKey: 'provider.family' },
  { code: 'O', nameKey: 'provider.hilife' },
  { code: 'C', nameKey: 'provider.tcat' },
  { code: 'H', nameKey: 'provider.hct' },
  { code: 'P', nameKey: 'provider.pelican' },
  { code: 'E', nameKey: 'provider.sf' },
  { code: 'S', nameKey: 'provider.shopeeOffline' },
  { code: 'A', nameKey: 'provider.shopeeAuth' },
]

const STORAGE_KEY = 'cix3752iLabelPrint.printerMap'

const MOCK_PRINTERS = computed(() => [
  { name: t('page.printer.mockPrefix') + ' HP LaserJet Pro M404', system_name: 'mock_hp_laserjet', driver_name: 'HP Universal', is_default: true, state: 'IDLE' },
  { name: t('page.printer.mockPrefix') + ' Brother QL-820NWB', system_name: 'mock_brother_ql820', driver_name: 'Brother QL Series', is_default: false, state: 'IDLE' },
  { name: t('page.printer.mockPrefix') + ' Zebra GK420t', system_name: 'mock_zebra_gk420', driver_name: 'ZDesigner GK420t', is_default: false, state: 'IDLE' },
])

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const printers = ref([])
const map = reactive(JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}'))
const errorMsg = ref('')
const loading = ref(false)

const refresh = async () => {
  if (!isTauriRuntime) {
    printers.value = MOCK_PRINTERS.value
    errorMsg.value = ''
    return
  }
  loading.value = true
  try {
    printers.value = await listPrinters()
    errorMsg.value = ''
  } catch (e) {
    errorMsg.value = t('page.printer.loadFailed', { reason: String(e?.message || e) })
    printers.value = []
  } finally {
    loading.value = false
  }
}
onMounted(refresh)

const persist = () => {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(map))
  // 通知同分頁 ScanPrint/AutoPrint 印表機對照表已變更即時重載(raw setItem 不觸發 storage 事件)
  window.dispatchEvent(new Event('printer-map-updated'))
}

const reset = () => {
  for (const p of PROVIDERS) delete map[p.code]
  persist()
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.printer.title')" :subtitle="$t('page.printer.subtitle')" icon="tabler-printer">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="primary" :loading="loading" :disabled="!isTauriRuntime" @click="refresh">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
          </VBtn>
          <VBtn color="error" @click="reset">
            <VIcon icon="tabler-restore" size="16" class="me-1" />{{ $t('common.reset') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="refresh">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>{{ $t('common.reload') }}</VListItemTitle>
              </VListItem>
              <VListItem @click="reset">
                <template #prepend><VIcon icon="tabler-restore" size="20" /></template>
                <VListItemTitle>{{ $t('common.reset') }}</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      {{ $t('page.printer.browserAlert') }}
    </VAlert>
    <VAlert v-else-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-else-if="printers.length === 0" type="warning" variant="tonal" class="mb-3">
      {{ $t('page.printer.noPrinters') }}
    </VAlert>

    <VRow density="compact">
      <VCol v-for="p in PROVIDERS" :key="p.code" cols="12" md="6" class="py-1">
        <VCard>
          <VCardText>
            <div class="text-subtitle-1 mb-2">{{ $t(p.nameKey) }}</div>
            <VSelect
              v-model="map[p.code]"
              :items="printers"
              item-title="name"
              item-value="system_name"
              :placeholder="$t('page.printer.selectPrinterPlaceholder')"
              variant="outlined"
              density="compact"
              hide-details
              clearable
              @update:model-value="persist"
            />
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>

