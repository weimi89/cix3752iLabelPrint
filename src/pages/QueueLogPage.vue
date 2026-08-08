<script setup>
import { queueList, queueRetryFailed, queuePurge } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import TablePagination from '@/components/TablePagination.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const queueStatus = ref(null)
const searchKeyword = ref('')
/// null = 全部;true = 只看工控機沒回報的(直印模式下用來抓工控機回報鏈是否斷掉)
const unreportedOnly = ref(null)
const queueItems = ref([])
const loading = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const page = ref(1)
const pageSize = ref(25)
const PAGE_SIZES = [25, 50, 100, 200]
const mockTotal = ref(0)
const totalPages = computed(() => Math.max(1, Math.ceil(mockTotal.value / pageSize.value)))

const searchOpen = ref(0)

const resetSearch = () => {
  queueStatus.value = null
  searchKeyword.value = ''
  unreportedOnly.value = null
}

const QUEUE_STATUSES = computed(() => [
  { title: t('common.all'), value: null },
  { title: t('page.queue.status.pending'), value: 'pending' },
  { title: t('page.queue.status.sending'), value: 'sending' },
  { title: t('page.queue.status.success'), value: 'success' },
  { title: t('page.queue.status.failed'), value: 'failed' },
])
const STATUS_LABELS = computed(() => ({
  pending: t('page.queue.status.pending'),
  sending: t('page.queue.status.sending'),
  success: t('page.queue.status.success'),
  failed: t('page.queue.status.failed'),
}))

const REPORTED_FILTERS = computed(() => [
  { title: t('common.all'), value: null },
  { title: t('page.queue.filter.unreportedOnly'), value: true },
])

const CHANNELS_SAMPLE = ['A1', 'A2', 'B1', 'B2', 'C1']
const STICKERS_SAMPLE = ['小美', '阿傑', '阿宏', '貓宅']
const STATUSES_SAMPLE = ['success', 'success', 'success', 'pending', 'sending', 'failed']

// 瀏覽器預覽模式 mock 資料(80 筆,展示分頁)
// 來源刻意涵蓋三種樣態:工控機回報、中介機直印自補(已回報 / 未回報 / 遲到回報)
const MOCK_QUEUE = Array.from({ length: 80 }, (_, i) => {
  const status = STATUSES_SAMPLE[i % STATUSES_SAMPLE.length]
  const hh = 17 - Math.floor(i / 12)
  const mm = String(Math.floor(i / 6)).padStart(2, '0')
  const createdAt = `2026-05-14T${hh}:${mm}:00`
  const sentAt = status === 'success' ? `2026-05-14T${hh}:${mm}:05` : null
  const kind = i % 4 // 0=工控機建立 1=自補+已回報 2=自補+未回報 3=自補+遲到回報
  return {
    id: 200 - i,
    tracking_no: `SF${1234567000 + i}`,
    response_id: 9000000 + i,
    sort_channel: CHANNELS_SAMPLE[i % CHANNELS_SAMPLE.length],
    job_sticker: STICKERS_SAMPLE[i % STICKERS_SAMPLE.length],
    status,
    retry_count: status === 'failed' ? (i % 5) + 1 : 0,
    created_at: createdAt,
    sent_at: sentAt,
    source: kind === 0 ? 'ipc' : 'direct_print',
    ipc_reported_at:
      kind === 0 ? createdAt
      : kind === 1 ? `2026-05-14T${hh}:${mm}:03`
      : kind === 2 ? null
      : sentAt ? `2026-05-14T${hh}:${mm}:20` : null,
  }
})

