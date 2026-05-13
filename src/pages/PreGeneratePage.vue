<script setup>
import { cloudFetchLabel } from '@/api/tauri'
import {
  isDownloadable, statusLabel, statusIcon, errorMessageFromException,
} from '@/composables/useLabelStatus'
import AppBulkInput from '@/components/AppBulkInput.vue'
import AppHeader from '@/components/AppHeader.vue'

const orderSnList = ref([])
const downloadList = reactive([])
const downloadStatus = reactive([])
const progressPct = ref(0)
const isProcessing = ref(false)
let abortRequested = false

const totalItems = computed(() => downloadList.length)
const completedItems = computed(() => downloadList.filter(p => p.code !== '').length)
const successItems = computed(() => downloadList.filter(p => isDownloadable(p.code)).length)

const CONCURRENCY = 5

const initDownloadList = snList => {
  downloadList.splice(0)
  downloadStatus.splice(0)
  for (const sn of snList) {
    downloadList.push({
      sn, code: '', message: '', shipping_no: '', shipping_provider: '', image: null,
    })
  }
}

const insertStatusByIndex = (index, entry) => {
  const tagged = { ...entry, _idx: index }
  let pos = downloadStatus.length
  while (pos > 0 && downloadStatus[pos - 1]._idx > index) pos--
  downloadStatus.splice(pos, 0, tagged)
}

const processOne = async index => {
  const item = downloadList[index]
  try {
    const data = await cloudFetchLabel(item.sn, { mode: 'download' })
    item.code = data.print_view_status || ''
    item.shipping_no = data.print_shipping_no || ''
    item.shipping_provider = data.print_shipping_provider || ''
    item.image = data.print_file_path || null
    item.message = statusLabel(item.code)
    if (!isDownloadable(item.code)) {
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
  abortRequested = false
  progressPct.value = 0
  const total = downloadList.length
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

const stopProcessing = () => { abortRequested = true }

const handleQuery = async () => {
  if (orderSnList.value.length === 0) return
  initDownloadList(orderSnList.value)
  orderSnList.value = []
  await processShipments()
}
</script>

<template>
  <div>
    <AppHeader title="面單預產" subtitle="先讓伺服器產好面單圖（不寫列印記錄、不檢查出貨狀態）" icon="tabler-photo-down" />

    <VRow>
      <VCol cols="12" lg="5">
        <VCard>
          <VCardText>
            <AppBulkInput v-model="orderSnList" />
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
        </div>
      </VCol>

      <VCol cols="12" lg="7">
        <VCard>
          <VCardText>
            <div v-if="downloadList.length === 0" class="text-center py-12">
              <VIcon icon="tabler-photo-down" size="80" color="primary" class="opacity-50" />
              <h4 class="text-h6 mt-4 mb-1">尚未載入面單</h4>
              <p class="text-body-2 text-medium-emphasis">請在左側輸入訂單編號後按下「查詢」</p>
            </div>
            <div v-else class="d-flex flex-wrap gap-3">
              <div v-for="item in downloadList" :key="item.sn" class="cell">
                <div class="cell__paper">
                  <img v-if="item.image" :src="item.image" :alt="item.sn" />
                  <div v-else-if="!item.code" class="cell__loading"><VProgressCircular indeterminate size="32" /></div>
                  <div v-else class="cell__error">
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

    <VCard v-if="downloadStatus.length > 0" class="mt-3" border>
      <VCardText>
        <div class="text-body-1 mb-2">
          <VIcon icon="tabler-alert-triangle" color="error" class="me-1" />
          下載警告
          <VChip size="x-small" color="error" variant="elevated" class="ms-2">{{ downloadStatus.length }}</VChip>
        </div>
      </VCardText>
    </VCard>
  </div>
</template>

<style scoped>
.cell {
  inline-size: 200px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  overflow: hidden;
}
.cell__paper {
  position: relative;
  aspect-ratio: 4 / 3;
  background: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
}
.cell__paper img {
  max-inline-size: 100%;
  max-block-size: 100%;
  object-fit: contain;
}
.cell__loading,
.cell__error {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 8px;
  text-align: center;
}
.cell__error {
  color: rgb(var(--v-theme-error));
}
</style>
