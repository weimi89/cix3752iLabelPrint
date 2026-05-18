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

const PRINT_TYPE_OPTIONS = computed(() => [
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

const form = reactive({
  shipment_no: '',
  order_sn: '',
  package_sn: '',
  print_type: '7',
  scanner_user: '',
  sticker_user: '',
  enforce: false,
})

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

  try {
    const data = await cloudFetchCloudPrint(orderSn, {
      printType: form.print_type,
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
        packageOrders.value = packageOrders.value.map(o =>
          o.shipping_no === data.shipment_no ? { ...o, _printed: true } : o,
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
        <VCard class="py-1">
          <VCardText>
            <div class="d-flex gap-3 mb-3">
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
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.auto.printType') }}</VLabel>
              <VSelect
                v-model="form.print_type"
                :items="PRINT_TYPE_OPTIONS"
                item-title="title"
                item-value="value"
              />
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
            <div>
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.auto.orderSn') }}</VLabel>
              <VTextField
                ref="orderSnRef"
                v-model="form.order_sn"
                clearable
                @keyup.enter="handlePrintSubmit"
              />
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
                    :class="{ 'opacity-50': item._printed, 'bg-error-tint': item.is_abnormal }"
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
                    <td class="text-center d-none d-sm-table-cell">{{ item.last_print_time || '-' }}</td>
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
</style>
