<script setup>
import { provide, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTheme, useLocale } from 'vuetify'
import { VuetifyDateAdapter } from 'vuetify/date/adapters/vuetify'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart, HeatmapChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, VisualMapComponent } from 'echarts/components'
import VChart from 'vue-echarts'
import {
  printStatsSummary,
  printStatsDaily,
  printStatsHourly,
  printStatsByProvider,
  printStatsBySticker,
  printStatsByScanner,
  printStatsHeatmap,
  printStatsReprint,
  printStatsProviderSource,
  printStatsFailure,
  printStatsCompare,
  workSessionReset,
} from '@/api/tauri'
import { listen } from '@tauri-apps/api/event'
import AppHeader from '@/components/AppHeader.vue'

// echarts 按需引入 — 用到 line + heatmap + grid + tooltip + visualMap
use([CanvasRenderer, LineChart, HeatmapChart, GridComponent, TooltipComponent, VisualMapComponent])

const theme = useTheme()
const primaryColor = computed(() => theme.current.value.colors.primary)
const onSurfaceColor = computed(() => theme.current.value.colors['on-surface'])

const { t, locale } = useI18n()

// i18n locale → date adapter locale (Intl BCP-47)
const I18N_TO_DATE_LOCALE = { 'zh-Hant': 'zh-TW', 'vi-VN': 'vi-VN', en: 'en-US' }
const dateLocale = computed(() => I18N_TO_DATE_LOCALE[locale.value] || 'en-US')

// VDatePicker 內部用 useDate() → inject(DateOptionsSymbol) 重新建 instance,
// 所以要 provide 的是 DateOptions。先 hardcode 'zh-Hant' → 'zh-TW' 驗證 provide 有效。
const DateOptionsSymbol = Symbol.for('vuetify:date-options')
const customDateOptions = {
  adapter: VuetifyDateAdapter,
  locale: {
    'zh-Hant': 'zh-TW',
    'vi-VN': 'vi-VN',
    en: 'en-US',
  },
  formats: {},
}
provide(DateOptionsSymbol, customDateOptions)

// 根因:createVueI18nAdapter 拿的 i18n.global.locale ref 跟 useI18n().locale 沒同步,
// vuetify locale.current.value 一直停在 'en'(initialization race?),導致 createInstance
// 用 options.locale['en'] = 'en-US' 而不是 zh-TW。手動 watch i18n locale → vuetify
// locale.current 強制同步,配合上面 customDateOptions 中 'zh-Hant'/'vi-VN' 的 mapping,
// VDatePicker 重新 createInstance 時就會用到正確的 Intl locale。
const vuetifyLocale = useLocale()
watchEffect(() => {
  if (vuetifyLocale?.current && vuetifyLocale.current.value !== locale.value) {
    vuetifyLocale.current.value = locale.value
  }
})

// 物流商代碼 → 顯示字串(對齊 ScanPrintPage / AutoPrintPage)
const PROVIDER_LABEL_KEY = {
  '7': 'provider.7eleven',
  F: 'provider.family',
  O: 'provider.hilife',
  C: 'provider.tcat',
  H: 'provider.hct',
  P: 'provider.pelican',
  E: 'provider.sf',
  S: 'provider.shopeeOffline',
  A: 'provider.shopeeAuth',
}
const providerDisplay = code => {
  if (!code) return t('page.printStats.providerUnknown')
  const key = PROVIDER_LABEL_KEY[code]
  return key ? `${t(key)} (${code})` : code
}

const SOURCE_LABEL_KEY = {
  scan: 'page.printStats.sourceScan',
  auto: 'page.printStats.sourceAuto',
  ipc: 'page.printStats.sourceIpc',
}
const sourceDisplay = code => t(SOURCE_LABEL_KEY[code] || 'page.printStats.sourceUnknown')

// 日期區間:預設今天往前 6 天(含今天共 7 天)
const todayStr = () => {
  const d = new Date()
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${dd}`
}
const dateOffsetStr = days => {
  const d = new Date()
  d.setDate(d.getDate() + days)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${dd}`
}

const startDate = ref(dateOffsetStr(-6))
const endDate = ref(todayStr())

// 日期彈窗開關
const startMenu = ref(false)
const endMenu = ref(false)

