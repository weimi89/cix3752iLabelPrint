<script setup>
import { getConfig, listPrinters, updateConfig } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { hasBackend } from '@/api/runtime'

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


const printers = ref([])
const map = reactive(JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}'))
const errorMsg = ref('')
const loading = ref(false)

const refresh = async () => {
  if (!hasBackend) {
    printers.value = MOCK_PRINTERS.value
    errorMsg.value = ''
    return
  }
  loading.value = true
  try {
    printers.value = await listPrinters()
    errorMsg.value = ''
  } catch (e) {
    errorMsg.value = t('page.printer.loadFailed', { reason: errorMessageFromException(e) })
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

// 分揀機提示面單印表機(存於 AppConfig.error_label.printer_name,空字串＝不指定),存在中介機設定、
// 不是這台瀏覽器:中介機直印時,異常件要去的格口沒有印表機就印在這台,不指定就印在電腦的預設印表機。
// 上方「重設」只清各物流的對照,不動這一項
const errorLabelPrinter = ref('')
const savingErrorLabelPrinter = ref(false)

const loadErrorLabelPrinter = async () => {
  if (!hasBackend) return
  try {
    const cfg = await getConfig()
    errorLabelPrinter.value = cfg?.error_label?.printer_name || ''
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  }
}
onMounted(loadErrorLabelPrinter)

// 重抓 config 再寫回,避免蓋掉其他頁的併發變更;熱套用即時生效
const saveErrorLabelPrinter = async val => {
  const name = val || ''
  const previous = errorLabelPrinter.value
  errorLabelPrinter.value = name
  if (!hasBackend) return
  savingErrorLabelPrinter.value = true
  try {
    const cfg = await getConfig()
    cfg.error_label = { ...(cfg.error_label || {}), printer_name: name }
    await updateConfig(cfg)
    toast.success(t('page.printer.errorLabelSaved'))
  } catch (e) {
    errorLabelPrinter.value = previous
    errorMsg.value = errorMessageFromException(e)
  } finally {
    savingErrorLabelPrinter.value = false
  }
}

const reset = () => {
  for (const p of PROVIDERS) delete map[p.code]
  persist()
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.printer.title')" :subtitle="$t('page.printer.subtitle')" :subtitle-short="$t('page.printer.subtitleShort')" icon="tabler-printer">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="primary" :loading="loading" :disabled="!hasBackend" @click="refresh">
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
              <VListItem :disabled="!hasBackend" @click="refresh">
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

    <VAlert v-if="!hasBackend" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      {{ $t('page.printer.browserAlert') }}
    </VAlert>
    <VAlert v-else-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-else-if="printers.length === 0" type="warning" variant="tonal" class="mb-3">
      {{ $t('page.printer.noPrinters') }}
    </VAlert>

    <VCard class="mb-3">
      <VCardText>
        <div class="d-flex align-center ga-2 text-subtitle-1 mb-1">
          <VIcon icon="tabler-file-alert" size="18" color="warning" />
          {{ $t('page.printer.errorLabelTitle') }}
        </div>
        <div class="text-body-small text-medium-emphasis mb-3">{{ $t('page.printer.errorLabelBrief') }}</div>
        <VSelect
          :model-value="errorLabelPrinter || null"
          :items="printers"
          item-title="name"
          item-value="name"
          :placeholder="$t('page.printer.errorLabelPlaceholder')"
          :loading="savingErrorLabelPrinter"
          variant="outlined"
          density="compact"
          hide-details
          clearable
          style="max-inline-size: 480px;"
          @update:model-value="saveErrorLabelPrinter"
        />
      </VCardText>
    </VCard>

    <div class="text-body-medium font-weight-medium mb-2">{{ $t('page.printer.providerSection') }}</div>

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

