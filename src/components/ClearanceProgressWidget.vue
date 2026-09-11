<script setup>
// 清關進度浮動框:全域常駐(掛 DefaultLayout,跨頁不消失),只有按關閉才收起。
// 預設顯示「當日」報關進度 — 袋(剩/總)、件(剩/總);列印由 clearance-date 頻道即時遞減。
// 日期區間預設當日,有需要時點齒輪開對話框另設(上限 3 天)。
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@/api/events'
import { useClearanceProgress } from '@/stores/clearanceProgress'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { hasBackend } from '@/api/runtime'

const { t } = useI18n()
const store = useClearanceProgress()

// 即時更新全走雲端廣播(無輪詢):已印 → 遞減剩餘;新增 → 累加總數
let unlistenPrinted = null
let unlistenAdded = null
let unlistenRemoved = null
let unlistenReconnected = null
onMounted(async () => {
  if (!hasBackend) return
  unlistenPrinted = await listen('clearance-progress-printed', evt => {
    store.applyPrinted(evt?.payload?.shipping_no, evt?.payload?.package_sn)
  })
  unlistenAdded = await listen('clearance-progress-added', evt => {
    store.applyAdded(evt?.payload?.parcels)
  })
  unlistenRemoved = await listen('clearance-progress-removed', evt => {
    store.applyRemoved(evt?.payload?.parcels)
  })
  // WS 重連成功:斷線窗內 clearance-date 廣播已遺失且無補洞 → 重拉基準校正數字
  unlistenReconnected = await listen('sync-reconnected', () => {
    store.reloadAfterReconnect()
  })
})
onUnmounted(() => {
  if (unlistenPrinted) { unlistenPrinted(); unlistenPrinted = null }
  if (unlistenAdded) { unlistenAdded(); unlistenAdded = null }
  if (unlistenRemoved) { unlistenRemoved(); unlistenRemoved = null }
  if (unlistenReconnected) { unlistenReconnected(); unlistenReconnected = null }
  window.removeEventListener('resize', keepInViewport)
})

// === 顯示設定對話框(日期區間 + 貼單數廠別;預設當日、全廠,有需要才開)===
const dlg = ref(false)
const dFrom = ref(store.from)
const dTo = ref(store.to)
const dScope = ref(store.stickerScope)

// 貼單數的統計範圍。作業監控頁是分廠顯示的,兩邊要對帳時把這裡切成同一個廠別。
const scopeItems = computed(() => [
  { value: '', title: t('page.clearanceProgress.scopeAll') },
  { value: '0', title: t('page.fieldOperationMonitor.tabTaoyuan') },
  { value: '1', title: t('page.fieldOperationMonitor.tabTaichung') },
])
const scopeLabel = computed(() => scopeItems.value.find(i => i.value === store.stickerScope)?.title || '')

const openDlg = () => {
  dFrom.value = store.from
  dTo.value = store.to
  dScope.value = store.stickerScope
  dlg.value = true
}
const applyDates = () => {
  // 防呆:結束日早於起始日(YYYY-MM-DD 字典序=時序)→ 收斂為單日,避免空訂閱/反向顯示
  if (dTo.value && dFrom.value && dTo.value < dFrom.value) dTo.value = dFrom.value
  dlg.value = false
  store.setStickerScope(dScope.value)
  store.loadRange(dFrom.value, dTo.value || dFrom.value)
}

// 千位分隔(例 25000 → 25,000)
const fmt = n => Number(n || 0).toLocaleString('en-US')

// === 位置與拖曳 ===
// 位置存在 localStorage,但存的時候的視窗跟現在的不一定一樣(桌面存的 x=260 拿到 390px 的手機上,
// 整個框有一半在畫面外;手機轉向也一樣)。所以位置不直接採信,每次要用都先夾回目前視窗內。
const widgetEl = ref(null)
const clampPos = (x, y) => {
  const w = widgetEl.value?.offsetWidth || 230
  return {
    x: Math.min(Math.max(0, x), Math.max(0, window.innerWidth - w)),
    y: Math.min(Math.max(0, y), Math.max(0, window.innerHeight - 48)),
  }
}
const keepInViewport = () => {
  if (!store.open) return
  const p = clampPos(store.pos.x, store.pos.y)
  if (p.x !== store.pos.x || p.y !== store.pos.y) store.setPos(p.x, p.y)
}
window.addEventListener('resize', keepInViewport)
// 開啟時 DOM 才存在,等渲染完再量寬度夾位置
watch(() => store.open, open => { if (open) requestAnimationFrame(keepInViewport) }, { immediate: true })