// 字串 yyyy-mm-dd ↔ Date 互轉(VDatePicker 內部用 Date 物件)
const strToDate = s => {
  if (!s) return null
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}
const dateToStr = d => {
  if (!d) return ''
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${dd}`
}
const startDateObj = computed({
  get: () => strToDate(startDate.value),
  set: v => { startDate.value = dateToStr(v); startMenu.value = false },
})
const endDateObj = computed({
  get: () => strToDate(endDate.value),
  set: v => { endDate.value = dateToStr(v); endMenu.value = false },
})

const summary = ref(null)
const daily = ref([])
const hourly = ref([])
const providers = ref([])
const stickers = ref([])
const scanners = ref([])
const heatmap = ref([])
const reprint = ref([])
const providerSource = ref([])
const failure = ref(null)
const compare = ref([])
const loading = ref(false)
const errorMsg = ref('')
const resetDialog = ref(false)

// 平均每袋訂單數 = 區間內面單張數 / 分袋數;區間沒分袋時為 N/A
const avgOrdersPerPackage = computed(() => {
  if (!summary.value || !summary.value.packages_range_total) return null
  return (summary.value.range_total / summary.value.packages_range_total).toFixed(1)
})

const scannersTotal = computed(() => scanners.value.reduce((a, b) => a + b.count, 0))
const scannersMax = computed(() => scanners.value.reduce((a, b) => Math.max(a, b.count), 0) || 1)

// 物流商 × 來源:轉成「provider → { scan, auto, ipc, total }」便於 table 呈現
const providerSourceTable = computed(() => {
  const m = new Map()
  for (const cell of providerSource.value) {
    if (!m.has(cell.provider_code)) m.set(cell.provider_code, { scan: 0, auto: 0, ipc: 0, total: 0 })
    const row = m.get(cell.provider_code)
    row[cell.source] = (row[cell.source] || 0) + cell.count
    row.total += cell.count
  }
  return Array.from(m.entries())
    .map(([code, v]) => ({ provider_code: code, ...v }))
    .sort((a, b) => b.total - a.total)
})

// 重印分布:bucket 標籤(1=只印 1 次 / 2=2 次 / ≥3 合併)
const reprintBuckets = computed(() => {
  const single = reprint.value.find(b => b.print_times === 1)?.shipment_count || 0
  const twice = reprint.value.find(b => b.print_times === 2)?.shipment_count || 0
  const more = reprint.value.filter(b => b.print_times >= 3).reduce((a, b) => a + b.shipment_count, 0)
  return [
    { key: '1', label: t('page.printStats.reprintBucket1'), count: single },
    { key: '2', label: t('page.printStats.reprintBucket2'), count: twice },
    { key: '3+', label: t('page.printStats.reprintBucket3'), count: more },
  ]
})
const reprintTotal = computed(() => reprintBuckets.value.reduce((a, b) => a + b.count, 0))

// 熱力圖:轉成 echarts heatmap 格式 [hour, weekday, count] 並算 max
const heatmapData = computed(() => heatmap.value.map(c => [c.hour, c.weekday, c.count]))
const heatmapMax = computed(() => heatmap.value.reduce((a, b) => Math.max(a, b.count), 0) || 1)

const heatmapOption = computed(() => ({
  tooltip: {
    position: 'top',
    formatter: (params) => {
      const [hour, weekday, count] = params.value
      const wd = [t('page.printStats.wdSun'), t('page.printStats.wdMon'), t('page.printStats.wdTue'), t('page.printStats.wdWed'), t('page.printStats.wdThu'), t('page.printStats.wdFri'), t('page.printStats.wdSat')][weekday]
      return `${wd} ${String(hour).padStart(2, '0')}:00<br>${count}`
    },
  },
  grid: { left: 50, right: 16, top: 8, bottom: 24 },
  xAxis: {
    type: 'category',
    data: Array.from({ length: 24 }, (_, i) => String(i).padStart(2, '0')),
    splitArea: { show: true },
    axisLabel: { color: hexToRgba(onSurfaceColor.value, 0.6), fontSize: 10 },
  },
  yAxis: {
    type: 'category',
    data: [t('page.printStats.wdSun'), t('page.printStats.wdMon'), t('page.printStats.wdTue'), t('page.printStats.wdWed'), t('page.printStats.wdThu'), t('page.printStats.wdFri'), t('page.printStats.wdSat')],
    splitArea: { show: true },
    axisLabel: { color: hexToRgba(onSurfaceColor.value, 0.6), fontSize: 11 },
  },
  visualMap: {
    min: 0,
    max: heatmapMax.value,
    calculable: false,
    orient: 'horizontal',
    left: 'center',
    bottom: 0,
    show: false,
    inRange: { color: [hexToRgba(primaryColor.value, 0.08), primaryColor.value] },
  },
  series: [{
    type: 'heatmap',
    data: heatmapData.value,
    label: { show: false },
    emphasis: { itemStyle: { shadowBlur: 8, shadowColor: hexToRgba(primaryColor.value, 0.4) } },
  }],
}))

// 計算每日趨勢中的最大值(畫橫條圖用)
const dailyMax = computed(() => daily.value.reduce((a, b) => Math.max(a, b.count), 0) || 1)
const hourlyMax = computed(() => hourly.value.reduce((a, b) => Math.max(a, b.count), 0) || 1)
const providersMax = computed(() => providers.value.reduce((a, b) => Math.max(a, b.count), 0) || 1)
const stickersMax = computed(() => stickers.value.reduce((a, b) => Math.max(a, b.count), 0) || 1)
const providersTotal = computed(() => providers.value.reduce((a, b) => a + b.count, 0))
const stickersTotal = computed(() => stickers.value.reduce((a, b) => a + b.count, 0))

const pct = (n, max) => Math.round((n / max) * 100)
const sharePct = (n, total) => (total > 0 ? Math.round((n / total) * 100) : 0)

// hex (#RRGGBB) → rgba 字串(echarts 支援 'rgba(...)' 與 hex,但 hex8 顯卡不一定支援)
const hexToRgba = (hex, alpha) => {
  const h = hex.replace('#', '')
  const r = parseInt(h.slice(0, 2), 16)
  const g = parseInt(h.slice(2, 4), 16)
  const b = parseInt(h.slice(4, 6), 16)
  return `rgba(${r},${g},${b},${alpha})`
}

// 每日趨勢 line chart option(隨資料 / theme 變動自動 reactive)
const dailyOption = computed(() => ({
  grid: { top: 16, right: 16, bottom: 28, left: 40 },
  tooltip: {
    trigger: 'axis',
    backgroundColor: '#2F2B3D',
    borderWidth: 0,
    textStyle: { color: '#fff', fontSize: 12 },
    axisPointer: { lineStyle: { color: hexToRgba(onSurfaceColor.value, 0.15) } },
  },
  xAxis: {
    type: 'category',
    boundaryGap: false,
    data: daily.value.map(p => p.date.slice(5)),
    axisLine: { lineStyle: { color: hexToRgba(onSurfaceColor.value, 0.12) } },
    axisLabel: { color: hexToRgba(onSurfaceColor.value, 0.6), fontSize: 11 },
    axisTick: { show: false },
  },
  yAxis: {
    type: 'value',
    minInterval: 1,
    axisLine: { show: false },
    splitLine: { lineStyle: { color: hexToRgba(onSurfaceColor.value, 0.06) } },
    axisLabel: { color: hexToRgba(onSurfaceColor.value, 0.6), fontSize: 11 },
    axisTick: { show: false },
  },
  series: [{
    type: 'line',
    smooth: true,
    showSymbol: true,
    symbolSize: 6,
    data: daily.value.map(p => p.count),
    itemStyle: { color: primaryColor.value },
    lineStyle: { width: 2, color: primaryColor.value },
    areaStyle: {
      color: {
        type: 'linear',
        x: 0, y: 0, x2: 0, y2: 1,
        colorStops: [
          { offset: 0, color: hexToRgba(primaryColor.value, 0.28) },
          { offset: 1, color: hexToRgba(primaryColor.value, 0.02) },
        ],
      },
    },
  }],
}))

const reload = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const args = { startDate: startDate.value, endDate: endDate.value }
    const [s, d, h, p, st, sc, hm, rp, ps, fl, cmp] = await Promise.all([
      printStatsSummary(args),
      printStatsDaily(args),
      printStatsHourly(),
      printStatsByProvider(args),
      printStatsBySticker(args),
      printStatsByScanner(args),
      printStatsHeatmap(args),
      printStatsReprint(args),
      printStatsProviderSource(args),
      printStatsFailure(args),
      printStatsCompare(),
    ])
    summary.value = s
    daily.value = d
    hourly.value = h
    providers.value = p
    stickers.value = st
    scanners.value = sc
    heatmap.value = hm
    reprint.value = rp
    providerSource.value = ps
    failure.value = fl
    compare.value = cmp
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

const confirmReset = async () => {
  try {
    await workSessionReset()
    resetDialog.value = false
    await reload()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}

// 物流商代碼 → 顯示(複用 providerDisplay,但去掉括弧)
const providerShort = code => {
  if (!code) return t('page.printStats.providerUnknown')
  const key = PROVIDER_LABEL_KEY[code]
  return key ? t(key) : code
}

// 快選 toggle group:依當前 startDate/endDate 推算是否吻合某個快選
// null 代表使用者手動調整(自訂),三個 toggle 都不亮
const quickRange = computed({
  get() {
    const today = todayStr()
    if (endDate.value !== today) return null
    if (startDate.value === today) return 'today'
    if (startDate.value === dateOffsetStr(-6)) return '7d'
    if (startDate.value === dateOffsetStr(-29)) return '30d'
    return null
  },
  set(v) {
    // v=null 表示使用者點同一個 toggle 取消選取,維持原日期不變
    if (v === 'today') setRange(1)
    else if (v === '7d') setRange(7)
    else if (v === '30d') setRange(30)
  },
})

const setRange = days => {
  endDate.value = todayStr()
  startDate.value = dateOffsetStr(-(days - 1))
}

watch([startDate, endDate], () => {
  // 任一邊改了就 reload(避免使用者忘記按)
  reload()
})

let _unlistenStats = null
onMounted(async () => {
  await reload()
  try { _unlistenStats = await listen('print-stats-updated', reload) } catch {}
})
onUnmounted(() => { if (_unlistenStats) { _unlistenStats(); _unlistenStats = null } })
</script>

<template>
  <div>
    <AppHeader
      :title="$t('page.printStats.title')"
      :subtitle="$t('page.printStats.subtitle')"
      icon="tabler-chart-bar"
    >
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn
            color="warning"
            variant="tonal"
            prepend-icon="tabler-refresh-dot"
            @click="resetDialog = true"
          >
            {{ $t('page.printStats.resetBtn') }}
          </VBtn>
          <VBtn color="primary" :loading="loading" @click="reload">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem @click="resetDialog = true">
                <template #prepend><VIcon icon="tabler-refresh-dot" size="20" color="warning" /></template>
                <VListItemTitle>{{ $t('page.printStats.resetBtn') }}</VListItemTitle>
              </VListItem>
              <VListItem @click="reload">
                <template #prepend><VIcon icon="tabler-refresh" size="20" /></template>
                <VListItemTitle>{{ $t('common.reload') }}</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">
      {{ errorMsg }}
    </VAlert>

    <!-- 概況卡片:4 張等寬,「本場累計」主色描邊突出 + 重置按鈕 -->
    <VRow dense>
      <VCol cols="6" md="3">
        <VCard class="card-shadow kpi-card kpi-card--primary h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="primary" variant="flat" size="44">
              <VIcon icon="tabler-restore" />
            </VAvatar>
            <div class="flex-grow-1">
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.sinceReset') }}</div>
              <div class="text-h3 font-weight-bold text-primary">{{ summary?.since_reset ?? 0 }}</div>
              <div class="text-caption text-medium-emphasis mt-1">
                <VIcon icon="tabler-package" size="12" class="me-1" />{{ summary?.packages_since_reset ?? 0 }} {{ $t('page.printStats.packagesUnit') }}
              </div>
              <div class="text-caption text-medium-emphasis" style="font-size: 10px;">
                {{ $t('page.printStats.sinceLabel') }} {{ summary?.since_reset_at || '—' }}
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
      <VCol cols="6" md="3">
        <VCard class="card-shadow kpi-card h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="info" variant="tonal" size="40">
              <VIcon icon="tabler-clock-hour-4" size="20" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.past24h') }}</div>
              <div class="text-h5 font-weight-medium">{{ summary?.past_24h ?? 0 }}</div>
              <div class="text-caption text-medium-emphasis mt-1">
                <VIcon icon="tabler-package" size="12" class="me-1" />{{ summary?.packages_past_24h ?? 0 }} {{ $t('page.printStats.packagesUnit') }}
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
      <VCol cols="6" md="3">
        <VCard class="card-shadow kpi-card h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="success" variant="tonal" size="40">
              <VIcon icon="tabler-calendar-week" size="20" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.last7Days') }}</div>
              <div class="text-h5 font-weight-medium">{{ summary?.last_7_days ?? 0 }}</div>
              <div class="text-caption text-medium-emphasis mt-1">
                <VIcon icon="tabler-package" size="12" class="me-1" />{{ summary?.packages_last_7_days ?? 0 }} {{ $t('page.printStats.packagesUnit') }}
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
      <VCol cols="6" md="3">
        <VCard class="card-shadow kpi-card h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="warning" variant="tonal" size="40">
              <VIcon icon="tabler-calendar-month" size="20" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.last30Days') }}</div>
              <div class="text-h5 font-weight-medium">{{ summary?.last_30_days ?? 0 }}</div>
              <div class="text-caption text-medium-emphasis mt-1">
                <VIcon icon="tabler-package" size="12" class="me-1" />{{ summary?.packages_last_30_days ?? 0 }} {{ $t('page.printStats.packagesUnit') }}
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 區間選擇列:日期 picker + 快選 toggle / 區間總數 / 來源分布 -->
    <VCard class="mt-3 card-shadow">
      <VCardText>
        <!-- 第一行:日期區 / 快選按鈕 / 區間總數 各自不換行,整體允許 wrap 到下一行 -->
        <div class="d-flex flex-wrap align-center gap-x-4 gap-y-3">
          <div class="d-flex align-center gap-2 flex-nowrap">
            <VMenu v-model="startMenu" :close-on-content-click="false" location="bottom start">
              <template #activator="{ props: act }">
                <VTextField
                  v-bind="act"
                  :model-value="startDate"
                  :label="$t('page.printStats.startDate')"
                  density="compact"
                  hide-details
                  readonly
                  prepend-inner-icon="tabler-calendar"
                  style="inline-size: 170px;"
                />
              </template>
              <VDatePicker
                v-model="startDateObj"
                :max="endDateObj"
                :locale="dateLocale"
                show-adjacent-months
                hide-header
              />
            </VMenu>
            <span class="text-medium-emphasis">~</span>
            <VMenu v-model="endMenu" :close-on-content-click="false" location="bottom start">
              <template #activator="{ props: act }">
                <VTextField
                  v-bind="act"
                  :model-value="endDate"
                  :label="$t('page.printStats.endDate')"
                  density="compact"
                  hide-details
                  readonly
                  prepend-inner-icon="tabler-calendar"
                  style="inline-size: 170px;"
                />
              </template>
              <VDatePicker
                v-model="endDateObj"
                :min="startDateObj"
                :locale="dateLocale"
                show-adjacent-months
                hide-header
              />
            </VMenu>
          </div>
          <div class="d-flex gap-2 flex-nowrap">
            <VBtn
              size="default"
              :variant="quickRange === 'today' ? 'flat' : 'outlined'"
              :color="quickRange === 'today' ? 'primary' : 'default'"
              @click="setRange(1)"
            >
              {{ $t('page.printStats.todayBtn') }}
            </VBtn>
            <VBtn
              size="default"
              :variant="quickRange === '7d' ? 'flat' : 'outlined'"
              :color="quickRange === '7d' ? 'primary' : 'default'"
              @click="setRange(7)"
            >
              {{ $t('page.printStats.last7DaysBtn') }}
            </VBtn>
            <VBtn
              size="default"
              :variant="quickRange === '30d' ? 'flat' : 'outlined'"
              :color="quickRange === '30d' ? 'primary' : 'default'"
              @click="setRange(30)"
            >
              {{ $t('page.printStats.last30DaysBtn') }}
            </VBtn>
          </div>
          <VSpacer />
          <div class="d-flex align-center gap-3 flex-nowrap">
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.rangeTotal') }}</div>
              <div class="text-caption text-medium-emphasis">
                <VIcon icon="tabler-package" size="12" class="me-1" />{{ summary?.packages_range_total ?? 0 }} {{ $t('page.printStats.packagesUnit') }}
              </div>
            </div>
            <div class="text-h4 font-weight-bold text-primary">{{ summary?.range_total ?? 0 }}</div>
          </div>
        </div>

        <!-- 第二行:來源分布(無資料時整列隱藏) -->
        <template v-if="summary?.by_source?.length">
          <VDivider class="my-3" />
          <div class="d-flex flex-wrap align-center gap-2">
            <span class="text-caption text-medium-emphasis me-1">{{ $t('page.printStats.bySourceLabel') }}</span>
            <VChip
              v-for="s in summary.by_source"
              :key="s.source"
              size="small"
              variant="tonal"
              :color="s.source === 'scan' ? 'primary' : s.source === 'auto' ? 'success' : 'info'"
            >
              <VIcon
                :icon="s.source === 'scan' ? 'tabler-browser' : s.source === 'auto' ? 'tabler-cloud-cog' : 'tabler-device-desktop'"
                size="14"
                class="me-1"
              />
              {{ sourceDisplay(s.source) }} · {{ s.count }}<template v-if="s.package_count > 0"> · {{ s.package_count }} {{ $t('page.printStats.packagesUnit') }}</template>
            </VChip>
          </div>
        </template>
      </VCardText>
    </VCard>

    <!-- 每日趨勢 + 最新 4 小時 -->
    <VRow dense class="mt-3">
      <VCol cols="12" md="7">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-trending-up" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.dailyTrend') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.dailyTrendHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <VChart
              v-if="daily.length > 0"
              :option="dailyOption"
              autoresize
              style="block-size: 240px;"
            />
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" md="5">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-clock-hour-4" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.hourly') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.hourlyHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div class="stat-rows">
              <div v-for="p in hourly" :key="p.hour" class="stat-row" :class="{ 'stat-row--zero': p.count === 0 }">
                <div class="stat-row__label">{{ p.hour }}</div>
                <div class="stat-row__bar">
                  <div
                    v-if="p.count > 0"
                    class="stat-row__fill stat-row__fill--info"
                    :style="{ inlineSize: pct(p.count, hourlyMax) + '%' }"
                  />
                </div>
                <div class="stat-row__value">{{ p.count }}</div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 物流商分組 + 貼標人員分組 -->
    <VRow dense class="mt-3">
      <VCol cols="12" md="6">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="success" variant="tonal">
                <VIcon icon="tabler-truck-delivery" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.byProvider') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.byProviderHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-if="providers.length === 0" class="empty-state">
              <VIcon icon="tabler-truck-off" size="40" class="empty-state__icon" />
              <div class="empty-state__text">{{ $t('page.printStats.noProviderData') }}</div>
            </div>
            <div v-else class="stat-rows">
              <div v-for="p in providers" :key="p.provider_code" class="stat-row">
                <div class="stat-row__label stat-row__label--wide">{{ providerDisplay(p.provider_code) }}</div>
                <div class="stat-row__bar">
                  <div class="stat-row__fill stat-row__fill--success" :style="{ inlineSize: pct(p.count, providersMax) + '%' }" />
                </div>
                <div class="stat-row__value">
                  {{ p.count }}
                  <span class="text-caption text-medium-emphasis ms-1">({{ sharePct(p.count, providersTotal) }}%)</span>
                </div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" md="6">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="warning" variant="tonal">
                <VIcon icon="tabler-user-check" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.bySticker') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.byStickerHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-if="stickers.length === 0" class="empty-state">
              <VIcon icon="tabler-user-off" size="40" class="empty-state__icon" />
              <div class="empty-state__text">{{ $t('page.printStats.noStickerData') }}</div>
            </div>
            <div v-else class="stat-rows">
              <div v-for="p in stickers" :key="p.sticker_user" class="stat-row">
                <div class="stat-row__label stat-row__label--wide">{{ p.sticker_user }}</div>
                <div class="stat-row__bar">
                  <div class="stat-row__fill stat-row__fill--warning" :style="{ inlineSize: pct(p.count, stickersMax) + '%' }" />
                </div>
                <div class="stat-row__value">
                  {{ p.count }}
                  <span class="text-caption text-medium-emphasis ms-1">({{ sharePct(p.count, stickersTotal) }}%)</span>
                </div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 階段 2 - 操作人員排行 + 歷史比對 -->
    <VRow dense class="mt-3">
      <VCol cols="12" md="6">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-user-scan" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.byScanner') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.byScannerHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-if="scanners.length === 0" class="empty-state">
              <VIcon icon="tabler-user-off" size="40" class="empty-state__icon" />
              <div class="empty-state__text">{{ $t('page.printStats.noScannerData') }}</div>
            </div>
            <div v-else class="stat-rows">
              <div v-for="p in scanners" :key="p.scanner_user" class="stat-row">
                <div class="stat-row__label stat-row__label--wide">{{ p.scanner_user }}</div>
                <div class="stat-row__bar">
                  <div class="stat-row__fill" :style="{ inlineSize: pct(p.count, scannersMax) + '%' }" />
                </div>
                <div class="stat-row__value">
                  {{ p.count }}
                  <span class="text-caption text-medium-emphasis ms-1">({{ sharePct(p.count, scannersTotal) }}%)</span>
                </div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" md="6">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-trending-up" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.compareTitle') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.compareHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-for="c in compare" :key="c.label" class="compare-row">
              <div class="compare-row__label">
                {{ c.label === 'week' ? $t('page.printStats.compareWeek') : $t('page.printStats.compareMonth') }}
              </div>
              <div class="compare-row__nums">
                <span class="text-h5 font-weight-bold">{{ c.current }}</span>
                <span class="text-caption text-medium-emphasis ms-1">vs {{ c.previous }}</span>
              </div>
              <div class="compare-row__delta">
                <template v-if="c.delta_ratio === null">
                  <span class="text-caption text-medium-emphasis">—</span>
                </template>
                <template v-else>
                  <VIcon
                    :icon="c.delta_ratio >= 0 ? 'tabler-arrow-up-right' : 'tabler-arrow-down-right'"
                    :color="c.delta_ratio >= 0 ? 'success' : 'error'"
                    size="18"
                  />
                  <span :class="c.delta_ratio >= 0 ? 'text-success' : 'text-error'" class="font-weight-bold ms-1">
                    {{ (c.delta_ratio * 100).toFixed(1) }}%
                  </span>
                </template>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 階段 3-4 - 失敗率 + 平均每袋 + 重印分布 KPI 列 -->
    <VRow dense class="mt-3">
      <VCol cols="6" md="4">
        <VCard class="card-shadow kpi-card h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar :color="failure?.failure_rate > 0.05 ? 'error' : 'success'" variant="tonal" size="40">
              <VIcon :icon="failure?.failure_rate > 0.05 ? 'tabler-alert-triangle' : 'tabler-check'" size="20" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.failureRate') }}</div>
              <div class="text-h5 font-weight-medium">
                {{ failure ? ((failure.failure_rate || 0) * 100).toFixed(2) + '%' : '—' }}
              </div>
              <div class="text-caption text-medium-emphasis mt-1">
                {{ failure?.failure_count ?? 0 }} / {{ (failure?.failure_count ?? 0) + (failure?.success_count ?? 0) }}
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
      <VCol cols="6" md="4">
        <VCard class="card-shadow kpi-card h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="success" variant="tonal" size="40">
              <VIcon icon="tabler-packages" size="20" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.avgPerPackage') }}</div>
              <div class="text-h5 font-weight-medium">{{ avgOrdersPerPackage ?? '—' }}</div>
              <div class="text-caption text-medium-emphasis mt-1">{{ $t('page.printStats.avgPerPackageHint') }}</div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
      <VCol cols="12" md="4">
        <VCard class="card-shadow kpi-card h-100">
          <VCardItem class="pb-1">
            <template #prepend>
              <VAvatar color="warning" variant="tonal" size="36">
                <VIcon icon="tabler-repeat" size="18" />
              </VAvatar>
            </template>
            <VCardTitle class="text-body-1">{{ $t('page.printStats.reprintTitle') }}</VCardTitle>
          </VCardItem>
          <VCardText class="pt-0">
            <div v-for="b in reprintBuckets" :key="b.key" class="d-flex align-center justify-space-between mb-1">
              <span class="text-body-2">{{ b.label }}</span>
              <span class="text-body-2 font-weight-medium">{{ b.count }} <span class="text-caption text-medium-emphasis">({{ sharePct(b.count, reprintTotal) }}%)</span></span>
            </div>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 階段 3 - 失敗代碼分類 + 物流×來源 cross-tab -->
    <VRow dense class="mt-3">
      <VCol cols="12" md="4">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="error" variant="tonal">
                <VIcon icon="tabler-bug" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.failureBreakdown') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.failureBreakdownHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-if="!failure?.by_code?.length" class="empty-state">
              <VIcon icon="tabler-mood-smile" size="40" class="empty-state__icon" />
              <div class="empty-state__text">{{ $t('page.printStats.noFailure') }}</div>
            </div>
            <div v-else class="stat-rows">
              <div v-for="b in failure.by_code" :key="b.respond_code" class="stat-row">
                <div class="stat-row__label stat-row__label--wide">{{ b.respond_code }}</div>
                <div class="stat-row__bar">
                  <div
                    class="stat-row__fill"
                    style="background: rgb(var(--v-theme-error))"
                    :style="{ inlineSize: pct(b.count, failure.failure_count || 1) + '%' }"
                  />
                </div>
                <div class="stat-row__value">{{ b.count }}</div>
              </div>
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" md="8">
        <VCard class="card-shadow h-100">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-grid-pattern" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.providerSourceTitle') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.providerSourceHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText class="pa-0">
            <VTable density="comfortable">
              <thead>
                <tr>
                  <th>{{ $t('page.printStats.colProvider') }}</th>
                  <th class="text-end">{{ $t('page.printStats.sourceScan') }}</th>
                  <th class="text-end">{{ $t('page.printStats.sourceAuto') }}</th>
                  <th class="text-end">{{ $t('page.printStats.sourceIpc') }}</th>
                  <th class="text-end">{{ $t('page.printStats.colTotal') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="providerSourceTable.length === 0">
                  <td colspan="5" class="text-center text-medium-emphasis py-4">
                    {{ $t('page.printStats.noProviderData') }}
                  </td>
                </tr>
                <tr v-for="r in providerSourceTable" :key="r.provider_code">
                  <td>{{ providerShort(r.provider_code) }}</td>
                  <td class="text-end">{{ r.scan || '—' }}</td>
                  <td class="text-end">{{ r.auto || '—' }}</td>
                  <td class="text-end">{{ r.ipc || '—' }}</td>
                  <td class="text-end font-weight-bold">{{ r.total }}</td>
                </tr>
              </tbody>
            </VTable>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 階段 3 - 24h × 7d 熱力圖 -->
    <VRow dense class="mt-3">
      <VCol cols="12">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-temperature" />
              </VAvatar>
            </template>
            <VCardTitle>{{ $t('page.printStats.heatmapTitle') }}</VCardTitle>
            <VCardSubtitle>{{ $t('page.printStats.heatmapHint') }}</VCardSubtitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div v-if="heatmap.length === 0" class="empty-state">
              <VIcon icon="tabler-clock-off" size="40" class="empty-state__icon" />
              <div class="empty-state__text">{{ $t('page.printStats.noData') }}</div>
            </div>
            <VChart v-else :option="heatmapOption" autoresize style="height: 280px;" />
          </VCardText>
        </VCard>
      </VCol>
    </VRow>

    <!-- 重置確認對話框 -->
    <VDialog v-model="resetDialog" max-width="420">
      <VCard>
        <VCardTitle>{{ $t('page.printStats.resetDialogTitle') }}</VCardTitle>
        <VCardText>
          {{ $t('page.printStats.resetDialogBody') }}
          <div class="text-caption text-medium-emphasis mt-2">
            {{ $t('page.printStats.sinceLabel') }} {{ summary?.since_reset_at || '—' }}
          </div>
        </VCardText>
        <VCardActions>
          <VSpacer />
          <VBtn variant="text" @click="resetDialog = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="primary" variant="flat" @click="confirmReset">{{ $t('page.printStats.resetConfirm') }}</VBtn>
        </VCardActions>
      </VCard>
    </VDialog>
  </div>
</template>

<style lang="scss" scoped>
// KPI 卡:今日為主視覺(更高 elevation + 主色描邊)
.kpi-card {
  block-size: 100%;

  &--primary {
    border-block-start: 3px solid rgb(var(--v-theme-primary));
  }
}

.stat-rows {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.stat-row {
  display: grid;
  grid-template-columns: 64px 1fr 96px;
  align-items: center;
  gap: 8px;

  // 0 值列:整列降低對比度,讓有資料的列更突出
  &--zero {
    opacity: 0.45;
  }
}
.stat-row__label {
  font-family: 'Menlo', 'Consolas', monospace;
  font-size: 12px;
  color: rgba(var(--v-theme-on-surface), 0.7);
  text-align: end;

  &--wide {
    font-family: inherit;
    text-align: start;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
}
.stat-row__bar {
  block-size: 10px;
  background: rgba(var(--v-theme-on-surface), 0.05);
  border-radius: 5px;
  overflow: hidden;
}
.stat-row__fill {
  block-size: 100%;
  background: rgb(var(--v-theme-primary));
  border-radius: 5px;
  transition: inline-size 0.3s ease;

  &--info { background: rgb(var(--v-theme-info)); }
  &--success { background: rgb(var(--v-theme-success)); }
  &--warning { background: rgb(var(--v-theme-warning)); }
}
.stat-row__value {
  font-size: 13px;
  font-weight: 600;
  text-align: end;
  font-variant-numeric: tabular-nums;
}

// 空狀態:圖示 + 提示語,比一行「無資料」字串友善
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 32px 16px;
  gap: 8px;
}
.empty-state__icon {
  color: rgba(var(--v-theme-on-surface), 0.25);
}

// 歷史比對單列:label / 數字 / 箭頭三段式
.compare-row {
  display: grid;
  grid-template-columns: 80px 1fr auto;
  align-items: center;
  gap: 12px;
  padding-block: 8px;

  & + & {
    border-block-start: 1px dashed rgba(var(--v-theme-on-surface), 0.08);
  }
}
.compare-row__label {
  font-size: 13px;
  color: rgba(var(--v-theme-on-surface), 0.7);
}
.compare-row__nums {
  display: flex;
  align-items: baseline;
}
.compare-row__delta {
  display: flex;
  align-items: center;
  min-inline-size: 80px;
  justify-content: flex-end;
}
.empty-state__text {
  font-size: 13px;
  color: rgba(var(--v-theme-on-surface), 0.55);
  text-align: center;
}
</style>

<!--
  VDatePicker 被 VMenu Teleport 到 body,scoped CSS 無法套到 portal 外的 DOM。
  所以這段樣式必須是「非 scoped」全域,才能命中所有 VDatePicker。
  VBtn defaults color=primary 會讓每格日期/月份/年份文字繼承主色(綠),這裡強制
  把非選中按鈕的文字色改回 on-surface,維持可讀性。
-->
<style lang="scss">
// VBtn 套 `text-primary` class(其本身有 !important),要壓過必須同時 target
// .text-primary 把 specificity 拉高並一樣帶 !important
.v-date-picker-month .v-btn.text-primary:not(.v-btn--active),
.v-date-picker-month .v-btn:not(.v-btn--active),
.v-date-picker-months__content .v-btn.text-primary:not(.v-btn--active),
.v-date-picker-months__content .v-btn:not(.v-btn--active),
.v-date-picker-years__content .v-btn.text-primary:not(.v-btn--active),
.v-date-picker-years__content .v-btn:not(.v-btn--active),
.v-date-picker-header .v-btn.text-primary,
.v-date-picker-header .v-btn,
.v-date-picker-controls .v-btn.text-primary,
.v-date-picker-controls .v-btn {
  color: rgba(var(--v-theme-on-surface), 0.87) !important;
}

.v-date-picker-month__day--adjacent .v-btn.text-primary,
.v-date-picker-month__day--adjacent .v-btn {
  color: rgba(var(--v-theme-on-surface), 0.38) !important;
}

// VDatePicker 預設 picker__body 寬度 328px,但 day grid (7×40 + gap) 約需 341px,
// 導致最右側「六」column 被裁。改成自適應內容寬度,讓 7 個 column 都看得到
.v-date-picker > .v-picker__body,
.v-date-picker {
  width: auto !important;
  min-width: 328px;
}
</style>
