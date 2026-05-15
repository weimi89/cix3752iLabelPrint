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

const isPrinting = ref(false)
const printProgress = reactive({ done: 0, total: 0, ok: 0, fail: 0, skip: 0 })
const printSummary = ref(null)
const printErrors = ref([])
let printSummaryTimer = null

const handlePrintAll = async () => {
  const printable = printList.filter(p => isPrintable(p.code) && p.image)
  printProgress.done = 0
  printProgress.total = printable.length
  printProgress.ok = 0
  printProgress.fail = 0
  printProgress.skip = 0
  printSummary.value = null
  printErrors.value = []
  if (printSummaryTimer) { clearTimeout(printSummaryTimer); printSummaryTimer = null }
  isPrinting.value = true
  try {
    for (const item of printable) {
      const printerName = printerMap.value[item.shipping_provider]
      if (!printerName) {
        printProgress.skip += 1
        printProgress.done += 1
        continue
      }
      try {
        // image 可能是 http://... 或 file://...，原生列印先支援 file path
        const path = item.image.startsWith('file://') ? item.image.slice(7) : item.image
        await printImage({ printerName, imagePath: path })
        printProgress.ok += 1
      } catch (e) {
        console.error('列印失敗', item.sn, e)
        printProgress.fail += 1
        printErrors.value.push({ sn: item.sn, message: String(e) })
      } finally {
        printProgress.done += 1
      }
    }
  } finally {
    isPrinting.value = false
    printSummary.value = {
      ok: printProgress.ok,
      fail: printProgress.fail,
      skip: printProgress.skip,
      total: printProgress.total,
    }
    printSummaryTimer = setTimeout(() => { printSummary.value = null }, 12000)
  }
}

const printProgressPct = computed(() =>
  printProgress.total > 0 ? Math.round((printProgress.done / printProgress.total) * 100) : 0,
)

const printSummaryColor = computed(() => {
  if (!printSummary.value) return 'info'
  if (printSummary.value.fail > 0) return 'error'
  if (printSummary.value.skip > 0) return 'warning'
  return 'success'
})

const removePrintItem = (sn) => {
  const idx = printList.findIndex(p => p.sn === sn)
  if (idx >= 0) printList.splice(idx, 1)
  const sidx = printStatus.findIndex(s => s.sn === sn)
  if (sidx >= 0) printStatus.splice(sidx, 1)
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
        <div class="left-sticky">
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
            <AppBulkInput v-model="orderSnList" label="訂單編號" placeholder="可掃描連續輸入，或貼上多筆，以換行 / 逗號 / 空白分隔" clearable-top />

            <div v-if="totalItems > 0" class="mt-3">
              <div class="d-flex justify-space-between mb-1">
                <span class="text-xs text-medium-emphasis">面單載入進度</span>
                <span class="text-xs text-medium-emphasis">{{ completedItems }} / {{ totalItems }}（{{ progressPct }}%）</span>
              </div>
              <VProgressLinear :model-value="progressPct" color="primary" height="6" rounded />
            </div>

            <div v-if="isPrinting || printProgress.total > 0 && printProgress.done < printProgress.total" class="mt-3">
              <div class="d-flex justify-space-between mb-1">
                <span class="text-xs text-medium-emphasis">列印進度</span>
                <span class="text-xs text-medium-emphasis">{{ printProgress.done }} / {{ printProgress.total }}（{{ printProgressPct }}%）</span>
              </div>
              <VProgressLinear :model-value="printProgressPct" color="success" height="6" rounded :indeterminate="isPrinting && printProgress.total === 0" />
            </div>
          </VCardText>
        </VCard>

        <VAlert
          v-if="printSummary"
          :type="printSummaryColor"
          variant="tonal"
          density="compact"
          class="mt-3"
          closable
          @click:close="printSummary = null"
        >
          <div>
            列印完成：成功 {{ printSummary.ok }} 筆<span v-if="printSummary.fail > 0">、失敗 {{ printSummary.fail }} 筆</span><span v-if="printSummary.skip > 0">、略過 {{ printSummary.skip }} 筆（未設定印表機）</span>
          </div>
          <ul v-if="printErrors.length > 0" class="mt-1 ps-4 text-xs">
            <li v-for="(err, idx) in printErrors.slice(0, 5)" :key="idx">{{ err.sn }}：{{ err.message }}</li>
            <li v-if="printErrors.length > 5">…（還有 {{ printErrors.length - 5 }} 筆）</li>
          </ul>
        </VAlert>

        <div class="d-flex justify-center gap-2 mt-3">
          <VBtn v-if="!isProcessing" color="primary" :disabled="orderSnList.length === 0 || isPrinting" @click="handleQuery">
            <VIcon icon="tabler-search" class="me-1" />查詢
          </VBtn>
          <VBtn v-else color="error" @click="stopProcessing">
            <VIcon icon="tabler-player-stop" class="me-1" />停止
          </VBtn>
          <VBtn v-if="!isProcessing && successItems > 0" color="success" :loading="isPrinting" :disabled="isPrinting || completedItems < totalItems" @click="handlePrintAll">
            <VIcon icon="tabler-printer" class="me-1" />
            <template v-if="isPrinting">列印中（{{ printProgress.done }} / {{ printProgress.total }}）</template>
            <template v-else-if="completedItems < totalItems">面單載入中（{{ completedItems }} / {{ totalItems }}）</template>
            <template v-else>貼標列印（{{ successItems }} 筆）</template>
          </VBtn>
        </div>
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
            <div v-if="printList.length === 0" class="empty-state">
              <VIcon icon="tabler-printer" size="80" color="primary" class="empty-state__icon" />
              <h4 class="text-h6 mt-4 mb-1">尚未載入面單</h4>
              <p class="text-body-2 text-medium-emphasis">請在左側輸入訂單編號後按下「查詢」</p>
              <div class="empty-state__steps">
                <div class="step-item">
                  <span class="step-num">1</span>
                  <span>選擇列印範圍與是否強制列印</span>
                </div>
                <div class="step-item">
                  <span class="step-num">2</span>
                  <span>輸入或掃描訂單編號（可多筆）</span>
                </div>
                <div class="step-item">
                  <span class="step-num">3</span>
                  <span>系統會逐筆載入面單</span>
                </div>
                <div class="step-item">
                  <span class="step-num">4</span>
                  <span>確認後點「貼標列印」送印</span>
                </div>
              </div>
            </div>
            <div v-else class="label-grid">
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
                    <VIcon :icon="statusIcon(item.code)" size="40" class="label-cell__error-icon" />
                    <div class="label-cell__error-sn">{{ item.sn }}</div>
                    <div class="label-cell__error-msg">{{ item.message }}</div>
                  </div>

                  <!-- 重覆列印警示（覆蓋於面單上方）-->
                  <div v-if="item.code === 'SHIPPING-REPEAT'" class="repeat-overlay">
                    <div class="repeat-overlay__content">
                      <div class="repeat-overlay__head">
                        <VIcon icon="tabler-alert-triangle" size="13" class="me-1" />
                        <strong>重覆列印 {{ item.print_time?.length || 0 }} 次</strong>
                      </div>
                      <div class="repeat-overlay__sn">{{ item.sn }}</div>
                      <div v-if="item.print_time && item.print_time.length > 0" class="repeat-overlay__list">
                        <div v-for="(t, ti) in item.print_time.slice(0, 3)" :key="`t-${ti}`" class="repeat-overlay__time">
                          <span class="me-1">#{{ ti + 1 }}</span>{{ t }}
                        </div>
                        <div v-if="item.print_time.length > 3" class="repeat-overlay__time text-medium-emphasis">
                          …還有 {{ item.print_time.length - 3 }} 次
                        </div>
                      </div>
                    </div>
                    <VBtn size="x-small" variant="flat" color="error" class="repeat-overlay__btn" @click="removePrintItem(item.sn)">
                      <VIcon icon="tabler-trash" size="12" class="me-1" />
                      從列印區移除
                    </VBtn>
                  </div>
                </div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>

<style lang="scss" scoped>
.left-sticky {
  position: sticky;
  inset-block-start: 5rem;
  z-index: 1;
}

.label-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}
.label-cell {
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  overflow: hidden;
}
.label-cell__paper {
  position: relative;
  aspect-ratio: 3 / 4;
  background: #fff;
  display: flex;
  align-items: center;
  justify-content: center;

  img {
    max-inline-size: 100%;
    max-block-size: 100%;
    object-fit: contain;
  }
}
.label-cell__loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: 8px;
}

