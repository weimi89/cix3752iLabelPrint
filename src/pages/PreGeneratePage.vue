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
    <AppHeader :title="$t('page.preGenerate.title')" :subtitle="$t('page.preGenerate.subtitle')" icon="tabler-photo-down" />

    <VRow>
      <VCol cols="12" lg="5">
        <div class="left-sticky">
          <VCard>
            <VCardText>
              <AppBulkInput v-model="orderSnList" :label="$t('form.orderSn')" :placeholder="$t('form.orderSnPlaceholder')" clearable-top />
              <div v-if="totalItems > 0" class="mt-3">
                <div class="d-flex justify-space-between mb-1">
                  <span class="text-xs text-medium-emphasis">{{ $t('page.preGenerate.progress') }}</span>
                  <span class="text-xs text-medium-emphasis">{{ completedItems }} / {{ totalItems }}（{{ progressPct }}%）</span>
                </div>
                <VProgressLinear :model-value="progressPct" color="primary" height="6" rounded />
              </div>
            </VCardText>
          </VCard>

          <div class="d-flex justify-center gap-2 mt-3">
            <VBtn v-if="!isProcessing" color="primary" @click="handleQuery">
              <VIcon icon="tabler-search" class="me-1" />{{ $t('common.search') }}
            </VBtn>
            <VBtn v-else color="error" @click="stopProcessing">
              <VIcon icon="tabler-player-stop" class="me-1" />{{ $t('common.stop') }}
            </VBtn>
          </div>
        </div>
      </VCol>

      <VCol cols="12" lg="7">
        <VCard>
          <VCardText>
            <div v-if="downloadList.length === 0" class="text-center py-12">
              <VIcon icon="tabler-photo-down" size="80" color="primary" class="opacity-50" />
              <h4 class="text-h6 mt-4 mb-1">{{ $t('page.preGenerate.empty') }}</h4>
              <p class="text-body-2 text-medium-emphasis">{{ $t('page.preGenerate.emptyHint') }}</p>
            </div>
            <div v-else class="label-grid">
              <div v-for="item in downloadList" :key="item.sn" class="cell">
                <div class="cell__paper">
                  <img v-if="item.image" :src="item.image" :alt="item.sn" />
                  <div v-else-if="!item.code" class="cell__loading"><VProgressCircular indeterminate size="32" /></div>
                  <div v-else class="cell__error">
                    <VIcon :icon="statusIcon(item.code)" size="40" class="cell__error-icon" />
                    <div class="cell__error-sn">{{ item.sn }}</div>
                    <div class="cell__error-msg">{{ item.message }}</div>
                  </div>
                </div>
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
          {{ $t('page.preGenerate.downloadWarnings') }}
          <VChip size="x-small" color="error" variant="elevated" class="ms-2">{{ downloadStatus.length }}</VChip>
        </div>
      </VCardText>
    </VCard>
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
.cell {
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  overflow: hidden;
}
.cell__paper {
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
.cell__loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: 8px;
}

// 黑底錯誤遮罩(對齐 ScanPrintPage / web 端 .error-mask)
.cell__error {
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
.cell__error-icon {
  color: #ff8a65;
  margin-block-end: 2px;
}
.cell__error-sn {
  font-family: 'Menlo', 'Consolas', monospace;
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 1px;
  background: rgba(255, 255, 255, 0.08);
  padding: 2px 8px;
  border-radius: 4px;
}
.cell__error-msg {
  font-size: 12px;
  line-height: 1.4;
  max-inline-size: 90%;
  word-break: break-word;
  color: #ffd1c1;
}
</style>
