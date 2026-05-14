<script setup>
import { eventLogList } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import TablePagination from '@/components/TablePagination.vue'

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const eventLevel = ref(null)
const eventCategory = ref(null)
const searchKeyword = ref('')
const events = ref([])
const loading = ref(false)
const errorMsg = ref('')

const page = ref(1)
const pageSize = ref(25)
const PAGE_SIZES = [25, 50, 100, 200]

const searchOpen = ref(0) // 預設展開進階查詢

const resetSearch = () => {
  eventLevel.value = null
  eventCategory.value = null
  searchKeyword.value = ''
}

const LEVELS = [
  { title: '全部', value: null },
  { title: 'info', value: 'info' },
  { title: 'warn', value: 'warn' },
  { title: 'error', value: 'error' },
]
const CATEGORIES = [
  { title: '全部', value: null },
  { title: 'server', value: 'server' },
  { title: 'cloud', value: 'cloud' },
  { title: 'cache', value: 'cache' },
  { title: 'queue', value: 'queue' },
  { title: 'printer', value: 'printer' },
]

// 瀏覽器預覽模式 mock 資料(60 筆,展示分頁)
const TEMPLATES = [
  { level: 'info', category: 'server', action: 'parcel_query', msg: 'GET /api/parcel/{TN} → 200 (label_source=local)' },
  { level: 'info', category: 'queue', action: 'report_received', msg: 'POST /api/report tracking_no={TN} → 寫入 queue' },
  { level: 'info', category: 'queue', action: 'push_success', msg: 'Queue 已成功送達雲端 (tracking={TN})' },
  { level: 'info', category: 'cache', action: 'prefetch_done', msg: '補下載 labels/2026/05/{TN}.png (12.4 KB)' },
  { level: 'info', category: 'printer', action: 'print_success', msg: '送印 7-ELEVEN 訂單 {TN} 至 EPSON_L6190' },
  { level: 'warn', category: 'queue', action: 'retry_unauthorized', msg: 'Queue 推送雲端失敗 (401),保持 pending' },
  { level: 'warn', category: 'cache', action: 'eviction', msg: '快取容量達上限 500MB,LRU 淘汰 8 筆面單' },
  { level: 'error', category: 'printer', action: 'print_failed', msg: '送印失敗:找不到印表機 mock_zebra_gk420' },
  { level: 'info', category: 'cloud', action: 'login_success', msg: '雲端 API 登入成功' },
  { level: 'info', category: 'server', action: 'http_server_started', msg: '本地 axum server 已綁定 0.0.0.0:18080' },
]
const MOCK_EVENTS = Array.from({ length: 60 }, (_, i) => {
  const t = TEMPLATES[i % TEMPLATES.length]
  const tn = `SF${1234567000 + i}`
  const m = String(Math.floor(i / 6)).padStart(2, '0')
  return {
    id: 60 - i,
    level: t.level,
    category: t.category,
    action: t.action,
    message: t.msg.replace('{TN}', tn),
    created_at: `2026-05-14T${17 - Math.floor(i / 12)}:${m}:00`,
  }
})

