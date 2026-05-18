<script setup>
import { parcelQueryLogList } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import TablePagination from '@/components/TablePagination.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const searchKeyword = ref('')
const items = ref([])
const total = ref(0)
const loading = ref(false)
const errorMsg = ref('')

const page = ref(1)
const pageSize = ref(25)
const PAGE_SIZES = [25, 50, 100, 200]
const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)))

const searchOpen = ref(0)

const resetSearch = () => { searchKeyword.value = '' }

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const resp = await parcelQueryLogList({
      keyword: searchKeyword.value || null,
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

watch(searchKeyword, () => { page.value = 1 })
watch(pageSize, () => { page.value = 1; load() })
watch(page, load)
onMounted(load)

const formatDate = s => s ? s.replace('T', ' ').slice(0, 19) : ''
</script>

<style scoped lang="scss">
.parcel-log-table {
  th,
  td {
    white-space: nowrap;
  }
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
            <VCol cols="12" class="px-2 py-1">
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
            <th class="text-center" style="min-width: 220px;">{{ $t('page.parcelQueryLog.col.labelKey') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!items.length">
            <td colspan="8">
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
            <td class="text-center"><code class="text-caption">{{ row.label_key || '—' }}</code></td>
          </tr>
        </tbody>
      </VTable>

      <VDivider />

      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" />
    </VCard>
  </div>
</template>
