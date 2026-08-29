<script setup>
// 清關進度浮動框:全域常駐(掛 DefaultLayout,跨頁不消失),只有按關閉才收起。
// 預設顯示「當日」報關進度 — 袋(剩/總)、件(剩/總);列印由 clearance-date 頻道即時遞減。
// 日期區間預設當日,有需要時點齒輪開對話框另設(上限 3 天)。
import { onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'
import { useClearanceProgress } from '@/stores/clearanceProgress'
import AppDatePicker from '@/components/AppDatePicker.vue'

const { t } = useI18n()
const store = useClearanceProgress()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 即時更新全走雲端廣播(無輪詢):已印 → 遞減剩餘;新增 → 累加總數
let unlistenPrinted = null
let unlistenAdded = null
let unlistenRemoved = null
let unlistenReconnected = null
onMounted(async () => {
  if (!isTauriRuntime) return
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
})

// === 日期區間設定對話框(預設當日,有需要才開)===
const dlg = ref(false)
const dFrom = ref(store.from)
const dTo = ref(store.to)
const openDlg = () => {
  dFrom.value = store.from
  dTo.value = store.to
  dlg.value = true
}
const applyDates = () => {
  // 防呆:結束日早於起始日(YYYY-MM-DD 字典序=時序)→ 收斂為單日,避免空訂閱/反向顯示
  if (dTo.value && dFrom.value && dTo.value < dFrom.value) dTo.value = dFrom.value
  dlg.value = false
  store.loadRange(dFrom.value, dTo.value || dFrom.value)
}

// 千位分隔(例 25000 → 25,000)
const fmt = n => Number(n || 0).toLocaleString('en-US')

// === 拖曳 ===
const dragging = ref(false)
let startX = 0;let startY = 0;let baseX = 0;let baseY = 0
// rAF 節流:mousemove 每幀最多寫一次 pinia(取該幀最新位置),避免每個 mousemove 都寫 store
// 觸發響應式 + widget style 重算
let _dragRaf = 0;let _lastMove = null
const onDragMove = e => {
  if (!dragging.value) return
  _lastMove = e
  if (_dragRaf) return
  _dragRaf = requestAnimationFrame(() => {
    _dragRaf = 0
    const ev = _lastMove
    const w = 230
    const x = Math.min(Math.max(0, baseX + (ev.clientX - startX)), window.innerWidth - w)
    const y = Math.min(Math.max(0, baseY + (ev.clientY - startY)), window.innerHeight - 48)
    store.pos = { x, y }
  })
}
const onDragEnd = () => {
  if (!dragging.value) return
  if (_dragRaf) { cancelAnimationFrame(_dragRaf); _dragRaf = 0 }
  dragging.value = false
  store.setPos(store.pos.x, store.pos.y)
  window.removeEventListener('mousemove', onDragMove)
  window.removeEventListener('mouseup', onDragEnd)
}
const onDragStart = e => {
  dragging.value = true
  startX = e.clientX; startY = e.clientY
  baseX = store.pos.x; baseY = store.pos.y
  window.addEventListener('mousemove', onDragMove)
  window.addEventListener('mouseup', onDragEnd)
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="store.open"
      class="clearance-widget"
      :style="{ insetInlineStart: store.pos.x + 'px', insetBlockStart: store.pos.y + 'px' }"
    >
      <!-- 標題列(可拖曳)-->
      <div class="clearance-widget__bar" @mousedown.prevent="onDragStart">
        <VIcon icon="tabler-clipboard-check" size="18" class="me-1" />
        <span class="text-body-medium font-weight-bold">{{ t('page.clearanceProgress.title') }}</span>
        <VSpacer />
        <VBtn icon="tabler-calendar-cog" size="x-small" variant="text" density="comfortable" @click="openDlg" @mousedown.stop>
          <VIcon icon="tabler-calendar-cog" />
          <VTooltip activator="parent" location="bottom">{{ t('page.clearanceProgress.setRange') }}</VTooltip>
        </VBtn>
        <VBtn icon="tabler-refresh" size="x-small" variant="text" density="comfortable" :loading="store.loading" @click="store.loadRange(store.from, store.to)" @mousedown.stop>
          <VIcon icon="tabler-refresh" />
        </VBtn>
        <VBtn icon="tabler-x" size="x-small" variant="text" density="comfortable" @click="store.close()" @mousedown.stop />
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
        <div class="text-body-small text-disabled">{{ t('page.clearanceProgress.stickerHint', { date: store.stickerDate || '—' }) }}</div>
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
          <div class="d-flex align-center ga-2">
            <AppDatePicker v-model="dFrom" density="compact" />
            <span class="text-disabled">~</span>
            <AppDatePicker v-model="dTo" density="compact" />
          </div>
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
  z-index: 2400;
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
