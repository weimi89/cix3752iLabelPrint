<script setup>
import { toast } from 'vue3-toastify'
import { cloudExaminePackage, cloudFetchCloudPrint, printImage } from '@/api/tauri'
import { playSound } from '@/composables/useSoundEffects'
import AppHeader from '@/components/AppHeader.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const STORAGE_KEY = 'cix3752iLabelPrint.printerMap'

const printerMap = computed(() => {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  } catch {
    return {}
  }
})

// 物流商下拉只列出有設印表機的(避免選了卻無法出單;沒設就要先去設)
const ALL_PROVIDER_ITEMS = computed(() => [
  { value: '7', title: t('provider.7eleven') },
  { value: 'F', title: t('provider.family') },
  { value: 'O', title: t('provider.hilife') },
  { value: 'C', title: t('provider.tcat') },
  { value: 'H', title: t('provider.hct') },
  { value: 'P', title: t('provider.pelican') },
  { value: 'E', title: t('provider.sf') },
  { value: 'S', title: t('provider.shopeeOffline') },
  { value: 'A', title: t('provider.shopeeAuth') },
])
const PRINT_TYPE_OPTIONS = computed(() =>
  ALL_PROVIDER_ITEMS.value.filter(p => printerMap.value[p.value]),
)

// 操作人員 / 貼單人員 / 列印類型記在本機 — 操作人員 / 貼單人員與 ScanPrintPage 共用 key
const SCANNER_USER_KEY = 'cix3752iLabelPrint.scannerUser'
const STICKER_USER_KEY = 'cix3752iLabelPrint.stickerUser'
const PRINT_TYPE_KEY = 'cix3752iLabelPrint.autoPrintType'
const PRINT_TYPE_MULTIPLE_KEY = 'cix3752iLabelPrint.autoPrintTypeMultiple'

// 讀取舊版單值字串 / 新版 JSON array,兩種格式都吃,讓既有使用者升級不掉資料
const loadStoredPrintTypes = () => {
  const raw = localStorage.getItem(PRINT_TYPE_KEY)
  if (!raw) return []
  try {
    const parsed = JSON.parse(raw)
    if (Array.isArray(parsed)) return parsed.filter(v => typeof v === 'string' && v)
    if (typeof parsed === 'string' && parsed) return [parsed]
  } catch {
    // 舊版直接存單字串(非 JSON),raw 本身就是值
    return [raw]
  }
  return []
}

// 列印範圍是否複選(預設單選,跟 ScanPrintPage / cix3752iWeb 對齊)
const printTypeMultiple = ref(localStorage.getItem(PRINT_TYPE_MULTIPLE_KEY) === '1')
const initialPrintTypes = loadStoredPrintTypes()
if (!printTypeMultiple.value && initialPrintTypes.length > 1) {
  initialPrintTypes.splice(1)
}

const form = reactive({
  shipment_no: '',
  order_sn: '',
  package_sn: '',
  // 由下方 immediate watch 校正:讀回的值會過濾掉不在當前 options 內(該物流商 printer 後來被刪)的項目
  print_types: initialPrintTypes,
  scanner_user: localStorage.getItem(SCANNER_USER_KEY) || '',
  sticker_user: localStorage.getItem(STICKER_USER_KEY) || '',
  enforce: false,
})
watch(() => form.scanner_user, v => localStorage.setItem(SCANNER_USER_KEY, v || ''))
watch(() => form.sticker_user, v => localStorage.setItem(STICKER_USER_KEY, v || ''))
watch(() => form.print_types, v => localStorage.setItem(PRINT_TYPE_KEY, JSON.stringify(v || [])), { deep: true })
watch(printTypeMultiple, v => {
  localStorage.setItem(PRINT_TYPE_MULTIPLE_KEY, v ? '1' : '0')
  // 切回單選時,若已勾多筆只保留第一筆
  if (!v && form.print_types.length > 1) {
    form.print_types = [form.print_types[0]]
  }
})

// checkbox 點擊:單選 = 替換、複選 = toggle 加入/移除
const togglePrintType = (value, checked) => {
  if (printTypeMultiple.value) {
    if (checked) {
      if (!form.print_types.includes(value)) {
        form.print_types = [...form.print_types, value]
      }
    } else {
      form.print_types = form.print_types.filter(v => v !== value)
    }
  } else {
    form.print_types = checked ? [value] : []
  }
}

