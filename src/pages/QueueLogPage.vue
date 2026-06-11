<script setup>
import { queueList, queueRetryFailed, queuePurge } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import TablePagination from '@/components/TablePagination.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const queueStatus = ref(null)
const searchKeyword = ref('')
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

const CHANNELS_SAMPLE = ['A1', 'A2', 'B1', 'B2', 'C1']
const STICKERS_SAMPLE = ['小美', '阿傑', '阿宏', '貓宅']
const STATUSES_SAMPLE = ['success', 'success', 'success', 'pending', 'sending', 'failed']

// 瀏覽器預覽模式 mock 資料(80 筆,展示分頁)
const MOCK_QUEUE = Array.from({ length: 80 }, (_, i) => {
  const status = STATUSES_SAMPLE[i % STATUSES_SAMPLE.length]
  return {
    id: 200 - i,
    tracking_no: `SF${1234567000 + i}`,
    response_id: 9000000 + i,
    sort_channel: CHANNELS_SAMPLE[i % CHANNELS_SAMPLE.length],
    job_sticker: STICKERS_SAMPLE[i % STICKERS_SAMPLE.length],
    status,
    retry_count: status === 'failed' ? (i % 5) + 1 : 0,
    created_at: `2026-05-14T${17 - Math.floor(i / 12)}:${String(Math.floor(i / 6)).padStart(2, '0')}:00`,
    sent_at: status === 'success' ? `2026-05-14T${17 - Math.floor(i / 12)}:${String(Math.floor(i / 6)).padStart(2, '0')}:05` : null,
  }
})

const load = async () => {
  if (!isTauriRuntime) {
    let result = MOCK_QUEUE
    if (queueStatus.value) result = result.filter(r => r.status === queueStatus.value)
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

watch([queueStatus, searchKeyword], () => { page.value = 1; load() })
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
            <VCol cols="12" sm="6" lg="6" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.eventLog.keyword') }}</label>
                <VTextField v-model="searchKeyword" :placeholder="$t('page.queue.keywordPlaceholder')" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
            <VCol cols="12" sm="6" lg="6" class="px-2 py-1">
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
            <th class="text-center" style="width: 120px;">{{ $t('page.queue.col.responseId') }}</th>
            <th class="text-center" style="width: 90px;">{{ $t('page.queue.col.status') }}</th>
            <th class="text-center" style="width: 80px;">{{ $t('page.queue.col.retry') }}</th>
            <th class="text-center" style="width: 170px;">{{ $t('page.queue.col.createdAt') }}</th>
            <th class="text-center" style="width: 170px;">{{ $t('page.queue.col.sentAt') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!queueItems.length">
            <td colspan="9">
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
