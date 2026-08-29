<script setup>
// 現場作業監控:與雲端網頁版 field-operation-monitor 同一份資料 ——
// 上半「清關 / 轉寄進度」看板(依報關日區間,最多 3 天)+ 依物流細分;
// 下半「每日貼單」作業人員統計(今日業務日 06:00 起算,桃園 / 台中分頁)。30 秒輪詢,視窗在背景時略過。
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppHeader from '@/components/AppHeader.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { fieldOperationMonitor } from '@/api/tauri'

const { t } = useI18n()
const POLL_INTERVAL = 30000

const todayStr = () => {
  const d = new Date()
  const p = n => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

const from = ref(todayStr())
const to = ref(todayStr())
const progress = ref({ bag_total: 0, bag_remaining: 0, parcel_total: 0, parcel_remaining: 0, printed: 0, storage_total: 0, storage_printed: 0, storage_remaining: 0, providers: [] })
const scopes = ref({})
const currentTab = ref('taoyuan')
const loading = ref(false)
const errorMsg = ref('')
const autoRefresh = ref(true)
const showProviders = ref(true)

const tabs = computed(() => [
  { value: 'taoyuan', label: t('page.fieldOperationMonitor.tabTaoyuan') },
  { value: 'taichung', label: t('page.fieldOperationMonitor.tabTaichung') },
])
const emptyScope = { business_date: '', updated_at: '', rows: [], total: { package_num: 0, order_num: 0 } }
const currentScope = computed(() => scopes.value[currentTab.value] || emptyScope)
// 作業人數只計真人,排除物流貓系統列
const operatorCount = computed(() => currentScope.value.rows.filter(r => !r.is_system).length)
const providerRows = computed(() => progress.value.providers || [])
const rangeLabel = computed(() => (from.value === to.value ? from.value : `${from.value} ~ ${to.value}`))

const fmt = n => Number(n || 0).toLocaleString('en-US')
const pct = (num, den) => (den > 0 ? (num / den) * 100 : 0)
const weekDays = ['日', '一', '二', '三', '四', '五', '六']
const formatBusinessDate = s => (s ? `${s.slice(5)} (${weekDays[new Date(s + 'T00:00:00').getDay()]})` : '-')

const load = async () => {
  if (loading.value) return
  loading.value = true
  errorMsg.value = ''
  try {
    const r = await fieldOperationMonitor(from.value, to.value)
    if (r?.progress) progress.value = r.progress
    if (r?.scopes) scopes.value = r.scopes
    // 雲端會把區間裁到上限,以它回的為準
    if (r?.progressRange?.from) from.value = r.progressRange.from
    if (r?.progressRange?.to) to.value = r.progressRange.to
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

let timer = null
const stopTimer = () => { if (timer) { clearInterval(timer); timer = null } }
const startTimer = () => {
  stopTimer()
  timer = setInterval(() => { if (document.hidden) return; load() }, POLL_INTERVAL)
}
watch(autoRefresh, on => (on ? startTimer() : stopTimer()))
onMounted(() => { load(); if (autoRefresh.value) startTimer() })
onBeforeUnmount(stopTimer)

// 日期區間設定對話框
const dlg = ref(false)
const dFrom = ref(from.value)
const dTo = ref(to.value)
const openDlg = () => { dFrom.value = from.value; dTo.value = to.value; dlg.value = true }
const applyDates = () => {
  if (dTo.value && dFrom.value && dTo.value < dFrom.value) dTo.value = dFrom.value
  from.value = dFrom.value
  to.value = dTo.value || dFrom.value
  dlg.value = false
  load()
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.fieldOperationMonitor.title')" :subtitle="$t('page.fieldOperationMonitor.subtitle')" icon="tabler-user-check">
      <template #actions>
        <VBtn color="primary" :loading="loading" @click="load">
          <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <!-- 清關 / 轉寄進度看板 -->
    <VCard class="mb-4">
      <VCardText>
        <div class="d-flex flex-wrap align-center justify-space-between ga-3 mb-3">
          <div class="d-flex align-center flex-wrap ga-2">
            <VIcon icon="tabler-clipboard-check" size="22" color="primary" />
            <span class="text-h6">{{ $t('page.fieldOperationMonitor.board') }}</span>
            <VChip size="small" variant="tonal" color="secondary">
              <VIcon icon="tabler-calendar" size="14" start />{{ rangeLabel }}
            </VChip>
            <span class="text-caption text-medium-emphasis d-flex align-center ga-1">
              <VIcon :icon="errorMsg ? 'tabler-alert-triangle' : 'tabler-refresh'" size="14" :class="{ 'text-error': errorMsg }" />
              {{ $t('page.fieldOperationMonitor.dataTime', { time: currentScope.updated_at || '-' }) }}
            </span>
          </div>
          <div class="d-flex align-center flex-wrap ga-2">
            <VSwitch v-model="autoRefresh" color="primary" density="compact" hide-details :label="$t('page.fieldOperationMonitor.autoRefresh')" />
            <VBtn variant="tonal" size="small" @click="openDlg">
              <VIcon icon="tabler-calendar-cog" size="18" class="me-1" />{{ $t('page.fieldOperationMonitor.setRange') }}
            </VBtn>
          </div>
        </div>

        <VRow>
          <VCol cols="12" sm="4">
            <div class="progress-tile">
              <div class="d-flex align-center ga-2 mb-1">
                <VIcon icon="tabler-packages" size="20" class="text-medium-emphasis" />
                <span class="text-subtitle-1 font-weight-medium">{{ $t('page.fieldOperationMonitor.clearanceBags') }}</span>
              </div>
              <div class="d-flex align-baseline ga-1">
                <span class="text-h3 font-weight-bold text-info">{{ fmt(progress.bag_remaining) }}</span>
                <span class="text-h5 text-disabled">/</span>
                <span class="text-h5 text-medium-emphasis">{{ fmt(progress.bag_total) }}</span>
              </div>
            </div>
          </VCol>
          <VCol cols="12" sm="4">
            <div class="progress-tile">
              <div class="d-flex align-center ga-2 mb-1">
                <VIcon icon="tabler-file-invoice" size="20" class="text-medium-emphasis" />
                <span class="text-subtitle-1 font-weight-medium">{{ $t('page.fieldOperationMonitor.clearanceParcels') }}</span>
              </div>
              <div class="d-flex align-baseline ga-1">
                <span class="text-h3 font-weight-bold text-warning">{{ fmt(progress.parcel_remaining) }}</span>
                <span class="text-h5 text-disabled">/</span>
                <span class="text-h5 text-medium-emphasis">{{ fmt(progress.parcel_total) }}</span>
              </div>
              <VProgressLinear :model-value="pct(progress.printed, progress.parcel_total)" color="success" height="6" rounded class="mt-2" />
            </div>
          </VCol>
          <VCol cols="12" sm="4">
            <div class="progress-tile">
              <div class="d-flex align-center ga-2 mb-1">
                <VIcon icon="tabler-truck-delivery" size="20" class="text-medium-emphasis" />
                <span class="text-subtitle-1 font-weight-medium">{{ $t('page.fieldOperationMonitor.storageParcels') }}</span>
              </div>
              <div class="d-flex align-baseline ga-1">
                <span class="text-h3 font-weight-bold text-primary">{{ fmt(progress.storage_remaining) }}</span>
                <span class="text-h5 text-disabled">/</span>
                <span class="text-h5 text-medium-emphasis">{{ fmt(progress.storage_total) }}</span>
              </div>
              <VProgressLinear :model-value="pct(progress.storage_printed, progress.storage_total)" color="success" height="6" rounded class="mt-2" />
            </div>
          </VCol>
        </VRow>
        <div class="text-caption text-disabled mt-2">
          <VIcon icon="tabler-info-circle" size="14" class="me-1" />{{ $t('page.fieldOperationMonitor.boardHint') }}
        </div>

        <!-- 依物流細分 -->
        <div class="mt-4">
          <div class="d-flex align-center justify-space-between flex-wrap ga-2 mb-2">
            <div class="d-flex align-center ga-2">
              <VIcon icon="tabler-truck" size="20" class="text-medium-emphasis" />
              <span class="text-subtitle-1 font-weight-medium">{{ $t('page.fieldOperationMonitor.byProvider') }}</span>
            </div>
            <VBtn variant="text" size="small" @click="showProviders = !showProviders">
              <VIcon :icon="showProviders ? 'tabler-chevron-up' : 'tabler-chevron-down'" size="18" class="me-1" />
              {{ showProviders ? $t('page.fieldOperationMonitor.collapse') : $t('page.fieldOperationMonitor.expand') }}
            </VBtn>
          </div>
          <VExpandTransition>
            <div v-show="showProviders">
              <VTable density="compact" hover>
                <thead>
                  <tr>
                    <th class="text-center">{{ $t('page.fieldOperationMonitor.colProvider') }}</th>
                    <th class="text-center">{{ $t('page.fieldOperationMonitor.colBags') }}</th>
                    <th class="text-center">{{ $t('page.fieldOperationMonitor.colClearance') }}</th>
                    <th class="text-center">{{ $t('page.fieldOperationMonitor.colStorage') }}</th>
                  </tr>
                </thead>
                <tbody>
                  <template v-if="providerRows.length">
                    <tr v-for="row in providerRows" :key="row.name">
                      <td class="text-center font-weight-medium">{{ row.name }}</td>
                      <td class="text-center"><span class="font-weight-bold text-info">{{ fmt(row.bag_remaining) }}</span><span class="text-disabled mx-1">/</span><span class="text-medium-emphasis">{{ fmt(row.bag_total) }}</span></td>
                      <td class="text-center"><span class="font-weight-bold text-warning">{{ fmt(row.clearance_remaining) }}</span><span class="text-disabled mx-1">/</span><span class="text-medium-emphasis">{{ fmt(row.clearance_total) }}</span></td>
                      <td class="text-center"><span class="font-weight-bold text-primary">{{ fmt(row.storage_remaining) }}</span><span class="text-disabled mx-1">/</span><span class="text-medium-emphasis">{{ fmt(row.storage_total) }}</span></td>
                    </tr>
                  </template>
                  <tr v-else>
                    <td colspan="4">
                      <div class="py-4 d-flex align-center justify-center text-medium-emphasis">
                        <VIcon icon="tabler-alert-circle" size="20" class="me-1" />{{ $t('page.fieldOperationMonitor.noProviderRows') }}
                      </div>
                    </td>
                  </tr>
                </tbody>
              </VTable>
              <div class="text-caption text-disabled mt-2">
                <VIcon icon="tabler-info-circle" size="14" class="me-1" />{{ $t('page.fieldOperationMonitor.providerHint') }}
              </div>
            </div>
          </VExpandTransition>
        </div>
      </VCardText>
    </VCard>

    <!-- 每日貼單(作業人員維度)-->
    <div class="d-flex align-center ga-2 mb-2">
      <VIcon icon="tabler-users" size="22" color="primary" />
      <span class="text-h6">{{ $t('page.fieldOperationMonitor.daily') }}</span>
      <VChip size="small" variant="tonal" color="secondary">
        <VIcon icon="tabler-calendar" size="14" start />{{ $t('page.fieldOperationMonitor.businessDay', { date: formatBusinessDate(currentScope.business_date) }) }}
      </VChip>
    </div>

    <VRow class="mb-1">
      <VCol cols="12" sm="4">
        <VCard variant="tonal" color="primary" class="pa-4 d-flex align-center ga-3">
          <VIcon icon="tabler-printer" size="36" />
          <div>
            <div class="text-h5 font-weight-bold">{{ fmt(operatorCount) }}</div>
            <div class="text-body-2">{{ $t('page.fieldOperationMonitor.operators') }}</div>
          </div>
        </VCard>
      </VCol>
      <VCol cols="12" sm="4">
        <VCard variant="tonal" color="info" class="pa-4 d-flex align-center ga-3">
          <VIcon icon="tabler-packages" size="36" />
          <div>
            <div class="text-h5 font-weight-bold">{{ fmt(currentScope.total.package_num) }}</div>
            <div class="text-body-2">{{ $t('page.fieldOperationMonitor.stickerBags') }}</div>
          </div>
        </VCard>
      </VCol>
      <VCol cols="12" sm="4">
        <VCard variant="tonal" color="success" class="pa-4 d-flex align-center ga-3">
          <VIcon icon="tabler-file-invoice" size="36" />
          <div>
            <div class="text-h5 font-weight-bold">{{ fmt(currentScope.total.order_num) }}</div>
            <div class="text-body-2">{{ $t('page.fieldOperationMonitor.stickerOrders') }}</div>
          </div>
        </VCard>
      </VCol>
    </VRow>

    <VTabs v-model="currentTab" grow hide-slider class="bookmark-tabs">
      <VTab v-for="tab in tabs" :key="tab.value" :value="tab.value">{{ tab.label }}</VTab>
    </VTabs>
    <VCard class="bookmark-card">
      <VTable hover>
        <thead>
          <tr>
            <th class="text-center" style="min-width: 160px;">{{ $t('page.fieldOperationMonitor.colOperator') }}</th>
            <th class="text-center" style="width: 120px;">{{ $t('page.fieldOperationMonitor.colMinTime') }}</th>
            <th class="text-center" style="width: 120px;">{{ $t('page.fieldOperationMonitor.colMaxTime') }}</th>
            <th class="text-center" style="width: 120px;">{{ $t('page.fieldOperationMonitor.colPackageNum') }}</th>
            <th class="text-center" style="width: 120px;">{{ $t('page.fieldOperationMonitor.colOrderNum') }}</th>
          </tr>
        </thead>
        <tbody>
          <template v-if="currentScope.rows.length">
            <tr v-for="(row, i) in currentScope.rows" :key="`${currentTab}-${i}`">
              <td class="text-center font-weight-medium">{{ row.name }}</td>
              <td class="text-center">{{ row.min_time || '-' }}</td>
              <td class="text-center">{{ row.max_time || '-' }}</td>
              <td class="text-center">{{ fmt(row.package_num) }}</td>
              <td class="text-center">{{ fmt(row.order_num) }}</td>
            </tr>
            <tr class="total-row">
              <td class="text-center font-weight-bold" colspan="3">{{ $t('page.fieldOperationMonitor.dedupTotal') }}</td>
              <td class="text-center font-weight-bold text-primary">{{ fmt(currentScope.total.package_num) }}</td>
              <td class="text-center font-weight-bold text-primary">{{ fmt(currentScope.total.order_num) }}</td>
            </tr>
          </template>
          <tr v-else>
            <td colspan="5">
              <div class="py-4 d-flex align-center justify-center text-medium-emphasis">
                <VIcon icon="tabler-alert-circle" size="20" class="me-1" />{{ $t('page.fieldOperationMonitor.noRows') }}
              </div>
            </td>
          </tr>
        </tbody>
      </VTable>
    </VCard>

    <!-- 日期區間設定 -->
    <VDialog v-model="dlg" max-width="420">
      <VCard>
        <VCardTitle class="text-body-1">{{ $t('page.fieldOperationMonitor.setRange') }}</VCardTitle>
        <VCardText>
          <div class="text-caption text-medium-emphasis mb-3">{{ $t('page.fieldOperationMonitor.rangeHint') }}</div>
          <div class="d-flex align-center ga-2">
            <AppDatePicker v-model="dFrom" density="compact" />
            <span class="text-disabled">~</span>
            <AppDatePicker v-model="dTo" density="compact" />
          </div>
        </VCardText>
        <VCardActions class="px-4 pb-3">
          <VSpacer />
          <VBtn variant="text" @click="dlg = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="primary" variant="flat" :loading="loading" @click="applyDates">{{ $t('common.search') }}</VBtn>
        </VCardActions>
      </VCard>
    </VDialog>
  </div>
</template>

<style scoped lang="scss">
.progress-tile {
  padding: 12px 16px;
  border: 1px solid rgba(var(--v-border-color), var(--v-border-opacity));
  border-radius: 8px;
  block-size: 100%;
}
.total-row td { background: rgba(var(--v-theme-primary), 0.06); }
</style>