// 拖曳走 Pointer Events:滑鼠、觸控、觸控筆同一套;只聽 mouse 事件手機會完全拖不動。
// 標題列要配 touch-action: none,否則手指一動瀏覽器先拿去捲頁面,pointermove 就沒了。
const dragging = ref(false)
let startX = 0;let startY = 0;let baseX = 0;let baseY = 0
// rAF 節流:pointermove 每幀最多寫一次 pinia(取該幀最新位置),避免每個事件都寫 store
// 觸發響應式 + widget style 重算
let _dragRaf = 0;let _lastMove = null
const onDragMove = e => {
  if (!dragging.value) return
  _lastMove = e
  if (_dragRaf) return
  _dragRaf = requestAnimationFrame(() => {
    _dragRaf = 0
    const ev = _lastMove
    store.pos = clampPos(baseX + (ev.clientX - startX), baseY + (ev.clientY - startY))
  })
}
const onDragEnd = e => {
  if (!dragging.value) return
  if (_dragRaf) { cancelAnimationFrame(_dragRaf); _dragRaf = 0 }
  dragging.value = false
  store.setPos(store.pos.x, store.pos.y)
  const bar = e?.currentTarget
  if (bar?.hasPointerCapture?.(e.pointerId)) bar.releasePointerCapture(e.pointerId)
}
const onDragStart = e => {
  // 只接主要按鍵(滑鼠左鍵 / 單指),右鍵或多指不當拖曳
  if (e.button !== 0) return
  dragging.value = true
  startX = e.clientX; startY = e.clientY
  baseX = store.pos.x; baseY = store.pos.y
  // 抓住指標:手指滑出標題列(甚至滑出視窗)仍持續收到 move / up,放開才結束
  e.currentTarget.setPointerCapture?.(e.pointerId)
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="store.open"
      ref="widgetEl"
      class="clearance-widget"
      :style="{ insetInlineStart: store.pos.x + 'px', insetBlockStart: store.pos.y + 'px' }"
    >
      <!-- 標題列(可拖曳)-->
      <div
        class="clearance-widget__bar"
        @pointerdown.prevent="onDragStart"
        @pointermove="onDragMove"
        @pointerup="onDragEnd"
        @pointercancel="onDragEnd"
      >
        <VIcon icon="tabler-clipboard-check" size="18" class="me-1" />
        <span class="text-body-medium font-weight-bold">{{ t('page.clearanceProgress.title') }}</span>
        <VSpacer />
        <VBtn icon="tabler-calendar-cog" size="x-small" variant="text" density="comfortable" @click="openDlg" @pointerdown.stop>
          <VIcon icon="tabler-calendar-cog" />
          <VTooltip activator="parent" location="bottom">{{ t('page.clearanceProgress.setRange') }}</VTooltip>
        </VBtn>
        <VBtn icon="tabler-refresh" size="x-small" variant="text" density="comfortable" :loading="store.loading" @click="store.loadRange(store.from, store.to)" @pointerdown.stop>
          <VIcon icon="tabler-refresh" />
        </VBtn>
        <VBtn icon="tabler-x" size="x-small" variant="text" density="comfortable" @click="store.close()" @pointerdown.stop />
      </div>

      <div class="pa-3">
        <!-- 追蹤期間 -->
        <div class="text-body-small text-medium-emphasis mb-2 d-flex align-center ga-1">
          <VIcon icon="tabler-calendar" size="14" />{{ store.rangeLabel }}
        </div>

        <VAlert v-if="store.error" type="error" variant="tonal" density="compact" class="mb-2">{{ store.error }}</VAlert>

        <!-- 袋 / 件:剩 / 總 -->
        <div class="cw-row">
          <div class="cw-row__label">{{ t('page.clearanceProgress.bags') }}</div>
          <div class="cw-row__val">
            <span class="cw-row__remain text-info">{{ fmt(store.bagRemaining) }}</span>
            <span class="cw-row__sep">/</span>
            <span class="cw-row__total">{{ fmt(store.bagTotal) }}</span>
          </div>
        </div>
        <div class="cw-row">
          <div class="cw-row__label">{{ t('page.clearanceProgress.parcels') }}</div>
          <div class="cw-row__val">
            <span class="cw-row__remain text-warning">{{ fmt(store.parcelRemaining) }}</span>
            <span class="cw-row__sep">/</span>
            <span class="cw-row__total">{{ fmt(store.parcelTotal) }}</span>
          </div>
        </div>
        <div class="text-body-small text-disabled mt-1">{{ t('page.clearanceProgress.remainTotalHint') }}</div>

        <!-- 今日貼單單數(去重):業務日 06:00 起算,與報關日區間無關 -->
        <div class="cw-row cw-row--sticker">
          <div class="cw-row__label">{{ t('page.clearanceProgress.stickerOrders') }}</div>
          <div class="cw-row__val">
            <span class="cw-row__remain text-success">{{ fmt(store.stickerOrderNum) }}</span>
          </div>
        </div>
        <!-- 標明統計範圍:全廠與單一廠別的數字本來就不同,沒寫出來會被當成兜不攏 -->
        <div class="text-body-small text-disabled">{{ scopeLabel }}・{{ t('page.clearanceProgress.stickerHint', { date: store.stickerDate || '—' }) }}</div>
      </div>
    </div>

    <!-- 日期區間設定對話框 -->
    <VDialog v-model="dlg" max-width="420">
      <div style="position: relative;">
        <VBtn
          icon
          variant="elevated"
          size="x-small"
          style="position: absolute; top: -12px; right: -12px; z-index: 10;"
          @click="dlg = false"
        >
          <VIcon icon="tabler-x" size="14" />
        </VBtn>
        <VCard>
        <VCardTitle class="text-body-large">{{ t('page.clearanceProgress.setRange') }}</VCardTitle>
        <VCardText>
          <div class="text-body-small text-medium-emphasis mb-3">{{ t('page.clearanceProgress.rangeHint') }}</div>
          <div class="date-range">
            <div class="date-range__field"><AppDatePicker v-model="dFrom" density="compact" /></div>
            <span class="date-range__sep text-disabled">~</span>
            <div class="date-range__field"><AppDatePicker v-model="dTo" density="compact" /></div>
          </div>

          <VDivider class="my-4" />

          <div class="text-body-small text-medium-emphasis mb-2">{{ t('page.clearanceProgress.scopeHint') }}</div>
          <VSelect
            v-model="dScope"
            :items="scopeItems"
            item-title="title"
            item-value="value"
            density="compact"
            hide-details
            :label="t('page.clearanceProgress.stickerOrders')"
          />
        </VCardText>
        <VCardActions class="px-4 pb-3">
          <VSpacer />
          <VBtn variant="text" @click="dlg = false">{{ t('common.cancel') }}</VBtn>
          <VBtn color="primary" variant="flat" :loading="store.loading" @click="applyDates">{{ t('common.search') }}</VBtn>
        </VCardActions>
        </VCard>
      </div>
    </VDialog>
  </Teleport>
</template>

<style scoped lang="scss">
.clearance-widget {
  position: fixed;
  // 要在導覽列(1003)之上、但在 Vuetify 的對話框 / 選單 / 提示(2400)之下:
  // 跟 2400 平手時後掛到 body 的這個框會蓋住自己開出來的「設定日期區間」對話框
  z-index: 1010;
  inline-size: 230px;
  background: rgb(var(--v-theme-surface));
  border: 1px solid rgba(var(--v-border-color), 0.2);
  border-radius: 10px;
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.22);
  overflow: hidden;
}
.clearance-widget__bar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 6px 4px 6px 12px;
  cursor: move;
  user-select: none;
  touch-action: none; // 手指按住標題列時不讓瀏覽器拿去捲頁面,拖曳才收得到 pointermove
  background: rgba(var(--v-theme-primary), 0.12);
}
.cw-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 4px;

  & + & { border-block-start: 1px solid rgba(var(--v-border-color), 0.12); }
}
.cw-row--sticker { margin-block-start: 6px; border-block-start: 1px dashed rgba(var(--v-border-color), 0.25); }
.cw-row__label { font-size: 1rem; font-weight: 600; opacity: 0.8; flex: 0 0 auto; }
.cw-row__val { font-variant-numeric: tabular-nums; white-space: nowrap; text-align: end; }
.cw-row__remain { font-size: 1.5rem; font-weight: 800; line-height: 1; }
.cw-row__sep { font-size: 1.1rem; opacity: 0.4; margin-inline: 3px; }
.cw-row__total { font-size: 1.1rem; font-weight: 600; opacity: 0.6; }
</style>
