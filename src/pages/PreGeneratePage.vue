<script setup>
import { cloudFetchLabel, cloudPackageOrders, cloudOrdersByDate } from '@/api/tauri'
import { preGenInputMode as inputMode } from '@/composables/usePreGenState'
import {
  isDownloadable, statusLabel, statusIcon, errorMessageFromException,
} from '@/composables/useLabelStatus'
import AppBulkInput from '@/components/AppBulkInput.vue'
import AppHeader from '@/components/AppHeader.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const orderSnList = ref([])
// 縮圖 cells:僅少量(<= THUMBNAIL_LIMIT)時建立並渲染;大量批次不建,避免上萬 DOM/img 把 webview 撐爆
const downloadList = reactive([])
// 失敗清單(限量保留),供下方列出
const downloadStatus = reactive([])
const isProcessing = ref(false)
let abortRequested = false

// 統計改用計數器(避免上萬筆每完成一筆就 filter 全表重算)
const totalCount = ref(0)
const completedCount = ref(0)
const successCount = ref(0)
const failCount = ref(0)
const progressPct = computed(() => (totalCount.value ? Math.round(completedCount.value / totalCount.value * 100) : 0))

const CONCURRENCY = 20
const FAIL_LIST_LIMIT = 300  // 失敗清單最多保留筆數

const startBatch = snList => {
  downloadList.splice(0)
  downloadStatus.splice(0)
  totalCount.value = snList.length
  completedCount.value = 0
  successCount.value = 0
  failCount.value = 0
  for (const sn of snList) {
    downloadList.push({ sn, code: '', message: '', shipping_no: '', shipping_provider: '', image: null })
  }
}

const pushFail = (sn, code, message = '') => {
  if (downloadStatus.length < FAIL_LIST_LIMIT) {
    downloadStatus.push({ sn, code, message: message || statusLabel(code) })
  }
}

const processOne = async (sn, index) => {
  const thumb = downloadList.length ? downloadList[index] : null
  try {
    const data = await cloudFetchLabel(sn, { mode: 'download' })
    const code = data.print_view_status || ''
    if (isDownloadable(code)) {
      successCount.value++
    } else {
      failCount.value++
      pushFail(sn, code)
    }
    if (thumb) {
      thumb.code = code
      thumb.shipping_no = data.print_shipping_no || ''
      thumb.shipping_provider = data.print_shipping_provider || ''
      thumb.image = data.print_file_path || null
      thumb.message = statusLabel(code)
    }
  } catch (e) {
    const msg = errorMessageFromException(e)
    failCount.value++
    pushFail(sn, 'ERROR', msg)
    if (thumb) { thumb.code = 'ERROR'; thumb.message = msg }
  } finally {
    completedCount.value++
  }
}

