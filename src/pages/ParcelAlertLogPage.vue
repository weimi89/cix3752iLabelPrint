<script setup>
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'
import AppHeader from '@/components/AppHeader.vue'
import TablePagination from '@/components/TablePagination.vue'
import { parcelAlertList } from '@/api/tauri'

const { t } = useI18n()
const items = ref([])
const total = ref(0)
const loading = ref(false)
const errorMsg = ref('')

// 雲端查件異常類別名稱:單一來源走 i18n parcelAlert.*(雙語 + 與 Flutter / 雲端 canonical 一致),
// **不可改用 inline 常數**:那會寫死中文並與 i18n 漂移,類別名容易變成語意相反的錯誤說法
const kindLabel = k => t(`parcelAlert.${k}`)
const kindColor = k =>
  k === 'store_closed' || k === 'unconfirmed' ? 'warning'
    : k === 'not_found' ? 'secondary' : 'error'

// 類別下拉:與後端寫入 parcel_alert.kind 的 canonical 值一致(server/mod.rs classify_parcel_alert)
const KINDS = ['store_closed', 'unconfirmed', 'not_found', 'not_proxy', 'not_forward', 'unauthorized', 'error']
const KIND_ITEMS = computed(() => [
  { title: t('common.all'), value: null },
  ...KINDS.map(k => ({ title: kindLabel(k), value: k })),
])

const searchKeyword = ref('')
const searchKind = ref(null)
const searchOpen = ref(0)
const page = ref(1)
const pageSize = ref(25)

const resetSearch = () => { searchKeyword.value = ''; searchKind.value = null }

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const resp = await parcelAlertList({
      keyword: searchKeyword.value || null,
      kind: searchKind.value,
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

watch([searchKeyword, searchKind], () => { page.value = 1 })
watch(pageSize, () => { page.value = 1; load() })
watch(page, load)

let unlisten = null
let _reloadTimer = null
// 去抖:連續異常(整袋門市關轉)合併為單次 reload,避免每筆都重查 + 重繪
const scheduleReload = () => {
  if (_reloadTimer) return
  _reloadTimer = setTimeout(() => { _reloadTimer = null; load() }, 400)
}
let _disposed = false
onMounted(async () => {
  await load()
  // 即時:後端 emit parcel-alert 時刷新清單。await load 期間可能已切頁,已卸載則立刻解除避免殘留
  try {
    const un = await listen('parcel-alert', scheduleReload)
    if (_disposed) un(); else unlisten = un
  } catch (e) {
    console.warn('listen parcel-alert 失敗', e)
  }
})
onBeforeUnmount(() => {
  _disposed = true
  if (unlisten) { unlisten(); unlisten = null }
  if (_reloadTimer) { clearTimeout(_reloadTimer); _reloadTimer = null }
})

const hasFilter = computed(() => !!searchKeyword.value || !!searchKind.value)
const empty = computed(() => !loading.value && items.value.length === 0)
</script>

<template>
  <div>
    <AppHeader :title="$t('page.alertLog.title')" :subtitle="$t('page.alertLog.subtitle')" icon="tabler-alert-triangle">
      <template #actions>
        <VBtn color="primary" :loading="loading" @click="load">
          <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
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
            <VCol cols="12" md="8" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.alertLog.keyword') }}</label>
                <VTextField
                  v-model="searchKeyword"
                  :placeholder="$t('page.alertLog.keywordPlaceholder')"
                  density="compact"
                  hide-details
                  variant="outlined"
                  clearable
                  @keyup.enter="load"
                />
              </div>
            </VCol>
            <VCol cols="12" md="4" class="px-2 py-1">
              <div class="search-field">
                <label>{{ $t('page.alertLog.col.kind') }}</label>
                <VSelect
                  v-model="searchKind"
                  :items="KIND_ITEMS"
                  density="compact"
                  hide-details
                  variant="outlined"
                />
              </div>
            </VCol>
          </VRow>
          <div class="d-flex justify-center pt-4 pb-0 ga-2">
            <VBtn variant="text" color="default" @click="resetSearch(); load()">
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

      <VTable v-if="!empty" density="comfortable">
        <thead>
          <tr>
            <th>{{ $t('page.alertLog.col.time') }}</th>
            <th>{{ $t('page.alertLog.col.kind') }}</th>
            <th>{{ $t('page.alertLog.col.queryNo') }}</th>
            <th>{{ $t('page.alertLog.col.shippingNo') }}</th>
            <th>{{ $t('page.alertLog.col.channel') }}</th>
            <th>{{ $t('page.alertLog.col.message') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="a in items" :key="a.id">
            <td class="text-no-wrap text-medium-emphasis">{{ a.created_at }}</td>
            <td><VChip :color="kindColor(a.kind)" size="small" variant="tonal">{{ kindLabel(a.kind) }}</VChip></td>
            <td class="font-weight-medium">{{ a.query_no || '—' }}</td>
            <td class="font-weight-medium">{{ a.shipping_no || '—' }}</td>
            <td class="text-center">{{ a.channel_code || '—' }}</td>
            <td class="text-medium-emphasis">{{ a.message || '—' }}</td>
          </tr>
        </tbody>
      </VTable>

      <div v-else class="d-flex flex-column align-center justify-center pa-10 text-medium-emphasis">
        <template v-if="hasFilter">
          <VIcon icon="tabler-alert-circle" size="48" class="mb-2" />
          {{ $t('common.noResults') }}
        </template>
        <template v-else>
          <VIcon icon="tabler-circle-check" size="48" color="success" class="mb-2" />
          {{ $t('page.alertLog.empty') }}
        </template>
      </div>

      <template v-if="total > pageSize">
        <VDivider />
        <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" />
      </template>
    </VCard>
  </div>
</template>