// 黑底錯誤遮罩(對齊 web 端 .error-mask)
.label-cell__error {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 12px;
  text-align: center;
  background: rgba(20, 20, 24, 0.78);
  color: #fff;
  font-weight: 400;
  letter-spacing: normal;
}
.label-cell__error-icon {
  color: #ff8a65;
  margin-block-end: 2px;
}
.label-cell__error-sn {
  font-family: 'Menlo', 'Consolas', monospace;
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 1px;
  background: rgba(255, 255, 255, 0.08);
  padding: 2px 8px;
  border-radius: 4px;
}
.label-cell__error-msg {
  font-size: 12px;
  line-height: 1.4;
  max-inline-size: 90%;
  word-break: break-word;
  color: #ffd1c1;
}

// 重覆列印警示覆蓋層(對齊 web 端 .repeat-overlay)
.repeat-overlay {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  padding: 10px;
  text-align: center;
  z-index: 5;

  &__content {
    background: rgba(0, 0, 0, 0.6);
    border-radius: 6px;
    padding: 8px;
    max-block-size: calc(100% - 40px);
    overflow-y: auto;
  }

  &__head {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    color: #ffd54f;
    margin-block-end: 4px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
  }

  &__sn {
    font-family: 'Menlo', 'Consolas', monospace;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 1px;
    margin-block-end: 6px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
  }

  &__list {
    margin-block-end: 4px;
  }

  &__time {
    font-family: 'Menlo', 'Consolas', monospace;
    font-size: 11px;
    line-height: 1.5;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
  }

  &__btn {
    position: absolute;
    inset-block-end: 8px;
    inset-inline: 0;
    margin: 0 auto;
    inline-size: max-content;
  }
}

.empty-state {
  text-align: center;
  padding: 56px 24px;

  &__icon {
    opacity: 0.35;
  }

  &__steps {
    display: inline-block;
    text-align: start;
    margin-block-start: 24px;
    padding: 16px 24px;
    border: 1px dashed rgba(var(--v-theme-on-surface), 0.15);
    border-radius: 12px;
    background: rgba(var(--v-theme-on-surface), 0.02);
  }

  .step-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    font-size: 14px;
    color: rgba(var(--v-theme-on-surface), 0.7);
  }

  .step-num {
    flex-shrink: 0;
    inline-size: 22px;
    block-size: 22px;
    border-radius: 50%;
    background: rgb(var(--v-theme-primary));
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    display: flex;
    align-items: center;
    justify-content: center;
  }
}
</style>