// 過濾掉印表機被刪掉而失效的選項;首次載入全空 + 有可用選項時:複選預設全勾、單選預設第一個
watch(PRINT_TYPE_OPTIONS, opts => {
  const valid = new Set(opts.map(o => o.value))
  const filtered = form.print_types.filter(v => valid.has(v))
  if (filtered.length === 0 && form.print_types.length === 0 && opts.length > 0) {
    form.print_types = printTypeMultiple.value ? opts.map(o => o.value) : [opts[0].value]
  } else if (filtered.length !== form.print_types.length) {
    form.print_types = filtered
  }
}, { immediate: true })

const packageOrders = ref([])
const shipmentNoRef = ref(null)
const orderSnRef = ref(null)

// 包裹條碼掃描 → 取訂單清單
const handleExaminePackage = async () => {
  const value = form.shipment_no.trim()
  if (!value) return
  try {
    const data = await cloudExaminePackage(value)
    if (data?.respond_code === 'FIND-PACKAGE-ORDER') {
      form.package_sn = data.package_sn || ''
      packageOrders.value = data.orders || []
      form.shipment_no = ''
      playSound('effect_1')
      toast(t('page.auto.toast.packageLoaded', { sn: data.package_sn, n: data.orders?.length || 0 }), { type: 'success' })
      nextTick(() => orderSnRef.value?.focus())
    } else if (data?.respond_code === 'NO-PACKAGE-DATA') {
      playSound('effect_2')
      toast(data.respond_message || t('common.noResults'), { type: 'error' })
      packageOrders.value = []
      form.package_sn = ''
    } else {
      playSound('effect_2')
      toast(t('page.auto.toast.unknownRespond', { code: data?.respond_code || t('page.auto.noCode'), msg: data?.respond_message || '' }), { type: 'error' })
    }
  } catch (e) {
    playSound('effect_2')
    toast(String(e), { type: 'error' })
  }
}

// 訂單編號掃描 → 後端產圖 + 原生列印
const handlePrintSubmit = async () => {
  const orderSn = form.order_sn.trim()
  if (!orderSn) return
  if (!form.scanner_user.trim()) { toast(t('page.scan.errScannerUserRequired'), { type: 'error' }); return }
  if (!form.sticker_user.trim()) { toast(t('page.scan.errStickerUserRequired'), { type: 'error' }); return }
  if (!form.print_types.length) { playSound('effect_2'); toast(t('page.auto.toast.errPrintTypeRequired'), { type: 'error' }); return }

  try {
    const data = await cloudFetchCloudPrint(orderSn, {
      printTypes: form.print_types,
      enforce: form.enforce,
      packageSn: form.package_sn || '',
      scannerUser: form.scanner_user || '',
      stickerUser: form.sticker_user || '',
    })
    form.order_sn = ''
    nextTick(() => orderSnRef.value?.focus())

    switch (data?.respond_code) {
      case 'PRINT-SUCCESS': {
        const printer = printerMap.value[data.provider_code]
        if (!printer) {
          playSound('effect_2')
          toast(t('page.auto.toast.noPrinterForProvider', { code: data.provider_code }), { type: 'error' })
          return
        }
        const path = data.image_path?.startsWith('file://') ? data.image_path.slice(7) : data.image_path
        try {
          await printImage({ printerName: printer, imagePath: path })
          playSound('effect_1')
          toast(t('page.auto.toast.printed', { sn: data.shipment_no || orderSn }), { type: 'success' })
        } catch (e) {
          playSound('effect_2')
          toast(t('page.auto.toast.printFailed', { reason: String(e) }), { type: 'error' })
          return
        }
        const pad = n => String(n).padStart(2, '0')
        const d = new Date()
        const nowStr = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
        packageOrders.value = packageOrders.value.map(o =>
          o.shipping_no === data.shipment_no ? { ...o, _printed: true, last_print_time: nowStr } : o,
        )
        break
      }
      case 'NO-DATA': playSound('effect_2'); toast(t('page.auto.toast.noData', { sn: orderSn }), { type: 'error' }); break
      case 'PRINT-ERROR': playSound('effect_2'); toast(t('page.auto.toast.printError', { sn: orderSn }), { type: 'error' }); break
      case 'UNCONFIRMED-SHIPMENT': playSound('effect_4'); toast(t('page.auto.toast.unconfirmed', { sn: orderSn }), { type: 'warning' }); break
      case 'ERROR-SHIPMENT': playSound('effect_2'); toast(t('page.auto.toast.errorShipment', { sn: orderSn }) + (data.respond_message ? `\n${data.respond_message}` : ''), { type: 'error' }); break
      case 'ABNORMAL-SHIPMENT': playSound('effect_2'); toast(t('page.auto.toast.abnormalShipment', { sn: data.shipment_no || orderSn }), { type: 'info' }); break
      case 'ABNORMAL-PACKAGE': playSound('effect_2'); toast(t('page.auto.toast.abnormalPackage', { sn: data.package_sn || orderSn }), { type: 'warning' }); break
      case 'WRAPPER-ERROR': playSound('effect_2'); toast(t('page.auto.toast.wrapperError', { sn: data.shipment_no || orderSn }), { type: 'warning' }); break
      case 'STORE-CLOSED': playSound('effect_3'); toast(t('page.auto.toast.storeClosed', { sn: data.shipment_no || orderSn }), { type: 'warning' }); break
      default: playSound('effect_2'); toast(t('page.auto.toast.unknownRespond', { code: data?.respond_code || t('page.auto.noCode'), msg: data?.respond_message || '' }), { type: 'error' })
    }
  } catch (e) {
    playSound('effect_2')
    toast(String(e), { type: 'error' })
  }
}

