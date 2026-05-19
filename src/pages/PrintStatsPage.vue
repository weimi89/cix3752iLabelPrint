<script setup>
import { provide } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTheme, useLocale } from 'vuetify'
import { VuetifyDateAdapter } from 'vuetify/date/adapters/vuetify'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart } from 'echarts/charts'
import { GridComponent, TooltipComponent } from 'echarts/components'
import VChart from 'vue-echarts'
import {
  printStatsSummary,
  printStatsDaily,
  printStatsHourly,
  printStatsByProvider,
  printStatsBySticker,
} from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

// echarts 按需引入 — 只用到 line + grid + tooltip,bundle 比全量小很多
use([CanvasRenderer, LineChart, GridComponent, TooltipComponent])

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
const loading = ref(false)
const errorMsg = ref('')

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
    const [s, d, h, p, st] = await Promise.all([
      printStatsSummary(args),
      printStatsDaily(args),
      printStatsHourly(),
      printStatsByProvider(args),
      printStatsBySticker(args),
    ])
    summary.value = s
    daily.value = d
    hourly.value = h
    providers.value = p
    stickers.value = st
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
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

onMounted(reload)
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
          <VBtn color="primary" :loading="loading" @click="reload">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
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

    <!-- 概況卡片:4 張等寬,「今日」用 text-h3 + 主色描邊突出 -->
    <VRow dense>
      <VCol cols="6" md="3">
        <VCard class="card-shadow kpi-card kpi-card--primary h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="primary" variant="flat" size="44">
              <VIcon icon="tabler-calendar-event" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.today') }}</div>
              <div class="text-h3 font-weight-bold text-primary">{{ summary?.today ?? 0 }}</div>
            </div>
          </VCardText>
        </VCard>
      </VCol>
      <VCol cols="6" md="3">
        <VCard class="card-shadow kpi-card h-100">
          <VCardText class="d-flex align-center gap-3">
            <VAvatar color="info" variant="tonal" size="40">
              <VIcon icon="tabler-calendar-minus" size="20" />
            </VAvatar>
            <div>
              <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.yesterday') }}</div>
              <div class="text-h5 font-weight-medium">{{ summary?.yesterday ?? 0 }}</div>
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
            <div class="text-caption text-medium-emphasis">{{ $t('page.printStats.rangeTotal') }}</div>
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
              {{ sourceDisplay(s.source) }} · {{ s.count }}
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
