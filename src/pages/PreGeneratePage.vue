<script setup>
import { cloudFetchLabel, cloudPackageOrders, cloudOrdersByDate, getConfig, updateConfig, getPregenStatus } from '@/api/tauri'
import { useRouter } from 'vue-router'
import { preGenInputMode as inputMode } from '@/composables/usePreGenState'
import { loadProcessed, isOrderProcessed, markOrderProcessed, persistProcessed, clearProcessed, processedCount } from '@/composables/usePreGenProcessed'
import {
  isDownloadable, statusLabel, statusIcon, errorMessageFromException,
} from '@/composables/useLabelStatus'
import AppBulkInput from '@/components/AppBulkInput.vue'
import AppHeader from '@/components/AppHeader.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { localTodayStr } from '@/utils/localDate'
import { toast } from 'vue3-toastify'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const router = useRouter()

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
const skippedCount = ref(0)  // 本快取日內已預產過、本批略過重打雲端的筆數
// 強制重跑:開啟時忽略「已預產」去重、且要求後端強制重新下載覆蓋既有快取檔
const forceRerun = ref(false)
// 本快取日目前累積的「已預產」筆數(供「清除已預產記錄」鈕顯示與啟用判斷)
const processedTodayCount = ref(0)
const refreshProcessedCount = () => { processedTodayCount.value = processedCount() }
const progressPct = computed(() => (totalCount.value ? Math.round(completedCount.value / totalCount.value * 100) : 0))

// 本批執行耗時(ms):執行中由 ticker 每 200ms 即時更新,結束後定格在總耗時。
// 用 performance.now()(單調時鐘)避免系統校時影響;前端瀏覽器環境可用。
const elapsedMs = ref(0)
let batchStartTs = 0
let elapsedTimer = null

const elapsedText = computed(() => {
  const s = elapsedMs.value / 1000
  if (s < 60) return t('page.preGenerate.elapsedSec', { s: s.toFixed(1) })
  const m = Math.floor(s / 60)
  return t('page.preGenerate.elapsedMin', { m, s: Math.round(s % 60) })
})

const startElapsedTimer = () => {
  batchStartTs = performance.now()
  elapsedMs.value = 0
  if (elapsedTimer) clearInterval(elapsedTimer)
  elapsedTimer = setInterval(() => { elapsedMs.value = performance.now() - batchStartTs }, 200)
}
const stopElapsedTimer = () => {
  if (elapsedTimer) { clearInterval(elapsedTimer); elapsedTimer = null }
  if (batchStartTs) elapsedMs.value = performance.now() - batchStartTs // 定格總耗時
}
// 執行中離開頁面時清掉 ticker / 狀態輪詢,避免殘留 interval
onUnmounted(() => {
  if (elapsedTimer) { clearInterval(elapsedTimer); elapsedTimer = null }
  if (_statusTimer) { clearInterval(_statusTimer); _statusTimer = null }
})

const CONCURRENCY = 20
const FAIL_LIST_LIMIT = 300  // 失敗清單最多保留筆數