const isAnyPrinterReady = computed(() => Object.keys(printerMap.value).length > 0)

onMounted(() => shipmentNoRef.value?.focus())
</script>

<template>
  <div>
    <AppHeader :title="$t('page.auto.title')" :subtitle="$t('page.auto.subtitle')" icon="tabler-cloud-cog">
      <template #actions>
        <VSwitch
          v-model="form.enforce"
          :label="$t('page.scan.enforce')"
          color="warning"
          inset
          hide-details
          density="compact"
        />
      </template>
    </AppHeader>

    <VAlert v-if="!isAnyPrinterReady" type="error" variant="tonal" class="mb-3">
      {{ $t('page.auto.noPrinterAlert') }}
    </VAlert>

    <VRow>
      <VCol cols="12" lg="5">
        <VCard>
          <VCardTitle class="d-flex align-center px-4 py-2 bg-grey-300 multi-switch-row" style="min-height: 58px;">
            <VIcon size="22" icon="tabler-printer" class="me-2" />
            <span>{{ $t('page.scan.printLabelTitle') }}</span>
            <VSpacer />
            <VSwitch
              v-model="printTypeMultiple"
              color="primary"
              density="compact"
              hide-details
              inset
            />
            <span class="text-body-1 ms-1 cursor-pointer" @click="printTypeMultiple = !printTypeMultiple">
              {{ $t('page.scan.printScopeMultiple') }}
            </span>
          </VCardTitle>
          <VCardText>
            <div class="d-flex gap-3 my-3">
              <div class="flex-grow-1">
                <VLabel class="mb-1 text-body-2" style="line-height: 15px;">
                  {{ $t('page.scan.scannerUser') }} <span class="text-error ms-1">※</span>
                </VLabel>
                <VTextField v-model="form.scanner_user" />
              </div>
              <div class="flex-grow-1">
                <VLabel class="mb-1 text-body-2" style="line-height: 15px;">
                  {{ $t('page.scan.stickerUser') }} <span class="text-error ms-1">※</span>
                </VLabel>
                <VTextField v-model="form.sticker_user" />
              </div>
            </div>
            <div class="mb-3">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.auto.packageBarcode') }}</VLabel>
              <VTextField
                ref="shipmentNoRef"
                v-model="form.shipment_no"
                autofocus
                clearable
                @keyup.enter="handleExaminePackage"
              />
            </div>
            <div class="mb-3">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.auto.orderSn') }}</VLabel>
              <VTextField
                ref="orderSnRef"
                v-model="form.order_sn"
                clearable
                @keyup.enter="handlePrintSubmit"
              />
            </div>
            <div>
              <VLabel
                class="mb-1 text-body-2"
                style="line-height: 15px;"
                :class="{ 'text-error font-weight-bold': printTypeMultiple }"
              >
                {{ $t('page.auto.printType') }}
              </VLabel>
              <div
                v-if="PRINT_TYPE_OPTIONS.length > 0"
                class="print-type-checkboxes d-flex flex-wrap px-1"
                :class="{ 'print-type-checkboxes--multiple': printTypeMultiple }"
              >
                <VCheckbox
                  v-for="opt in PRINT_TYPE_OPTIONS"
                  :key="opt.value"
                  :model-value="form.print_types.includes(opt.value)"
                  :label="opt.title"
                  hide-details
                  @update:model-value="checked => togglePrintType(opt.value, checked)"
                />
              </div>
              <div v-else class="text-medium-emphasis text-body-2 px-1 py-2">
                {{ $t('common.noPrintersConfigured') }}
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" lg="7">
        <VCard>
          <VCardTitle class="d-flex align-center justify-space-between px-4 pt-4 pb-3 bg-grey-300">
            <span>{{ $t('page.auto.packageSn') }} {{ form.package_sn || '—' }}</span>
            <span class="text-body-2">
              {{ $t('page.auto.totalCount') }}
              <span class="text-primary font-weight-bold text-h5">{{ packageOrders.length }}</span>
            </span>
          </VCardTitle>
          <VCardText class="pa-0">
            <VTable>
              <thead>
                <tr>
                  <th class="text-center">#</th>
                  <th class="text-center">{{ $t('page.auto.col.orderShipping') }}</th>
                  <th class="text-center d-none d-sm-table-cell">{{ $t('page.auto.col.lastPrintTime') }}</th>
                  <th class="text-center d-none d-sm-table-cell">{{ $t('page.auto.col.provider') }}</th>
                </tr>
              </thead>
              <tbody>
                <template v-if="packageOrders.length > 0">
                  <tr
                    v-for="(item, idx) in packageOrders"
                    :key="item.order_sn"
                    :class="{
                      'row-printed-now': item._printed,
                      'row-printed-before': !item._printed && item.last_print_time,
                      'bg-error-tint': item.is_abnormal,
                    }"
                  >
                    <td class="text-center">{{ idx + 1 }}</td>
                    <td class="text-center">
                      <div class="px-1 py-2 d-flex flex-column gap-1">
                        <span class="text-blue-600">{{ item.order_sn }}</span>
                        <template v-if="item.order_sn !== item.shipping_no">
                          <VDivider class="my-1" />
                          <span>{{ item.shipping_no }}</span>
                        </template>
                        <div class="d-block d-sm-none text-body-2 text-medium-emphasis">
                          {{ item.provider_name }} · {{ item.last_print_time || '-' }}
                        </div>
                      </div>
                    </td>
                    <td class="text-center d-none d-sm-table-cell">
                      <div class="d-flex align-center justify-center gap-1">
                        <VIcon v-if="item._printed" icon="tabler-circle-check" color="info" size="18" />
                        <VIcon v-else-if="item.last_print_time" icon="tabler-history" color="warning" size="18" />
                        <span>{{ item.last_print_time || '-' }}</span>
                      </div>
                    </td>
                    <td class="text-center d-none d-sm-table-cell">{{ item.provider_name }}</td>
                  </tr>
                </template>
                <template v-else>
                  <tr>
                    <td :colspan="4">
                      <div class="py-2 d-flex align-center justify-center">
                        <VIcon icon="tabler-alert-circle" size="20" class="me-1" />
                        <span class="text-md">{{ $t('page.auto.noScannedPackage') }}</span>
                      </div>
                    </td>
                  </tr>
                </template>
              </tbody>
            </VTable>
          </VCardText>
        </VCard>
      </VCol>

    </VRow>
  </div>