/// 一筆佇列在「來源」欄要顯示什麼:
/// - 工控機有回報過(不論是它建立的、還是併進直印自補那筆)→ 代表它確實收到分揀通道
/// - 只有中介機自印 → 面單印出來了,但沒有工控機的確認
/// - 回報時間晚於推送時間 → 遲到(雲端那筆沒帶到確認,本機仍看得到)
const sourceInfo = row => {
  const reported = !!row.ipc_reported_at
  if (!reported) {
    return { color: 'warning', icon: 'tabler-printer', text: t('page.queue.source.selfPrintOnly') }
  }
  const late = row.sent_at && row.ipc_reported_at > row.sent_at
  return {
    color: late ? 'info' : 'success',
    icon: late ? 'tabler-clock-exclamation' : 'tabler-check',
    text: late ? t('page.queue.source.ipcReportedLate') : t('page.queue.source.ipcReported'),
  }
}

const load = async () => {
  if (!isTauriRuntime) {
    let result = MOCK_QUEUE
    if (queueStatus.value) result = result.filter(r => r.status === queueStatus.value)
    if (unreportedOnly.value) result = result.filter(r => !r.ipc_reported_at)
    if (searchKeyword.value) {
      const kw = searchKeyword.value.toLowerCase()
      result = result.filter(r =>
        (r.tracking_no || '').toLowerCase().includes(kw) ||
        (r.sort_channel || '').toLowerCase().includes(kw) ||
        (r.job_sticker || '').toLowerCase().includes(kw)
      )
    }
    mockTotal.value = result.length
    const start = (page.value - 1) * pageSize.value
    queueItems.value = result.slice(start, start + pageSize.value)
    return
  }
  loading.value = true
  errorMsg.value = ''
  try {
    queueItems.value = await queueList({
      status: queueStatus.value,
      keyword: searchKeyword.value.trim() || null,
      unreportedOnly: unreportedOnly.value === true,
      limit: pageSize.value,
      offset: (page.value - 1) * pageSize.value,
    })
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

const handleRetry = async () => {
  try {
    const n = await queueRetryFailed()
    flashMsg.value = t('page.queue.retryFlash', { n })
    setTimeout(() => (flashMsg.value = ''), 3500)
    await load()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}

const handlePurge = async () => {
  try {
    const n = await queuePurge({ status: 'success', olderThanDays: 7 })
    flashMsg.value = t('page.queue.purgeFlash', { n })
    setTimeout(() => (flashMsg.value = ''), 3500)
    await load()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}

watch([queueStatus, searchKeyword, unreportedOnly], () => { page.value = 1; load() })
watch(pageSize, () => { page.value = 1; load() })
watch(page, load)
let _timer = null
onMounted(() => { load(); if (isTauriRuntime) _timer = setInterval(load, 5000) })
onUnmounted(() => { clearInterval(_timer); _timer = null })

const statusColor = s => ({
  pending: 'warning', sending: 'info', success: 'success', failed: 'error',
}[s] || 'grey')

const formatDate = s => s ? s.replace('T', ' ').slice(0, 19) : ''
</script>

<style scoped lang="scss">
.queue-table {
  th,
  td {
    white-space: nowrap;
  }
}
</style>

<template>
  <div>
    <AppHeader :title="$t('page.queue.title')" :subtitle="$t('page.queue.subtitle')" icon="tabler-truck-loading">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="primary" :loading="loading" :disabled="!isTauriRuntime" @click="load">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
          </VBtn>
          <VBtn color="warning" :disabled="!isTauriRuntime" @click="handleRetry">
            <VIcon icon="tabler-rotate" size="16" class="me-1" />{{ $t('page.queue.retryFailed') }}
          </VBtn>
          <VBtn color="error" :disabled="!isTauriRuntime" @click="handlePurge">
            <VIcon icon="tabler-trash" size="16" class="me-1" />{{ $t('page.queue.purgeOld') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="load">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>{{ $t('common.reload') }}</VListItemTitle>
              </VListItem>
              <VListItem :disabled="!isTauriRuntime" @click="handleRetry">
                <template #prepend><VIcon icon="tabler-rotate" size="20" /></template>
                <VListItemTitle>{{ $t('page.queue.retryFailedItems') }}</VListItemTitle>
              </VListItem>
              <VListItem :disabled="!isTauriRuntime" @click="handlePurge">
                <template #prepend><VIcon icon="tabler-trash" size="20" /></template>
                <VListItemTitle>{{ $t('page.queue.purgeOld7d') }}</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      {{ $t('page.queue.browserAlert') }}
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <!-- 進階查詢 -->
    <VExpansionPanels v-model="searchOpen" class="mb-3 advanced-search">
      <VExpansionPanel>
        <VExpansionPanelTitle class="advanced-search__title">{{ $t('common.advancedSearch') }}</VExpansionPanelTitle>
        <VExpansionPanelText>
          <VRow no-gutters class="mx-n2">
            <VCol cols="12" sm="6" lg="4" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.eventLog.keyword') }}</label>
                <VTextField v-model="searchKeyword" :placeholder="$t('page.queue.keywordPlaceholder')" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
            <VCol cols="12" sm="6" lg="4" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.queue.col.source') }}</label>
                <VSelect v-model="unreportedOnly" :items="REPORTED_FILTERS" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
            <VCol cols="12" sm="6" lg="4" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.queue.col.status') }}</label>
                <VSelect v-model="queueStatus" :items="QUEUE_STATUSES" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
          </VRow>
          <div class="d-flex justify-center pt-4 pb-0">
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
        <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="mockTotal" header />
      </div>

      <VDivider />

      <VTable hover class="queue-table">
        <thead>
          <tr>
            <th class="text-center" style="width: 70px;">#</th>
            <th class="text-center" style="min-width: 150px;">{{ $t('page.queue.col.trackingNo') }}</th>
            <th class="text-center" style="width: 90px;">{{ $t('page.queue.col.channel') }}</th>
            <th class="text-center" style="min-width: 100px;">{{ $t('page.queue.col.sticker') }}</th>
            <th class="text-center" style="min-width: 150px;">{{ $t('page.queue.col.source') }}</th>
            <th class="text-center" style="width: 120px;">{{ $t('page.queue.col.responseId') }}</th>
            <th class="text-center" style="width: 90px;">{{ $t('page.queue.col.status') }}</th>
            <th class="text-center" style="width: 80px;">{{ $t('page.queue.col.retry') }}</th>
            <th class="text-center" style="width: 170px;">{{ $t('page.queue.col.createdAt') }}</th>
            <th class="text-center" style="width: 170px;">{{ $t('page.queue.col.sentAt') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!queueItems.length">
            <td colspan="10">
              <div class="py-2 d-flex align-center justify-center">
                <VIcon icon="tabler-alert-circle" size="20" class="me-1" />
                <span class="text-md">{{ $t('common.noResults') }}</span>
              </div>
            </td>
          </tr>
          <tr v-for="row in queueItems" :key="row.id">
            <td class="text-center">{{ row.id }}</td>
            <td class="text-center">{{ row.tracking_no }}</td>
            <td class="text-center">{{ row.sort_channel || '—' }}</td>
            <td class="text-center">{{ row.job_sticker || '—' }}</td>
            <td class="text-center">
              <VChip :color="sourceInfo(row).color" size="small" variant="tonal" label>
                <VIcon :icon="sourceInfo(row).icon" size="14" class="me-1" />{{ sourceInfo(row).text }}
              </VChip>
            </td>
            <td class="text-center text-disabled">{{ row.response_id ?? '—' }}</td>
            <td class="text-center"><span class="font-weight-medium" :class="`text-${statusColor(row.status)}`">{{ STATUS_LABELS[row.status] || row.status }}</span></td>
            <td class="text-center">{{ row.retry_count }}</td>
            <td class="text-center">{{ formatDate(row.created_at) }}</td>
            <td class="text-center">{{ formatDate(row.sent_at) }}</td>
          </tr>
        </tbody>
      </VTable>

      <VDivider />

      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="mockTotal" />
    </VCard>
  </div>
</template>
