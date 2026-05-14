<script setup>
import { invoke } from '@tauri-apps/api/core'
import { printImage } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const STORAGE_KEY = 'cix3752iLabelPrint.printerMap'

const printerMap = computed(() => {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  } catch {
    return {}
  }
})

const PRINT_TYPE_OPTIONS = [
  { value: '7', title: '7-ELEVEN' },
  { value: 'F', title: '全家' },
  { value: 'O', title: '萊爾富' },
  { value: 'C', title: '黑貓' },
  { value: 'H', title: '新竹' },
  { value: 'P', title: '宅配通' },
  { value: 'E', title: '順豐速運' },
  { value: 'S', title: '蝦皮（離線）' },
  { value: 'A', title: '蝦皮（授權）' },
]

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
const errorMsg = ref('')
const shipmentNoRef = ref(null)
const orderSnRef = ref(null)

// 包裹條碼掃描 → 取訂單清單
const handleExaminePackage = async () => {
  const value = form.shipment_no.trim()
  if (!value) return
  errorMsg.value = ''
  try {
    const data = await invoke('cloud_fetch_label', {
      req: { order_sn: value, print_type: form.print_type, enforce: false, mode: 'cloud_print_examine' },
    })
    // 後端 examine-package 是另一個端點；簡化版直接走 cloud_fetch_label 等待後續整合
    if (data?.respond_code === 'FIND-PACKAGE-ORDER') {
      form.package_sn = data.package_sn || ''
      packageOrders.value = data.orders || []
      form.shipment_no = ''
      nextTick(() => orderSnRef.value?.focus())
    } else if (data?.respond_code === 'NO-PACKAGE-DATA') {
      errorMsg.value = data.respond_message || '查無資料'
      packageOrders.value = []
      form.package_sn = ''
    }
  } catch (e) {
    errorMsg.value = String(e)
  }
}

// 訂單編號掃描 → 後端產圖 + 原生列印
const handlePrintSubmit = async () => {
  const orderSn = form.order_sn.trim()
  if (!orderSn) return
  if (!form.scanner_user.trim()) { errorMsg.value = '未填寫操作人員'; return }
  if (!form.sticker_user.trim()) { errorMsg.value = '未填寫貼單人員'; return }
  errorMsg.value = ''

  try {
    const data = await invoke('cloud_fetch_label', {
      req: {
        order_sn: orderSn,
        print_type: form.print_type,
        enforce: form.enforce,
        mode: 'cloud_print',
      },
    })
    form.order_sn = ''
    nextTick(() => orderSnRef.value?.focus())

    switch (data?.respond_code) {
      case 'PRINT-SUCCESS': {
        const printer = printerMap.value[data.provider_code]
        if (!printer) {
          errorMsg.value = `未設定 ${data.provider_code} 印表機，請至「印表機設定」`
          return
        }
        const path = data.image_path?.startsWith('file://') ? data.image_path.slice(7) : data.image_path
        await printImage({ printerName: printer, imagePath: path })
        packageOrders.value = packageOrders.value.map(o =>
          o.shipping_no === data.shipment_no ? { ...o, _printed: true } : o,
        )
        break
      }
      case 'NO-DATA': errorMsg.value = `查無代寄包裹：${orderSn}`; break
      case 'PRINT-ERROR': errorMsg.value = `無法列印貼標：${orderSn}`; break
      case 'UNCONFIRMED-SHIPMENT': errorMsg.value = `包裹尚未確認：${orderSn}`; break
      case 'ERROR-SHIPMENT': errorMsg.value = `包裹資料異常：${orderSn}`; break
      default: errorMsg.value = data?.respond_message || '未知回應'
    }
  } catch (e) {
    errorMsg.value = String(e)
  }
}

const isAnyPrinterReady = computed(() => Object.keys(printerMap.value).length > 0)

onMounted(() => shipmentNoRef.value?.focus())
</script>

<template>
  <div>
    <AppHeader title="自動印單" subtitle="掃描自動送印" icon="tabler-cloud-cog">
      <template #actions>
        <VSwitch
          v-model="form.enforce"
          label="強制列印"
          color="warning"
          inset
          hide-details
          density="compact"
        />
      </template>
    </AppHeader>

    <VAlert v-if="!isAnyPrinterReady" type="error" variant="tonal" class="mb-3">
      尚未設定任何印表機，請先至「印表機設定」頁完成配置。
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <VRow>
      <VCol cols="12" lg="5">
        <VCard class="py-1">
          <VCardText>
            <div class="mb-3">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">列印類型</VLabel>
              <VSelect
                v-model="form.print_type"
                :items="PRINT_TYPE_OPTIONS"
                item-title="title"
                item-value="value"
              />
            </div>
            <div class="mb-3">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">包裹訂單條碼</VLabel>
              <VTextField
                ref="shipmentNoRef"
                v-model="form.shipment_no"
                autofocus
                clearable
                @keyup.enter="handleExaminePackage"
              />
            </div>
            <div class="mb-3">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">系統訂單編號</VLabel>
              <VTextField
                ref="orderSnRef"
                v-model="form.order_sn"
                clearable
                @keyup.enter="handlePrintSubmit"
              />
            </div>
            <div class="d-flex gap-3">
              <div class="flex-grow-1">
                <VLabel class="mb-1 text-body-2" style="line-height: 15px;">操作人員</VLabel>
                <VTextField v-model="form.scanner_user" />
              </div>
              <div class="flex-grow-1">
                <VLabel class="mb-1 text-body-2" style="line-height: 15px;">貼單人員</VLabel>
                <VTextField v-model="form.sticker_user" />
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" lg="7">
        <VCard>
          <VCardTitle class="d-flex align-center justify-space-between px-4 pt-4 pb-3 bg-grey-300">
            <span>袋號 {{ form.package_sn || '—' }}</span>
            <span class="text-body-2">
              總筆數
              <span class="text-primary font-weight-bold text-h5">{{ packageOrders.length }}</span>
            </span>
          </VCardTitle>
          <VCardText class="pa-0">
            <VTable>
              <thead>
                <tr>
                  <th class="text-center">#</th>
                  <th class="text-center">訂單編號 / 配送單號</th>
                  <th class="text-center d-none d-sm-table-cell">最後列印時間</th>
                  <th class="text-center d-none d-sm-table-cell">物流</th>
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
                        <span class="text-md">尚未掃描包裹</span>
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