</template>

<style scoped>
.border-b {
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
}

/* 本次 session 剛列印成功:淡藍背景 + ✓ icon,清爽表達「剛完成」 */
.row-printed-now {
  background-color: rgba(var(--v-theme-info), 0.1);
}
/* 載入時 already 有 last_print_time:淡黃背景 + 歷史 icon,提醒「之前列印過,可能要 enforce 重印」 */
.row-printed-before {
  background-color: rgba(var(--v-theme-warning), 0.08);
}

/* card-title 內的「列印範圍複選」switch:整體 scale 不破壞 Vuetify 內部結構 */
.multi-switch-row :deep(.v-switch) {
  flex: 0 0 auto;
  transform: scale(0.7);
  transform-origin: right center;
}

/* 列印類型 checkbox 加大 + 拉開間距: Vuetify 沒有 column-gap utility,直接寫死 */
.print-type-checkboxes {
  gap: 6px 12px;
  border: 1px dashed transparent;
  border-radius: 6px;
  padding: 4px 8px;
  transition: border-color 0.15s, background-color 0.15s;
}
.print-type-checkboxes--multiple {
  border-color: rgba(var(--v-theme-error), 0.5);
  background-color: rgba(var(--v-theme-error), 0.04);
}
.print-type-checkboxes :deep(.v-selection-control) {
  min-height: 40px;
}
.print-type-checkboxes :deep(.v-selection-control__wrapper) {
  width: 36px;
  height: 36px;
}
.print-type-checkboxes :deep(.v-selection-control__input) {
  width: 36px;
  height: 36px;
}
.print-type-checkboxes :deep(.v-selection-control__input .v-icon) {
  font-size: 28px;
}
.print-type-checkboxes :deep(.v-label) {
  font-size: 16px;
  opacity: 1;
}
</style>
