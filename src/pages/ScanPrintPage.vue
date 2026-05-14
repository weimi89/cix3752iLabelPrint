<script setup>
import { cloudFetchLabel, printImage } from '@/api/tauri'
import { isPrintable, statusLabel, statusIcon, statusGroupColor, errorMessageFromException } from '@/composables/useLabelStatus'
import AppBulkInput from '@/components/AppBulkInput.vue'
import AppHeader from '@/components/AppHeader.vue'

const STORAGE_KEY = 'cix3752iLabelPrint.printerMap'

const orderSnList = ref([])
const printType = ref('ALL')
const enforce = ref(false)

const printList = reactive([])
const printStatus = reactive([])
const progressPct = ref(0)
const isProcessing = ref(false)
const isCancelled = ref(false)
let abortRequested = false

const totalItems = computed(() => printList.length)
const completedItems = computed(() => printList.filter(p => p.code !== '').length)
const successItems = computed(() => printList.filter(p => isPrintable(p.code)).length)

const CONCURRENCY = 3

const printerMap = computed(() => {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  } catch {
    return {}
  }
})

const PRINT_TYPE_OPTIONS = [
  { value: 'ALL', title: '全部' },
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

const initPrintList = snList => {
  printList.splice(0)
  printStatus.splice(0)
  for (const sn of snList) {
    printList.push({
      sn,
      code: '',
      message: '',
      shipping_no: '',
      shipping_provider: '',
      image: null,
      print_time: [],
    })
  }
}

const insertStatusByIndex = (index, entry) => {
  const tagged = { ...entry, _idx: index }
  let pos = printStatus.length
  while (pos > 0 && printStatus[pos - 1]._idx > index) pos--
  printStatus.splice(pos, 0, tagged)
}

const processOne = async index => {
  const item = printList[index]
  try {
    const data = await cloudFetchLabel(item.sn, {
      printType: printType.value,
      enforce: enforce.value,
      mode: 'web_print',
    })
    item.code = data.print_view_status || ''
    item.shipping_no = data.print_shipping_no || ''
    item.shipping_provider = data.print_shipping_provider || ''
    item.image = data.print_file_path || null
    item.message = statusLabel(item.code)
    item.print_time = Array.isArray(data.print_time) ? data.print_time : []
    if (item.code !== 'LABEL-PROCESS') {
      insertStatusByIndex(index, { sn: item.sn, code: item.code })
    }
  } catch (e) {
    item.code = 'ERROR'
    item.message = errorMessageFromException(e)
    insertStatusByIndex(index, { sn: item.sn, code: 'ERROR' })
  }
}

const processShipments = async () => {
  isProcessing.value = true
  isCancelled.value = false
  abortRequested = false
  progressPct.value = 0

  const total = printList.length
  let cursor = 0
  let completed = 0

  const worker = async () => {
    while (!abortRequested) {
      const i = cursor++
      if (i >= total) break
      await processOne(i)
      completed += 1
      progressPct.value = Math.round((completed / total) * 100)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(CONCURRENCY, total) }, () => worker()),
  )
  isProcessing.value = false
}

const stopProcessing = () => {
  abortRequested = true
  isCancelled.value = true
}

const handleQuery = async () => {
  if (orderSnList.value.length === 0) return
  initPrintList(orderSnList.value)
  orderSnList.value = []
  await processShipments()
}

const handlePrintAll = async () => {
  const printable = printList.filter(p => isPrintable(p.code) && p.image)
  for (const item of printable) {
    const printerName = printerMap.value[item.shipping_provider]
    if (!printerName) continue
    try {
      // image 可能是 http://... 或 file://...，原生列印先支援 file path
      const path = item.image.startsWith('file://') ? item.image.slice(7) : item.image
      await printImage({ printerName, imagePath: path })
    } catch (e) {
      console.error('列印失敗', item.sn, e)
    }
  }
}

const groupedStatus = computed(() => {
  const m = {}
  for (const s of printStatus) {
    if (!m[s.code]) m[s.code] = []
    m[s.code].push(s.sn)
  }
  return m
})
</script>