const startBatch = snList => {
  downloadList.splice(0)
  downloadStatus.splice(0)
  totalCount.value = snList.length
  completedCount.value = 0
  successCount.value = 0
  failCount.value = 0
  skippedCount.value = 0
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
  // 本快取日內已成功預產過 → 略過,不重打雲端(只處理大量圖的預產才需要這層去重)。
  // 「強制重跑」開啟時跳過這層去重,讓已預產過的也重抓。
  if (!forceRerun.value && isOrderProcessed(sn)) {
    skippedCount.value++
    completedCount.value++
    if (thumb) { thumb.code = 'SKIPPED'; thumb.message = t('page.preGenerate.skipped') }
    return
  }
  try {
    // force:強制重跑時連後端也忽略快取命中、重新下載覆蓋既有面單檔
    const data = await cloudFetchLabel(sn, { mode: 'download', force: forceRerun.value })
    const code = data.print_view_status || ''
    if (isDownloadable(code)) {
      successCount.value++
      markOrderProcessed(sn)  // 記住已成功,本快取日內不再重打
    } else {
      failCount.value++
      // 雲端 4xx 業務錯誤(middleware 合成失敗結果)帶人類可讀解釋 → 一併顯示
      pushFail(sn, code, data.respond_message || '')
    }
    if (thumb) {
      thumb.code = code
      thumb.shipping_no = data.print_shipping_no || ''
      thumb.shipping_provider = data.print_shipping_provider || ''
      thumb.image = data.print_file_path || null
      thumb.message = data.respond_message || statusLabel(code)
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
  // 批次開始前先與後端同步「今日已預產」快照:才能吃到自動排程(或其他機台)已做的部分,
  // 已預產者直接略過、不重打雲端(這正是自動跑完後手動不再一堆「成功」的關鍵)。
  if (!forceRerun.value) await loadProcessed()
  startElapsedTimer()
  const total = snList.length
  let cursor = 0

  const worker = async () => {
    while (!abortRequested) {
      const i = cursor++
      if (i >= total) break
      await processOne(snList[i], i)
    }
  }

  try {
    await Promise.all(
      Array.from({ length: Math.min(CONCURRENCY, total) }, () => worker()),
    )
  } finally {
    stopElapsedTimer()
    await persistProcessed()  // 本批新標記的已預產訂單一次寫回後端共用去重(避免逐筆 IPC)
    refreshProcessedCount()
    isProcessing.value = false
  }
}

const stopProcessing = () => { abortRequested = true }

// 清除本快取日「已預產」記憶,讓所有訂單下次查詢都重新抓取(配合或不配合「強制重跑」皆可用)
// clearProcessed 先清後端、失敗會拋出;此處攔截並提示,不讓「清了卻沒清成」靜默發生。
const handleClearProcessed = async () => {
  try {
    await clearProcessed()
  } catch (e) {
    toast(t('page.preGenerate.clearProcessedFailed', { msg: errorMessageFromException(e) }), { type: 'error' })
  }
  refreshProcessedCount()
}

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
const clearanceDate = ref(localTodayStr())
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

// === 自動排程設定(寫入 config.pre_gen_schedule,後端常駐 worker 依此每天到點執行)===
const scheduleDialog = ref(false)
const scheduleSaving = ref(false)
const scheduleError = ref('')
const sched = reactive({ enabled: false, times: [], sources: [], catchUp: false })
let _fullConfig = null // 保存完整 config,存檔只覆寫 pre_gen_schedule,不動其他設定

// 頁面常駐狀態(不需開對話框即可看):目前排程設定快照 + 後端執行狀態
const pageSched = reactive({ enabled: false, times: [], catchUp: false, loaded: false })
const pregenStatus = ref(null)
let _statusTimer = null

const applyConfigSnapshot = cfg => {
  const s = (cfg && cfg.pre_gen_schedule) || {}
  pageSched.enabled = !!s.enabled
  pageSched.times = [...new Set((Array.isArray(s.times) ? s.times : []).filter(tt => /^\d{2}:\d{2}$/.test(tt)))].sort()
  pageSched.catchUp = !!s.catch_up_on_start
  pageSched.loaded = true
}

const refreshPregenStatus = async () => {
  try { pregenStatus.value = await getPregenStatus() } catch { /* 靜默:狀態查詢失敗不阻塞頁面 */ }
}

// 常駐「下次執行」:從頁面快照(非對話框暫存)計算,關掉對話框仍可見
const pageNextRunText = computed(() => {
  if (!pageSched.enabled || !pageSched.times.length) return ''
  const now = new Date()
  const nowMin = now.getHours() * 60 + now.getMinutes()
  for (const tt of pageSched.times) {
    const [h, m] = tt.split(':').map(Number)
    if (h * 60 + m > nowMin) return `${t('page.preGenerate.scheduleToday')} ${tt}`
  }
  return `${t('page.preGenerate.scheduleTomorrow')} ${pageSched.times[0]}`
})

const goPregenLog = () => router.push({ name: 'event-log', query: { category: 'pregen' } })

onMounted(async () => {
  await loadProcessed()  // 從後端載入「今日已預產」快照(含自動排程成果)
  refreshProcessedCount()
  try { applyConfigSnapshot(await getConfig()) } catch { /* 頁面首載失敗不致命 */ }
  await refreshPregenStatus()
  // 狀態每 30s 刷新一次,讓「上次執行」在背景到點後自動更新
  _statusTimer = setInterval(refreshPregenStatus, 30000)
})

const openSchedule = async () => {
  scheduleError.value = ''
  try {
    _fullConfig = await getConfig()
    const s = _fullConfig.pre_gen_schedule || {}
    sched.enabled = !!s.enabled
    sched.times = Array.isArray(s.times) ? [...s.times] : []
    sched.sources = Array.isArray(s.sources) ? [...s.sources] : ['clearance', 'transfer']
    sched.catchUp = !!s.catch_up_on_start
    applyConfigSnapshot(_fullConfig)
  } catch (e) {
    scheduleError.value = errorMessageFromException(e)
  }
  scheduleDialog.value = true
}

// 新增一列(預設 09:00),使用者再就地調整;永遠可點,不需先輸入
const addTimeRow = () => { sched.times.push('09:00') }
const removeTimeAt = i => { sched.times.splice(i, 1) }
const setTimeAt = (i, v) => { sched.times[i] = v }

// 有效時間點(去重、排序)
const validTimes = computed(() =>
  [...new Set(sched.times.filter(tt => /^\d{2}:\d{2}$/.test(tt)))].sort(),
)
// 已啟用但缺時間或來源 → 防呆(存檔禁用 + 提示)
const scheduleIncomplete = computed(
  () => sched.enabled && (validTimes.value.length === 0 || sched.sources.length === 0),
)
// 下次執行預覽:從今天當下找最近的時間點,沒有則明天第一個
const nextRunText = computed(() => {
  if (!sched.enabled || !validTimes.value.length) return ''
  const now = new Date()
  const nowMin = now.getHours() * 60 + now.getMinutes()
  for (const tt of validTimes.value) {
    const [h, m] = tt.split(':').map(Number)
    if (h * 60 + m > nowMin) return `${t('page.preGenerate.scheduleToday')} ${tt}`
  }
  return `${t('page.preGenerate.scheduleTomorrow')} ${validTimes.value[0]}`
})
const toggleSource = src => {
  if (sched.sources.includes(src)) sched.sources = sched.sources.filter(s => s !== src)
  else sched.sources.push(src)
}

const saveSchedule = async () => {
  scheduleError.value = ''
  scheduleSaving.value = true
  try {
    const cfg = _fullConfig ? { ..._fullConfig } : await getConfig()
    // 過濾有效 HH:MM、去重、排序
    const cleanTimes = [...new Set(sched.times.filter(t => /^\d{2}:\d{2}$/.test(t)))].sort()
    cfg.pre_gen_schedule = {
      ...(cfg.pre_gen_schedule || {}),
      enabled: sched.enabled,
      times: cleanTimes,
      sources: [...sched.sources],
      catch_up_on_start: sched.catchUp,
    }
    await updateConfig(cfg)
    applyConfigSnapshot(cfg) // 立即更新頁面常駐狀態(下次執行等)
    scheduleDialog.value = false
  } catch (e) {
    scheduleError.value = errorMessageFromException(e)
  } finally {
    scheduleSaving.value = false
  }
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.preGenerate.title')" :subtitle="$t('page.preGenerate.subtitle')" icon="tabler-photo-down">
      <template #actions>
        <VBtn color="default" variant="tonal" prepend-icon="tabler-clock-cog" @click="openSchedule">
          {{ $t('page.preGenerate.scheduleBtn') }}
        </VBtn>
      </template>
    </AppHeader>

    <!-- 排程常駐狀態:不必開對話框即可確認「排程是否啟用 / 下次何時跑 / 上次跑得如何」 -->
    <VCard v-if="pageSched.loaded" variant="tonal" class="mb-4 sched-status-card" :color="pageSched.enabled ? 'primary' : undefined">
      <VCardText class="py-3 d-flex flex-wrap align-center gc-6 gr-2">
        <div class="d-flex align-center ga-2">
          <VIcon :icon="pageSched.enabled ? 'tabler-clock-check' : 'tabler-clock-off'" size="20" :color="pageSched.enabled ? 'primary' : 'medium-emphasis'" />
          <span class="text-body-2 font-weight-medium">
            {{ pageSched.enabled ? $t('page.preGenerate.scheduleTitle') : $t('page.preGenerate.scheduleStatusDisabled') }}
          </span>
        </div>

        <div v-if="pageSched.enabled && pageNextRunText" class="text-caption d-flex align-center ga-1">
          <VIcon icon="tabler-player-play" size="15" color="primary" />
          {{ $t('page.preGenerate.scheduleNextRun') }}：<strong class="text-primary">{{ pageNextRunText }}</strong>
        </div>

        <div v-if="pregenStatus && pregenStatus.last_run_at" class="text-caption d-flex align-center ga-1 flex-wrap">
          <VIcon :icon="pregenStatus.last_had_error ? 'tabler-alert-triangle' : 'tabler-circle-check'" size="15" :color="pregenStatus.last_had_error ? 'warning' : 'success'" />
          {{ $t('page.preGenerate.scheduleStatusLastRun') }}：<strong>{{ pregenStatus.last_run_at }}</strong>
          <span class="text-medium-emphasis">({{ $t('page.preGenerate.scheduleStatusTrigger') }} {{ pregenStatus.last_trigger }})</span>
          <span class="text-medium-emphasis">{{ $t('page.preGenerate.scheduleStatusResult', { ok: pregenStatus.last_ok, skipped: pregenStatus.last_skipped, fail: pregenStatus.last_fail, empty: pregenStatus.last_empty }) }}</span>
        </div>
        <div v-else-if="pageSched.enabled" class="text-caption text-medium-emphasis d-flex align-center ga-1">
          <VIcon icon="tabler-hourglass" size="15" />{{ $t('page.preGenerate.scheduleStatusNeverRun') }}
        </div>

        <VSpacer />
        <VBtn variant="text" size="small" color="default" prepend-icon="tabler-list-search" @click="goPregenLog">
          {{ $t('page.preGenerate.scheduleOpenLog') }}
        </VBtn>
      </VCardText>
    </VCard>

    <!-- 自動排程設定對話框(persistent:點遮罩 / ESC 不關,避免誤觸丟失編輯,對齊其他編輯對話框) -->
    <VDialog v-model="scheduleDialog" max-width="460" persistent>
      <VCard class="sched-card">
        <VCardItem>
          <template #prepend>
            <VAvatar color="primary" variant="tonal" rounded>
              <VIcon icon="tabler-clock-cog" />
            </VAvatar>
          </template>
          <VCardTitle>{{ $t('page.preGenerate.scheduleTitle') }}</VCardTitle>
          <VCardSubtitle class="text-wrap">{{ $t('page.preGenerate.scheduleHint') }}</VCardSubtitle>
        </VCardItem>
        <VDivider />
        <VCardText>
          <VAlert v-if="scheduleError" type="error" variant="tonal" density="compact" class="mb-4">{{ scheduleError }}</VAlert>

          <!-- 主開關:醒目列,顯示啟用 / 關閉狀態 -->
          <div class="sched-switch" :class="{ 'sched-switch--on': sched.enabled }">
            <div>
              <div class="text-body-1 font-weight-medium">{{ $t('page.preGenerate.scheduleEnabled') }}</div>
              <div class="text-caption text-medium-emphasis">
                {{ sched.enabled ? $t('page.preGenerate.scheduleOn') : $t('page.preGenerate.scheduleOff') }}
              </div>
            </div>
            <VSwitch v-model="sched.enabled" color="primary" hide-details density="compact" inset />
          </div>

          <!-- 下次執行預覽 -->
          <div v-if="nextRunText" class="sched-next">
            <VIcon icon="tabler-player-play" size="16" color="primary" />
            <span class="text-caption">{{ $t('page.preGenerate.scheduleNextRun') }}：<strong class="text-primary">{{ nextRunText }}</strong></span>
          </div>

          <!-- 時間點 -->
          <div class="text-body-2 font-weight-medium mt-4 mb-2">{{ $t('page.preGenerate.scheduleTimes') }}</div>
          <div v-for="(tt, i) in sched.times" :key="i" class="d-flex align-center ga-1 mb-2">
            <VTextField
              :model-value="tt"
              type="time"
              variant="outlined"
              density="compact"
              hide-details
              prepend-inner-icon="tabler-clock"
              style="max-inline-size: 180px;"
              @update:model-value="v => setTimeAt(i, v)"
            />
            <VBtn icon variant="text" size="small" color="medium-emphasis" @click="removeTimeAt(i)">
              <VIcon icon="tabler-trash" size="18" />
            </VBtn>
          </div>
          <div v-if="!sched.times.length" class="text-caption text-disabled mb-2 d-flex align-center ga-1">
            <VIcon icon="tabler-clock-off" size="16" />{{ $t('page.preGenerate.scheduleNoTimes') }}
          </div>
          <VBtn variant="tonal" color="primary" size="small" prepend-icon="tabler-plus" @click="addTimeRow">
            {{ $t('page.preGenerate.scheduleAddTime') }}
          </VBtn>

          <!-- 預產來源:chip 切換 -->
          <div class="text-body-2 font-weight-medium mt-5 mb-2">{{ $t('page.preGenerate.scheduleSources') }}</div>
          <div class="d-flex ga-2">
            <VChip
              :color="sched.sources.includes('clearance') ? 'primary' : 'default'"
              :variant="sched.sources.includes('clearance') ? 'flat' : 'tonal'"
              @click="toggleSource('clearance')"
            >
              <VIcon v-if="sched.sources.includes('clearance')" icon="tabler-check" size="16" start />
              {{ $t('page.preGenerate.sourceClearance') }}
            </VChip>
            <VChip
              :color="sched.sources.includes('transfer') ? 'primary' : 'default'"
              :variant="sched.sources.includes('transfer') ? 'flat' : 'tonal'"
              @click="toggleSource('transfer')"
            >
              <VIcon v-if="sched.sources.includes('transfer')" icon="tabler-check" size="16" start />
              {{ $t('page.preGenerate.sourceTransfer') }}
            </VChip>
          </div>

          <!-- 開機補跑開關 -->
          <div class="sched-switch mt-5" :class="{ 'sched-switch--on': sched.catchUp }">
            <div>
              <div class="text-body-2 font-weight-medium">{{ $t('page.preGenerate.scheduleCatchUp') }}</div>
              <div class="text-caption text-medium-emphasis text-wrap">{{ $t('page.preGenerate.scheduleCatchUpHint') }}</div>
            </div>
            <VSwitch v-model="sched.catchUp" color="primary" hide-details density="compact" inset />
          </div>

          <div class="text-caption text-medium-emphasis mt-4 d-flex align-center ga-1">
            <VIcon icon="tabler-info-circle" size="15" />{{ $t('page.preGenerate.scheduleDateNote') }}
          </div>
          <div v-if="scheduleIncomplete" class="text-caption text-warning mt-2 d-flex align-center ga-1">
            <VIcon icon="tabler-alert-triangle" size="15" />{{ $t('page.preGenerate.scheduleIncomplete') }}
          </div>
        </VCardText>
        <VDivider />
        <VCardActions class="px-4 pb-3">
          <VSpacer />
          <VBtn variant="text" @click="scheduleDialog = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="primary" variant="flat" :loading="scheduleSaving" :disabled="scheduleIncomplete" @click="saveSchedule">
            {{ $t('common.save') }}
          </VBtn>
        </VCardActions>
      </VCard>
    </VDialog>

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

              <!-- 重跑選項:強制重跑(忽略已預產去重 + 後端強制覆蓋下載)+ 清除已預產記錄 -->
              <div class="rerun-opts mt-4">
                <div class="d-flex align-center justify-space-between">
                  <div class="pe-2">
                    <div class="text-body-2 font-weight-medium">{{ $t('page.preGenerate.forceRerun') }}</div>
                    <div class="text-caption text-medium-emphasis text-wrap">{{ $t('page.preGenerate.forceRerunHint') }}</div>
                  </div>
                  <VSwitch v-model="forceRerun" color="primary" hide-details density="compact" inset :disabled="isProcessing" />
                </div>
                <VBtn
                  v-if="processedTodayCount > 0"
                  size="small" variant="tonal" color="secondary" block class="mt-2"
                  :disabled="isProcessing"
                  @click="handleClearProcessed"
                >
                  <VIcon icon="tabler-eraser" size="16" class="me-1" />
                  {{ $t('page.preGenerate.clearProcessed', { n: processedTodayCount }) }}
                </VBtn>
              </div>

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
                <div v-if="skippedCount > 0">
                  <div class="text-h6 text-medium-emphasis">{{ skippedCount }}</div>
                  <div class="text-caption text-medium-emphasis">{{ $t('page.preGenerate.statSkipped') }}</div>
                </div>
                <div>
                  <div class="text-h6">{{ elapsedText }}</div>
                  <div class="text-caption text-medium-emphasis">{{ $t('page.preGenerate.statElapsed') }}</div>
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
                    <div v-else-if="item.code === 'SKIPPED'" class="cell__skipped">
                      <VIcon icon="tabler-circle-check" size="40" class="cell__skipped-icon" />
                      <div class="cell__error-sn">{{ item.sn }}</div>
                      <div class="cell__error-msg">{{ item.message }}</div>
                    </div>
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

/* 自動排程對話框 */
.sched-switch {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  border-radius: 10px;
  background-color: rgba(var(--v-theme-on-surface), 0.04);
  transition: background-color 0.2s ease;

  &--on { background-color: rgba(var(--v-theme-primary), 0.1); }
}
.sched-next {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-block-start: 10px;
  padding: 6px 12px;
  border-radius: 8px;
  background-color: rgba(var(--v-theme-primary), 0.08);
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

// 已預產略過遮罩:中性綠底,與錯誤(紅)區隔 —— 共用 .cell__error-sn / -msg 的排版
.cell__skipped {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 12px;
  text-align: center;
  background: rgba(46, 80, 60, 0.72);
  color: #fff;
  font-weight: 400;
  letter-spacing: normal;
}
.cell__skipped-icon {
  color: #81c784;
  margin-block-end: 2px;
}
</style>
