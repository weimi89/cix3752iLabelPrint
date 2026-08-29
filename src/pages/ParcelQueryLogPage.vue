<script setup>
import { parcelQueryLogList, getConfig } from '@/api/tauri'
import { listen } from '@tauri-apps/api/event'
import AppHeader from '@/components/AppHeader.vue'
import TablePagination from '@/components/TablePagination.vue'
import MultiNoField from '@/components/MultiNoField.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const searchKeyword = ref('')
const searchQueryNo = ref('')
const searchTrackingNo = ref('')
const msField = ref(null)
const minMs = ref(null)
const items = ref([])
const total = ref(0)
const loading = ref(false)
const errorMsg = ref('')

// 通用圖片檢視器:存證快照與面單原圖共用一套對話框,但走「不同」本機 HTTP 路由 ——
// 存證走 /captures(獨立存證目錄,與面單快取分離);面單原圖走 /images(面單快取)。server 同機,viewer 用 127.0.0.1。
const serverPort = ref(18080)
const mediaUrl = (route, key) => (key ? `http://127.0.0.1:${serverPort.value}/${route}/${encodeURI(key)}` : '')
const viewer = ref({ open: false, title: '', key: '', url: '' })
// 圖片載入失敗旗標(404 / 檔案已被清理):每次開新圖前歸零,<img> @error 時設 true → 顯示「圖案已遺失」
const viewerError = ref(false)
const openSnapshot = row => {
  viewerError.value = false
  viewer.value = { open: true, title: t('page.parcelQueryLog.snapshotTitle'), key: row.photo_path, url: mediaUrl('captures', row.photo_path) }
}
const openLabel = row => {
  viewerError.value = false
  viewer.value = { open: true, title: t('page.parcelQueryLog.labelTitle'), key: row.label_key, url: mediaUrl('images', row.label_key) }
}

const MS_FIELDS = computed(() => [
  { title: t('page.parcelQueryLog.msFieldAny'), value: null },
  { title: t('page.parcelQueryLog.col.cloudMs'), value: 'cloud' },
  { title: t('page.parcelQueryLog.col.labelMs'), value: 'label' },
  { title: t('page.parcelQueryLog.col.totalMs'), value: 'total' },
])

const page = ref(1)
const pageSize = ref(25)
const PAGE_SIZES = [25, 50, 100, 200]
const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)))

const searchOpen = ref(0)

const resetSearch = () => { searchKeyword.value = ''; searchQueryNo.value = ''; searchTrackingNo.value = ''; msField.value = null; minMs.value = null }

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const resp = await parcelQueryLogList({
      keyword: searchKeyword.value || null,
      queryNo: searchQueryNo.value || null,
      trackingNo: searchTrackingNo.value || null,
      msField: msField.value,
      minMs: Number(minMs.value) > 0 ? Number(minMs.value) : null,
      limit: pageSize.value,
      offset: (page.value - 1) * pageSize.value,
    })
    items.value = resp.items || []
    total.value = resp.total || 0
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

watch([searchKeyword, searchQueryNo, searchTrackingNo, msField, minMs], () => { page.value = 1 })
watch(pageSize, () => { page.value = 1; load() })
watch(page, load)
let _unlisten = null
let _reloadTimer = null
// 去抖:高頻事件(連續 NoRead / 大量查詢)合併為單次 reload,
// 避免每筆都全量重查 + 重繪造成畫面閃爍與捲動位置跳動。
const scheduleReload = () => {
  if (_reloadTimer) return
  _reloadTimer = setTimeout(() => { _reloadTimer = null; load() }, 400)
}
let _disposed = false
onMounted(async () => {
  load()
  try { const cfg = await getConfig(); if (cfg?.server?.port) serverPort.value = cfg.server.port } catch {}
  // await getConfig 期間可能已切頁;若已卸載則立刻解除,避免 Tauri 監聽殘留(每筆 /api/parcel 都觸發,殘留會放大後端查詢)
  if (isTauriRuntime) try {
    const un = await listen('parcel-query-logged', scheduleReload)
    if (_disposed) un(); else _unlisten = un
  } catch {}
})
onUnmounted(() => {
  _disposed = true
  if (_unlisten) { _unlisten(); _unlisten = null }
  if (_reloadTimer) { clearTimeout(_reloadTimer); _reloadTimer = null }
})