const load = async () => {
  if (!isTauriRuntime) {
    let result = MOCK_EVENTS
    if (eventLevel.value) result = result.filter(e => e.level === eventLevel.value)
    if (eventCategory.value) result = result.filter(e => e.category === eventCategory.value)
    if (searchKeyword.value) {
      const kw = searchKeyword.value.toLowerCase()
      result = result.filter(e =>
        (e.message || '').toLowerCase().includes(kw) ||
        (e.action || '').toLowerCase().includes(kw)
      )
    }
    const start = (page.value - 1) * pageSize.value
    events.value = result.slice(start, start + pageSize.value)
    mockTotal.value = result.length
    return
  }
  loading.value = true
  errorMsg.value = ''
  try {
    events.value = await eventLogList({
      level: eventLevel.value,
      category: eventCategory.value,
      limit: pageSize.value,
      offset: (page.value - 1) * pageSize.value,
    })
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

const mockTotal = ref(0)
const totalPages = computed(() => Math.max(1, Math.ceil(mockTotal.value / pageSize.value)))

watch([eventLevel, eventCategory, searchKeyword], () => { page.value = 1; load() })
watch(pageSize, () => { page.value = 1; load() })
watch(page, load)
onMounted(load)

const levelColor = level => ({ info: 'info', warn: 'warning', error: 'error' }[level] || 'grey')
const formatDate = s => s ? s.replace('T', ' ').slice(0, 19) : ''
</script>

<style scoped lang="scss">
.event-table {
  th,
  td {
    white-space: nowrap;
  }

  // 訊息欄位允許 ellipsis,不要 nowrap
  td:last-child {
    white-space: normal;
    word-break: break-all;
  }
}
</style>

<template>
  <div>
    <AppHeader title="事件記錄" subtitle="系統 / 雲端 / 快取 / Queue / 印表機事件" icon="tabler-bell-ringing">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="primary" :loading="loading" :disabled="!isTauriRuntime" @click="load">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />重新載入
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="load">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>重新載入</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      瀏覽器預覽模式 — 實機請於桌面 App 內開啟,系統會自動載入 SQLite 內的 event_log 紀錄。
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <!-- 進階查詢 -->
    <VExpansionPanels v-model="searchOpen" class="mb-3 advanced-search">
      <VExpansionPanel>
        <VExpansionPanelTitle class="advanced-search__title">進階查詢</VExpansionPanelTitle>
        <VExpansionPanelText>
          <VRow no-gutters class="mx-n2">
            <VCol cols="12" sm="6" lg="4" class="px-2 py-1">
              <div class="search-field">
                <label>關鍵字</label>
                <VTextField v-model="searchKeyword" placeholder="訊息或動作" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
            <VCol cols="12" sm="6" lg="4" class="px-2 py-1">
              <div class="search-field">
                <label>等級</label>
                <VSelect v-model="eventLevel" :items="LEVELS" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
            <VCol cols="12" sm="6" lg="4" class="px-2 py-1">
              <div class="search-field">
                <label>類別</label>
                <VSelect v-model="eventCategory" :items="CATEGORIES" density="compact" hide-details variant="outlined" />
              </div>
            </VCol>
          </VRow>
          <div class="d-flex justify-center pt-4">
            <VBtn variant="elevated" color="primary" @click="load">
              <VIcon icon="tabler-database-search" size="18" class="me-1" />查詢
            </VBtn>
          </div>
        </VExpansionPanelText>
      </VExpansionPanel>
    </VExpansionPanels>

    <VCard>
      <!-- 頂部 header 分頁 -->
      <div class="d-flex align-center ga-3 px-4 py-1">
        <VSpacer />
        <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="mockTotal" header />
      </div>

      <VDivider />

      <VTable hover class="event-table">
        <thead>
          <tr>
            <th class="text-center" style="width: 170px;">時間</th>
            <th class="text-center" style="width: 80px;">等級</th>
            <th class="text-center" style="width: 100px;">類別</th>
            <th class="text-center" style="width: 170px;">動作</th>
            <th style="min-width: 200px;">訊息</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!events.length">
            <td colspan="5">
              <div class="py-2 d-flex align-center justify-center">
                <VIcon icon="tabler-alert-circle" size="20" class="me-1" />
                <span class="text-md">查無資料</span>
              </div>
            </td>
          </tr>
          <tr v-for="ev in events" :key="ev.id">
            <td class="text-center">{{ formatDate(ev.created_at) }}</td>
            <td class="text-center"><span class="font-weight-medium" :class="`text-${levelColor(ev.level)}`">{{ ev.level }}</span></td>
            <td class="text-center">{{ ev.category }}</td>
            <td class="text-center"><code>{{ ev.action }}</code></td>
            <td>{{ ev.message }}</td>
          </tr>
        </tbody>
      </VTable>

      <VDivider />

      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="mockTotal" />
    </VCard>
  </div>
</template>