<template>
  <div>
    <AppHeader title="掃描列印" subtitle="逐筆載入面單列印" icon="tabler-browser">
      <template #actions>
        <VSwitch
          v-model="enforce"
          label="強制列印"
          color="warning"
          inset
          hide-details
          density="compact"
        />
      </template>
    </AppHeader>

    <VRow>
      <VCol cols="12" lg="5">
        <VCard>
          <VCardText>
            <div class="mb-3">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">列印範圍</VLabel>
              <VSelect
                v-model="printType"
                :items="PRINT_TYPE_OPTIONS"
                item-title="title"
                item-value="value"
              />
            </div>
            <AppBulkInput v-model="orderSnList" label="訂單編號" placeholder="可掃描連續輸入，或貼上多筆，以換行 / 逗號 / 空白分隔" />

            <div v-if="totalItems > 0" class="mt-3">
              <div class="d-flex justify-space-between mb-1">
                <span class="text-xs text-medium-emphasis">面單載入進度</span>
                <span class="text-xs text-medium-emphasis">{{ completedItems }} / {{ totalItems }}（{{ progressPct }}%）</span>
              </div>
              <VProgressLinear :model-value="progressPct" color="primary" height="6" rounded />
            </div>
          </VCardText>
        </VCard>

        <div class="d-flex justify-center gap-2 mt-3">
          <VBtn v-if="!isProcessing" color="primary" :disabled="orderSnList.length === 0" @click="handleQuery">
            <VIcon icon="tabler-search" class="me-1" />查詢
          </VBtn>
          <VBtn v-else color="error" @click="stopProcessing">
            <VIcon icon="tabler-player-stop" class="me-1" />停止
          </VBtn>
          <VBtn v-if="!isProcessing && successItems > 0" color="success" @click="handlePrintAll">
            <VIcon icon="tabler-printer" class="me-1" />貼標列印（{{ successItems }} 筆）
          </VBtn>
        </div>

        <VCard v-if="Object.keys(groupedStatus).length > 0" class="mt-3" variant="flat" border>
          <VCardItem>
            <VCardTitle class="text-body-1 d-flex align-center">
              <VIcon icon="tabler-alert-triangle" color="error" class="me-1" />
              列印警告
              <VChip size="x-small" color="error" variant="elevated" class="ms-2">{{ printStatus.length }}</VChip>
            </VCardTitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-for="(snList, code) in groupedStatus" :key="code" class="mb-3">
              <div class="d-flex align-center mb-1">
                <VIcon :icon="statusIcon(code)" :color="statusGroupColor(code)" size="18" class="me-1" />
                <span class="text-body-2 font-weight-medium" :class="`text-${statusGroupColor(code)}`">
                  {{ statusLabel(code) }}
                </span>
                <VSpacer />
                <VChip size="x-small" :color="statusGroupColor(code)" variant="elevated">{{ snList.length }}</VChip>
              </div>
              <div class="d-flex flex-wrap gap-1">
                <VChip
                  v-for="sn in snList"
                  :key="sn"
                  size="small"
                  :color="statusGroupColor(code)"
                  variant="tonal"
                >
                  {{ sn }}
                </VChip>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" lg="7">
        <VCard>
          <VCardText>
            <div v-if="printList.length === 0" class="text-center py-12">
              <VIcon icon="tabler-printer" size="80" color="primary" class="opacity-50" />
              <h4 class="text-h6 mt-4 mb-1">尚未載入面單</h4>
              <p class="text-body-2 text-medium-emphasis">請在左側輸入訂單編號後按下「查詢」</p>
            </div>
            <div v-else class="d-flex flex-wrap gap-3">
              <div
                v-for="item in printList"
                :key="item.sn"
                class="label-cell"
              >
                <div class="label-cell__paper">
                  <img v-if="item.image" :src="item.image" :alt="item.sn" />
                  <div v-else-if="!item.code" class="label-cell__loading">
                    <VProgressCircular indeterminate size="32" />
                  </div>
                  <div v-else class="label-cell__error">
                    <VIcon :icon="statusIcon(item.code)" size="32" />
                    <div class="text-caption mt-1">{{ item.message }}</div>
                  </div>
                </div>
                <div class="text-caption text-center py-1">{{ item.sn }}</div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>

<style scoped>
.label-cell {
  inline-size: 200px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  overflow: hidden;
}
.label-cell__paper {
  position: relative;
  aspect-ratio: 4 / 3;
  background: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
}
.label-cell__paper img {
  max-inline-size: 100%;
  max-block-size: 100%;
  object-fit: contain;
}
.label-cell__loading,
.label-cell__error {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: 8px;
}
.label-cell__error {
  color: rgb(var(--v-theme-error));
}
</style>