const formatDate = s => s ? s.replace('T', ' ').slice(0, 19) : ''
</script>

<style scoped lang="scss">
.parcel-log-table {
  th,
  td {
    white-space: nowrap;
  }
}

.viewer-img {
  display: block;
  max-width: 100%;
  max-height: 70vh;
  margin: 0 auto;
}
</style>

<template>
  <div>
    <AppHeader :title="$t('page.parcelQueryLog.title')" :subtitle="$t('page.parcelQueryLog.subtitle')" icon="tabler-history">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="primary" :loading="loading" @click="load">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem @click="load">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>{{ $t('common.reload') }}</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <!-- 進階查詢 -->
    <VExpansionPanels v-model="searchOpen" class="mb-3 advanced-search">
      <VExpansionPanel>
        <VExpansionPanelTitle class="advanced-search__title">{{ $t('common.advancedSearch') }}</VExpansionPanelTitle>
        <VExpansionPanelText>
          <VRow no-gutters class="mx-n2">
            <VCol cols="12" md="3" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.parcelQueryLog.col.queryNo') }}</label>
                <MultiNoField v-model="searchQueryNo" @search="load" />
              </div>
            </VCol>
            <VCol cols="12" md="3" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.parcelQueryLog.col.trackingNo') }}</label>
                <MultiNoField v-model="searchTrackingNo" @search="load" />
              </div>
            </VCol>
            <VCol cols="12" md="6" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.parcelQueryLog.keyword') }}</label>
                <VTextField
                  v-model="searchKeyword"
                  :placeholder="$t('page.parcelQueryLog.keywordPlaceholder')"
                  density="compact"
                  hide-details
                  variant="outlined"
                  @keyup.enter="load"
                />
              </div>
            </VCol>
            <VCol cols="6" md="3" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.parcelQueryLog.msField') }}</label>
                <VSelect
                  v-model="msField"
                  :items="MS_FIELDS"
                  density="compact"
                  hide-details
                  variant="outlined"
                />
              </div>
            </VCol>
            <VCol cols="6" md="3" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.parcelQueryLog.minMs') }}</label>
                <VNumberInput
                  v-model="minMs"
                  :min="0"
                  :step="100"
                  :placeholder="$t('page.parcelQueryLog.minMsPlaceholder')"
                  density="compact"
                  hide-details
                  variant="outlined"
                  control-variant="stacked"
                  @keyup.enter="load"
                />
              </div>
            </VCol>
          </VRow>
          <div class="d-flex justify-center pt-4 pb-0 ga-2">
            <VBtn variant="text" color="default" @click="resetSearch">
              <VIcon icon="tabler-eraser" size="18" class="me-1" />{{ $t('common.reset') }}
            </VBtn>
            <VBtn variant="elevated" color="primary" @click="load">
              <VIcon icon="tabler-database-search" size="18" class="me-1" />{{ $t('common.search') }}
            </VBtn>
          </div>
        </VExpansionPanelText>
      </VExpansionPanel>
    </VExpansionPanels>

    <VCard>
      <div class="d-flex align-center ga-3 px-4 py-1">
        <VSpacer />
        <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" header />
      </div>

      <VDivider />

      <VTable hover class="parcel-log-table">
        <thead>
          <tr>
            <th class="text-center" style="width: 170px;">{{ $t('page.parcelQueryLog.col.createdAt') }}</th>
            <th class="text-center" style="min-width: 150px;">{{ $t('page.parcelQueryLog.col.queryNo') }}</th>
            <th class="text-center" style="min-width: 150px;">{{ $t('page.parcelQueryLog.col.trackingNo') }}</th>
            <th class="text-center" style="width: 90px;">{{ $t('page.parcelQueryLog.col.shippingProvider') }}</th>
            <th class="text-center" style="width: 90px;">{{ $t('page.parcelQueryLog.col.channel') }}</th>
            <th class="text-center" style="min-width: 160px;">{{ $t('page.parcelQueryLog.col.printProfile') }}</th>
            <th class="text-center" style="width: 110px;">{{ $t('page.parcelQueryLog.col.responseId') }}</th>
            <th class="text-center" style="width: 80px;">{{ $t('page.parcelQueryLog.col.cloudMs') }}</th>
            <th class="text-center" style="width: 80px;">{{ $t('page.parcelQueryLog.col.labelMs') }}</th>
            <th class="text-center" style="width: 80px;">{{ $t('page.parcelQueryLog.col.totalMs') }}</th>
            <th class="text-center" style="width: 70px;">{{ $t('page.parcelQueryLog.col.labelKey') }}</th>
            <th class="text-center" style="width: 70px;">{{ $t('page.parcelQueryLog.col.snapshot') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!items.length">
            <td colspan="12">
              <div class="py-2 d-flex align-center justify-center">
                <VIcon icon="tabler-alert-circle" size="20" class="me-1" />
                <span class="text-md">{{ $t('common.noResults') }}</span>
              </div>
            </td>
          </tr>
          <tr v-for="row in items" :key="row.response_id">
            <td class="text-center">{{ formatDate(row.created_at) }}</td>
            <td class="text-center">{{ row.query_no }}</td>
            <td class="text-center">{{ row.tracking_no }}</td>
            <td class="text-center">{{ row.shipping_provider || '—' }}</td>
            <td class="text-center">{{ row.sort_channel || '—' }}</td>
            <td class="text-center">{{ row.print_profile || '—' }}</td>
            <td class="text-center text-disabled">{{ row.response_id }}</td>
            <td class="text-center text-caption">{{ row.cloud_ms != null ? row.cloud_ms + 'ms' : '—' }}</td>
            <td class="text-center text-caption">{{ row.label_ms != null ? row.label_ms + 'ms' : '—' }}</td>
            <td class="text-center text-caption" :class="row.total_ms != null && row.total_ms > 3000 ? 'text-warning' : ''">{{ row.total_ms != null ? row.total_ms + 'ms' : '—' }}</td>
            <td class="text-center">
              <VBtn v-if="row.label_key" icon variant="text" color="primary" density="comfortable" size="small" @click="openLabel(row)">
                <VIcon icon="tabler-photo" size="20" />
                <VTooltip activator="parent" location="top">{{ $t('page.parcelQueryLog.viewLabel') }}</VTooltip>
              </VBtn>
              <span v-else class="text-disabled">—</span>
            </td>
            <td class="text-center">
              <VBtn
                v-if="row.photo_path"
                icon
                variant="text"
                color="primary"
                density="comfortable"
                size="small"
                @click="openSnapshot(row)"
              >
                <VIcon icon="tabler-camera" size="20" />
                <VTooltip activator="parent" location="top">{{ $t('page.parcelQueryLog.viewSnapshot') }}</VTooltip>
              </VBtn>
              <span v-else class="text-disabled">—</span>
            </td>
          </tr>
        </tbody>
      </VTable>

      <VDivider />

      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" />
    </VCard>

    <!-- 通用圖片檢視器:存證快照 / 面單原圖共用 -->
    <VDialog v-model="viewer.open" max-width="760">
      <div v-if="viewer.key" style="position: relative;">
        <VBtn
          icon
          variant="elevated"
          size="x-small"
          style="position: absolute; top: -12px; right: -12px; z-index: 10;"
          @click="viewer.open = false"
        >
          <VIcon icon="tabler-x" size="14" />
        </VBtn>
        <VCard>
        <VCardItem>
          <VCardTitle>{{ viewer.title }}</VCardTitle>
        </VCardItem>
        <VDivider />
        <VCardText class="text-center">
          <img
            v-if="viewer.url && !viewerError"
            :src="viewer.url"
            class="viewer-img"
            alt=""
            @error="viewerError = true"
          />
          <div
            v-else
            class="d-flex flex-column align-center justify-center text-disabled"
            style="min-height: 180px;"
          >
            <VIcon icon="tabler-photo-off" size="40" class="mb-2" />
            <div>{{ $t('page.parcelQueryLog.imageLoadFailed') }}</div>
          </div>
          <div class="text-caption text-disabled mt-2"><code>{{ viewer.key }}</code></div>
        </VCardText>
        </VCard>
      </div>
    </VDialog>
  </div>
</template>