const processShipments = async snList => {
  isProcessing.value = true
  abortRequested = false
  const total = snList.length
  let cursor = 0

  const worker = async () => {
    while (!abortRequested) {
      const i = cursor++
      if (i >= total) break
      await processOne(snList[i], i)
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
  const list = [...orderSnList.value]
  orderSnList.value = []
  startBatch(list)
  await processShipments(list)
}

// 清關袋號反查(可多組):逐袋反查整袋訂單編號 → 合併去重 → 直接預產
const packageNoList = ref([])
const packageLoading = ref(false)
const packageError = ref('')

const handleQueryByPackage = async () => {
  const pkgs = packageNoList.value
  if (!pkgs.length || packageLoading.value || isProcessing.value) return
  packageError.value = ''
  packageLoading.value = true
  try {
    const allSns = []
    const notFound = []
    for (const pkg of pkgs) {
      const data = await cloudPackageOrders(pkg)
      if (data?.respond_code === 'FIND-PACKAGE-ORDER' && data.order_sns?.length) {
        allSns.push(...data.order_sns)
      } else {
        notFound.push(pkg)
      }
    }
    const uniqueSns = [...new Set(allSns)]
    if (!uniqueSns.length) {
      packageError.value = t('page.preGenerate.packageNotFound')
      return
    }
    if (notFound.length) {
      packageError.value = t('page.preGenerate.packageSomeNotFound', { list: notFound.join('、') })
    }
    packageNoList.value = []
    startBatch(uniqueSns)
    await processShipments(uniqueSns)
  } catch (e) {
    packageError.value = errorMessageFromException(e)
  } finally {
    packageLoading.value = false
  }
}

// 依清關日期執行:選日期 → 反查當日整批訂單編號 → 直接預產
const clearanceDate = ref(new Date().toISOString().slice(0, 10))
const dateLoading = ref(false)
const dateError = ref('')

const handleQueryByDate = async () => {
  const date = clearanceDate.value
  if (!date || dateLoading.value || isProcessing.value) return
  dateError.value = ''
  dateLoading.value = true
  try {
    const data = await cloudOrdersByDate(date, inputMode.value === 'transfer' ? 'transfer' : 'clearance')
    const snList = data?.respond_code === 'FIND-PACKAGE-ORDER' ? (data.order_sns || []) : []
    if (!snList.length) {
      dateError.value = data?.respond_message || t('page.preGenerate.dateNotFound')
      return
    }
    startBatch(snList)
    await processShipments(snList)
  } catch (e) {
    dateError.value = errorMessageFromException(e)
  } finally {
    dateLoading.value = false
  }
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.preGenerate.title')" :subtitle="$t('page.preGenerate.subtitle')" icon="tabler-photo-down" />

    <VRow>
      <VCol cols="12" lg="5">
        <div class="left-sticky">
          <VTabs v-model="inputMode" grow hide-slider class="bookmark-tabs">
            <VTab value="order">{{ $t('page.preGenerate.byOrderSn') }}</VTab>
            <VTab value="package">{{ $t('page.preGenerate.byPackage') }}</VTab>
            <VTab value="clearance">{{ $t('page.preGenerate.sourceClearance') }}</VTab>
            <VTab value="transfer">{{ $t('page.preGenerate.sourceTransfer') }}</VTab>
          </VTabs>

          <VCard class="bookmark-card">
            <VCardText>
              <AppBulkInput v-if="inputMode === 'order'" v-model="orderSnList" :label="$t('form.orderSn')" :placeholder="$t('form.orderSnPlaceholder')" clearable-top />
              <template v-else-if="inputMode === 'package'">
                <AppBulkInput v-model="packageNoList" :label="$t('page.preGenerate.packageNo')" :placeholder="$t('page.preGenerate.packagePlaceholder')" clearable-top />
                <VAlert v-if="packageError" type="error" variant="tonal" density="compact" class="mt-2">{{ packageError }}</VAlert>
              </template>
              <template v-else>
                <div class="text-body-2 text-medium-emphasis mb-1">{{ $t('page.preGenerate.dateLabel') }}</div>
                <AppDatePicker
                  v-model="clearanceDate"
                  :disabled="isProcessing || dateLoading"
                />
                <VAlert v-if="dateError" type="error" variant="tonal" density="compact" class="mt-2">{{ dateError }}</VAlert>
              </template>

              <div v-if="totalCount > 0" class="mt-3">
                <div class="d-flex justify-space-between mb-1">
                  <span class="text-xs text-medium-emphasis">{{ $t('page.preGenerate.progress') }}</span>
                  <span class="text-xs text-medium-emphasis">
                    {{ completedCount }} / {{ totalCount }}（{{ progressPct }}%）·
                    <span class="text-success">{{ $t('page.preGenerate.successCount', { n: successCount }) }}</span>
                  </span>
                </div>
                <VProgressLinear :model-value="progressPct" color="primary" height="6" rounded />
              </div>
            </VCardText>
          </VCard>

          <div class="d-flex justify-center gap-2 mt-3">
            <template v-if="!isProcessing">
              <VBtn v-if="inputMode === 'order'" color="primary" @click="handleQuery">
                <VIcon icon="tabler-search" class="me-1" />{{ $t('common.search') }}
              </VBtn>
              <VBtn v-else-if="inputMode === 'package'" color="primary" :loading="packageLoading" :disabled="!packageNoList.length" @click="handleQueryByPackage">
                <VIcon icon="tabler-search" class="me-1" />{{ $t('page.preGenerate.packageQueryBtn') }}
              </VBtn>
              <VBtn v-else color="primary" :loading="dateLoading" @click="handleQueryByDate">
                <VIcon icon="tabler-player-play" class="me-1" />{{ $t('page.preGenerate.dateQueryBtn') }}
              </VBtn>
            </template>
            <VBtn v-else color="error" @click="stopProcessing">
              <VIcon icon="tabler-player-stop" class="me-1" />{{ $t('common.stop') }}
            </VBtn>
          </div>
        </div>
      </VCol>

      <VCol cols="12" lg="7">
        <VCard>
          <VCardText>
            <div v-if="totalCount === 0" class="text-center py-12">
              <VIcon icon="tabler-photo-down" size="80" color="primary" class="opacity-50" />
              <h4 class="text-h6 mt-4 mb-1">{{ $t('page.preGenerate.empty') }}</h4>
              <p class="text-body-2 text-medium-emphasis">{{ $t('page.preGenerate.emptyHint') }}</p>
            </div>
            <template v-else>
              <!-- 統計列 -->
              <div class="d-flex justify-space-around text-center mb-4">
                <div>
                  <div class="text-h6">{{ totalCount }}</div>
                  <div class="text-caption text-medium-emphasis">{{ $t('page.preGenerate.statTotal') }}</div>
                </div>
                <div>
                  <div class="text-h6">{{ completedCount }}</div>
                  <div class="text-caption text-medium-emphasis">{{ $t('page.preGenerate.statDone') }}</div>
                </div>
                <div>
                  <div class="text-h6 text-success">{{ successCount }}</div>
                  <div class="text-caption text-medium-emphasis">{{ $t('page.preGenerate.statSuccess') }}</div>
                </div>
                <div>
                  <div class="text-h6" :class="failCount ? 'text-error' : 'text-disabled'">{{ failCount }}</div>
                  <div class="text-caption text-medium-emphasis">{{ $t('page.preGenerate.statFail') }}</div>
                </div>
              </div>
              <VProgressLinear :model-value="progressPct" color="primary" height="8" rounded class="mb-4" />

              <!-- 全部 cell 都渲染,但靠 .cell 的 content-visibility + img loading=lazy,
                   只有捲到可視範圍的才真正 layout/paint 與下載圖片,上萬筆也不卡 -->
              <div class="label-grid">
                <div v-for="item in downloadList" :key="item.sn" class="cell">
                  <div class="cell__paper">
                    <img v-if="item.image" :src="item.image" :alt="item.sn" loading="lazy" />
                    <div v-else-if="!item.code" class="cell__loading"><VProgressCircular indeterminate size="32" /></div>
                    <div v-else class="cell__error">
                      <VIcon :icon="statusIcon(item.code)" size="40" class="cell__error-icon" />
                      <div class="cell__error-sn">{{ item.sn }}</div>
                      <div class="cell__error-msg">{{ item.message }}</div>
                    </div>
                  </div>
                </div>
              </div>
            </template>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <VCard v-if="downloadStatus.length > 0" class="mt-3" border>
      <VCardText>
        <div class="text-body-1 mb-2">
          <VIcon icon="tabler-alert-triangle" color="error" class="me-1" />
          {{ $t('page.preGenerate.downloadWarnings') }}
          <VChip size="x-small" color="error" variant="elevated" class="ms-2">{{ failCount }}</VChip>
        </div>
        <VTable density="compact">
          <thead>
            <tr>
              <th class="text-start">{{ $t('form.orderSn') }}</th>
              <th class="text-start">{{ $t('page.preGenerate.failReason') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(f, i) in downloadStatus" :key="f.sn + i">
              <td>{{ f.sn }}</td>
              <td>{{ f.message || f.code }}</td>
            </tr>
          </tbody>
        </VTable>
        <div v-if="failCount > downloadStatus.length" class="text-caption text-medium-emphasis mt-2">
          {{ $t('page.preGenerate.failMore', { n: failCount - downloadStatus.length }) }}
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

/* 書籤式分頁:平均寬、上圓角,選中頁籤白底高亮並與下方卡片連成一體 */
.bookmark-tabs {
  min-block-size: 42px;
}
.bookmark-tabs :deep(.v-tab.v-btn) {
  border-start-start-radius: 10px !important;
  border-start-end-radius: 10px !important;
  border-end-start-radius: 0 !important;
  border-end-end-radius: 0 !important;
  margin-inline-end: 3px;
  min-block-size: 42px;
  background: rgba(var(--v-theme-on-surface), 0.05);
  color: rgba(var(--v-theme-on-surface), 0.6);
  text-transform: none;
  letter-spacing: normal;
}
.bookmark-tabs :deep(.v-tab.v-btn:last-child) {
  margin-inline-end: 0;
}
.bookmark-tabs :deep(.v-tab--selected) {
  background: rgb(var(--v-theme-primary)) !important;
  color: #fff !important;
  font-weight: 700;
}
/* 卡片上緣左右改直角,與上方 tab 列平接成一體 */
.bookmark-card {
  border-start-start-radius: 0 !important;
  border-start-end-radius: 0 !important;
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
  /* 離開可視範圍的 cell 由瀏覽器跳過 layout/paint(只渲染畫面看得到的);
     contain-intrinsic-size 給離屏 cell 一個預估高度,維持捲軸長度正確 */
  content-visibility: auto;
  contain-intrinsic-size: auto 240px;
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
