<script setup>
import { bagCheckSnapshot, bagCheckClear } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import DisplayLauncher from '@/components/DisplayLauncher.vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'
import Masonry from 'masonry-layout'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const bags = ref([])
const loading = ref(false)
const errorMsg = ref('')
let unlisten = null

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    bags.value = (await bagCheckSnapshot()) || []
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

const clearList = async () => {
  errorMsg.value = ''
  try {
    await bagCheckClear()
    bags.value = []
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}

// 袋卡狀態:interrupted(中途被打斷,仍缺件) / missing(一般缺件) / complete(完整)。
// 中途被打斷 = 此袋還沒印完就出現別的袋號(後端 active_bag 判定);回補齊(missing=0)後
// 後端 recount 自動清旗標 → 轉回 complete。載入失敗(散單/未登入/雲端失敗)後端不建卡,不需處理。
const isInterrupted = bag => !!bag.interrupted && (bag.missing || 0) > 0
const bagState = bag =>
  isInterrupted(bag) ? 'interrupted' : (bag.missing || 0) > 0 ? 'missing' : 'complete'
const STATE_COLOR = { interrupted: 'error', missing: 'warning', complete: 'success' }
const STATE_ICON = { interrupted: 'tabler-alert-triangle', missing: 'tabler-alert-circle', complete: 'tabler-circle-check' }

// === 視圖模式 + 狀態篩選(記 localStorage,即時切換不重載)===
// viewMode: 'cards'(詳細卡片,現狀)| 'compact'(精簡一行)| 'missing'(缺件總覽扁平清單)
// stateFilter: 'all' | 'missing' | 'complete'(套用於 cards / compact;missing 視圖本身只列有缺)
const VIEW_KEY = 'cix3752iLabelPrint.bagCheck.view'
const FILTER_KEY = 'cix3752iLabelPrint.bagCheck.filter'
const loadPref = (k, def) => { try { return localStorage.getItem(k) || def } catch { return def } }
const viewMode = ref(loadPref(VIEW_KEY, 'cards'))
const stateFilter = ref(loadPref(FILTER_KEY, 'all'))
watch(viewMode, v => { try { localStorage.setItem(VIEW_KEY, v) } catch { /* 不可用略過 */ } })
watch(stateFilter, v => { try { localStorage.setItem(FILTER_KEY, v) } catch { /* 不可用略過 */ } })

const missingBagCount = computed(() => bags.value.filter(b => (b.missing || 0) > 0).length)
const completeBagCount = computed(() => bags.value.filter(b => (b.missing || 0) === 0).length)
const totalMissingItems = computed(() => bags.value.reduce((s, b) => s + (b.missing || 0), 0))

// 卡片 / 精簡模式依狀態篩選後的袋清單。
// 順帶把狀態算好掛進 _state / _stateColor / _stateIcon,template 每卡直接讀,
// 不用在渲染時重複呼叫 bagState()(每卡 3-4 次);computed 有快取,非 filteredBags 變動的 re-render 不重算。
const filteredBags = computed(() => {
  const list = stateFilter.value === 'missing' ? bags.value.filter(b => (b.missing || 0) > 0)
    : stateFilter.value === 'complete' ? bags.value.filter(b => (b.missing || 0) === 0)
      : bags.value
  return list.map(bag => {
    const _state = bagState(bag)
    return { ...bag, _state, _stateColor: STATE_COLOR[_state], _stateIcon: STATE_ICON[_state] }
  })
})

// 缺件總覽:每個有缺的袋 + 其未印件清單(扁平,一眼看完所有缺料)
const missingOverview = computed(() =>
  bags.value
    .filter(b => (b.missing || 0) > 0)
    .map(b => ({
      package_sn: b.package_sn,
      missing: b.missing || 0,
      unprinted: (b.orders || []).filter(o => !isPrinted(o)),
    })),
)

const isPrinted = o => !!(o.last_print_time && String(o.last_print_time).trim())
const formatTime = s => (s ? String(s).replace('T', ' ').slice(0, 19) : '')
// 列表「列印時間」只顯示時間(HH:MM:SS),不帶日期 — 同袋訂單同日列印,日期重複無資訊量
const formatTimeShort = s => (s ? String(s).replace('T', ' ').slice(11, 19) : '')

// 訂單明細展開狀態:預設全部收合(免佔版面),使用者點擊後才展開。
// 以 package_sn 為 key,袋清單刷新不影響已展開狀態。
const expandedMap = reactive({})
const isExpanded = bag => !!expandedMap[bag.package_sn]
const toggleExpand = async bag => {
  // 切換狀態 → 等表格渲染(高度即時到位)→ 單次 layout,
  // Masonry 以 transform 過渡讓鄰卡平滑滑動補位(animating item size,避免逐幀重排造成拖影)
  expandedMap[bag.package_sn] = !isExpanded(bag)
  await nextTick()
  masonry?.layout()
}

// === Masonry 瀑布流(desandro masonry-layout)===
// row-major 排列(最新在左上、由左到右),4 個一列填滿,不等高也向上緊貼。
// 重排靠 ResizeObserver:任何卡片高度變動(展開 / 收合 / 圖片載入)即觸發 layout()。
const masonryEl = ref(null)
let masonry = null
let ro = null
let rafId = 0
// Masonry 首次排版前隱藏容器,排好(絕對定位到網格)再淡入 → 避免看到「流式排列→網格」中途跳動的閃爍
const masonryReady = ref(false)

// 合併同一幀內多次請求,避免重複 layout()
const scheduleLayout = () => {
  if (rafId) return
  rafId = requestAnimationFrame(() => {
    rafId = 0
    masonry?.layout()
  })
}

// 重新訂閱所有卡片高度(資料筆數變動後卡片增減,需重新 observe)
const observeItems = () => {
  if (!ro || !masonryEl.value) return
  ro.disconnect()
  masonryEl.value.querySelectorAll('.bag-masonry__item').forEach(el => ro.observe(el))
}

// 建立 / 重載:資料筆數變動(新增 / 移除卡)時呼叫,Masonry 需重新收集子元素
const initMasonry = async () => {
  await nextTick()
  if (!masonryEl.value) return
  const fresh = !masonry // 首次建立(進入卡片模式)才需隱藏淡入;篩選 reload 維持平滑過渡不閃
  if (fresh) masonryReady.value = false
  if (!ro) ro = new ResizeObserver(scheduleLayout)
  if (masonry) {
    masonry.reloadItems()
    masonry.layout()
  } else {
    masonry = new Masonry(masonryEl.value, {
      itemSelector: '.bag-masonry__item',
      columnWidth: '.bag-masonry__sizer',
      gutter: 12,
      // percentPosition:false → Masonry 以 transform:translate 定位,Outlayer 才會對它加過渡 →
      // 卡片重排會平滑滑動(percentPosition:true 用 left/top 定位但無過渡,會瞬間跳位)
      percentPosition: false,
      transitionDuration: '0.3s', // 卡片位置變化以 transform 過渡平滑滑動(desandro animating item size)
    })
  }
  observeItems()
  if (fresh) {
    // 等 Masonry 完成首次絕對定位後再顯示(下一幀),避免流式中途畫面
    await nextTick()
    requestAnimationFrame(() => { masonryReady.value = true })
  }
}

// 卡片清單 / 視圖模式變動 → 重載 Masonry;非卡片模式或清單空 → 銷毀(容器被 v-if 移除,需丟棄舊實例)
let _relayoutTimer = null
let _disposed = false
watch([filteredBags, viewMode], async () => {
  if (viewMode.value !== 'cards' || !filteredBags.value.length) {
    if (_relayoutTimer) { clearTimeout(_relayoutTimer); _relayoutTimer = null }
    ro?.disconnect()
    masonry?.destroy()
    masonry = null
    masonryReady.value = false
    return
  }
  // 分揀忙碌時 bag-check-updated 每秒數次 → filteredBags 反覆換參考 → 逐次 reloadItems + 全量重排會抖。
  // 去抖合併 burst,停歇 120ms 才重排一次;ResizeObserver 仍即時處理單卡高度變化,不影響即時感。
  if (_relayoutTimer) return
  _relayoutTimer = setTimeout(() => { _relayoutTimer = null; initMasonry() }, 120)
})

onMounted(async () => {
  await load()
  await initMasonry()
  // 即時:後端每次更新袋件清單就 emit,前端直接套用快照(不輪詢、不重查雲端)。
  // await 期間可能已切頁,已卸載則立刻解除避免 Tauri 監聽殘留
  if (isTauriRuntime) {
    const un = await listen('bag-check-updated', evt => {
      bags.value = evt.payload || []
    })
    if (_disposed) un(); else unlisten = un
  }
})
onUnmounted(() => {
  _disposed = true
  if (_relayoutTimer) { clearTimeout(_relayoutTimer); _relayoutTimer = null }
  if (unlisten) { unlisten(); unlisten = null }
  if (rafId) { cancelAnimationFrame(rafId); rafId = 0 }
  if (ro) { ro.disconnect(); ro = null }
  if (masonry) { masonry.destroy(); masonry = null }
})
</script>

<style scoped lang="scss">
// Masonry 瀑布流:JS 將 item 設為 position:absolute、容器設相對定位並計算總高。
// 這裡只需用媒體查詢決定 item 與 sizer 寬度(欄數):手機 1 / 平板 2 / 桌面 3 / 寬螢幕 4 欄。
// 寬度 calc 與 gutter:12 對齊 → n 欄剛好填滿容器(4 欄:item=(100%-36px)/4 + 3*12px gutter = 100%)。
.bag-masonry {
  position: relative;
  // 首次排版前隱藏(不淡入,避免淡入本身被當閃爍),排好(.--ready)即顯示 → 看不到「流式→網格」中途畫面
  visibility: hidden;
}
.bag-masonry--ready {
  visibility: visible;
}
.bag-masonry__sizer,
.bag-masonry__item {
  inline-size: 100%;

  @media (min-width: 600px) { inline-size: calc((100% - 12px) / 2); }
  @media (min-width: 960px) { inline-size: calc((100% - 24px) / 3); }
  @media (min-width: 1264px) { inline-size: calc((100% - 36px) / 4); }
}
.bag-masonry__item {
  margin-block-end: 12px;
}
.bag-card {
  // flex 容器內 text-truncate 需要 min-width:0 才生效(防長袋號換行)
  :deep(.v-card-item__content) {
    min-inline-size: 0;
  }
}
.bag-card__orders {
  th, td { white-space: nowrap; }
}
.bag-card__toggle {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 7px 16px;
  cursor: pointer;
  user-select: none;
  background-color: rgba(var(--v-theme-primary), 0.16);
  transition: background-color 0.15s ease;

  &:hover { background-color: rgba(var(--v-theme-primary), 0.24); }
}
.bag-stat {
  display: flex;
  align-items: baseline;
  gap: 4px;
}
.bag-stat__num { font-size: 1.5rem; font-weight: 700; line-height: 1; }

// 缺件總覽:袋號加大加粗;未印單號純文字(無框線/底色),加大字級、以間距分隔
.bag-missing__bag {
  font-size: 1.15rem;
  font-weight: 700;
}
.bag-missing__nums {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 22px; // 列距 6 / 欄距 22:不靠框線,以間距清楚分隔每個單號
}
.bag-missing__num {
  font-size: 1rem;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.02em;
  color: rgba(var(--v-theme-on-surface), 0.82);
}

// 精簡列表:整體加大,數字欄再加粗加大
.bag-compact {
  :deep(th) { font-size: 0.95rem; }
  :deep(td) {
    font-size: 1.05rem;
    block-size: 48px; // 放寬列高
  }
}
.bag-compact__bag { font-weight: 600; }
.bag-compact__num {
  font-size: 1.35rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.bag-compact__time {
  font-size: 0.95rem;
  color: rgba(var(--v-theme-on-surface), 0.7);
}
</style>

<template>
  <div>
    <AppHeader :title="$t('page.bagCheck.title')" :subtitle="$t('page.bagCheck.subtitle')" icon="tabler-packages">
      <template #actions>
        <div class="d-flex ga-2">
          <DisplayLauncher
            route="/bag-check"
            window-label="display-bagcheck"
            :title="$t('page.bagCheck.title')"
          />
          <VBtn color="default" variant="tonal" :disabled="!bags.length" @click="clearList">
            <VIcon icon="tabler-eraser" size="16" class="me-1" />{{ $t('page.bagCheck.clearList') }}
          </VBtn>
        </div>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <!-- 控制列:視圖模式 + 狀態篩選(即時切換,記 localStorage)-->
    <div v-if="bags.length" class="d-flex flex-wrap align-center ga-3 mb-4">
      <VBtnToggle v-model="viewMode" mandatory density="comfortable" variant="flat" divided color="primary">
        <VBtn value="cards" size="small" :ripple="false"><VIcon icon="tabler-layout-grid" size="18" class="me-1" />{{ $t('page.bagCheck.view.cards') }}</VBtn>
        <VBtn value="compact" size="small" :ripple="false"><VIcon icon="tabler-list" size="18" class="me-1" />{{ $t('page.bagCheck.view.compact') }}</VBtn>
        <VBtn value="missing" size="small" :ripple="false"><VIcon icon="tabler-alert-circle" size="18" class="me-1" />{{ $t('page.bagCheck.view.missing') }}</VBtn>
      </VBtnToggle>

      <VSpacer />

      <!-- 狀態篩選:缺件總覽本身只列有缺,故不顯示篩選,改顯示總缺件數 -->
      <VBtnToggle
        v-if="viewMode !== 'missing'"
        v-model="stateFilter"
        mandatory
        density="comfortable"
        variant="flat"
        divided
      >
        <VBtn value="all" size="small" :ripple="false">{{ $t('page.bagCheck.filter.all') }} {{ bags.length }}</VBtn>
        <VBtn value="missing" size="small" color="warning" :ripple="false">{{ $t('page.bagCheck.filter.missing') }} {{ missingBagCount }}</VBtn>
        <VBtn value="complete" size="small" color="success" :ripple="false">{{ $t('page.bagCheck.filter.complete') }} {{ completeBagCount }}</VBtn>
      </VBtnToggle>
      <VChip v-else color="warning" label>
        <VIcon icon="tabler-alert-circle" size="16" start />
        {{ $t('page.bagCheck.totalMissingItems') }}:{{ totalMissingItems }}（{{ missingBagCount }} {{ $t('page.bagCheck.bagsUnit') }}）
      </VChip>
    </div>

    <!-- 無資料 -->
    <VCard v-if="!bags.length" class="py-10">
      <div class="d-flex flex-column align-center justify-center text-medium-emphasis">
        <VIcon icon="tabler-package-off" size="48" class="mb-3" />
        <div class="text-h6 mb-1">{{ $t('page.bagCheck.empty') }}</div>
        <div class="text-body-2">{{ $t('page.bagCheck.emptyHint') }}</div>
      </div>
    </VCard>

    <!-- 缺件總覽:扁平列出每個有缺的袋 + 其未印件 -->
    <template v-else-if="viewMode === 'missing'">
      <VCard v-if="!missingOverview.length" class="py-10">
        <div class="d-flex flex-column align-center justify-center text-success">
          <VIcon icon="tabler-circle-check" size="48" class="mb-3" />
          <div class="text-h6">{{ $t('page.bagCheck.allComplete') }}</div>
        </div>
      </VCard>
      <VCard v-else>
        <VList density="compact" class="py-0">
          <template v-for="(b, bi) in missingOverview" :key="b.package_sn">
            <VDivider v-if="bi > 0" />
            <VListItem class="py-3">
              <VListItemTitle class="d-flex align-center flex-wrap ga-2">
                <span class="bag-missing__bag">{{ b.package_sn }}</span>
                <VChip color="warning" size="small" label class="font-weight-bold">{{ $t('page.bagCheck.status.missing', { n: b.missing }) }}</VChip>
              </VListItemTitle>
              <div class="bag-missing__nums mt-1">
                <span v-for="o in b.unprinted" :key="o.shipping_no" class="bag-missing__num">{{ o.shipping_no || '—' }}</span>
              </div>
            </VListItem>
          </template>
        </VList>
      </VCard>
    </template>

    <!-- 精簡列表:每袋一行 -->
    <template v-else-if="viewMode === 'compact'">
      <VCard v-if="!filteredBags.length" class="py-10 text-center text-medium-emphasis">
        <div class="text-body-1">{{ $t('page.bagCheck.noMatch') }}</div>
      </VCard>
      <VCard v-else>
        <VTable class="bag-compact">
          <thead>
            <tr>
              <th class="text-start">{{ $t('page.bagCheck.col.packageSn') }}</th>
              <th class="text-center" style="width: 90px;">{{ $t('page.bagCheck.total') }}</th>
              <th class="text-center" style="width: 90px;">{{ $t('page.bagCheck.printed') }}</th>
              <th class="text-center" style="width: 90px;">{{ $t('page.bagCheck.missing') }}</th>
              <th class="text-center" style="width: 120px;">{{ $t('page.bagCheck.lastRequestAt') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="bag in filteredBags" :key="bag.package_sn">
              <td class="text-start bag-compact__bag">
                <VIcon :icon="bag._stateIcon" :color="bag._stateColor" size="20" class="me-2" />
                {{ bag.package_sn }}
              </td>
              <td class="text-center bag-compact__num">{{ bag.total }}</td>
              <td class="text-center bag-compact__num text-success">{{ bag.printed }}</td>
              <td class="text-center bag-compact__num" :class="bag.missing > 0 ? 'text-warning font-weight-bold' : 'text-disabled'">{{ bag.missing }}</td>
              <td class="text-center bag-compact__time">{{ formatTimeShort(bag.last_request_at) }}</td>
            </tr>
          </tbody>
        </VTable>
      </VCard>
    </template>

    <!-- 詳細卡片:Masonry 瀑布流(row-major,4 個一列填滿),展開/收合/刷新由 ResizeObserver 自動重排 -->
    <template v-else>
      <VCard v-if="!filteredBags.length" class="py-10 text-center text-medium-emphasis">
        <div class="text-body-1">{{ $t('page.bagCheck.noMatch') }}</div>
      </VCard>
      <div v-else ref="masonryEl" class="bag-masonry" :class="{ 'bag-masonry--ready': masonryReady }">
      <!-- columnWidth 量測基準(不參與佈局,僅供 Masonry 計算欄寬) -->
      <div class="bag-masonry__sizer" />
      <VCard v-for="bag in filteredBags" :key="bag.package_sn" class="bag-card bag-masonry__item">
          <VCardItem>
            <template #prepend>
              <VAvatar :color="bag._stateColor" variant="tonal" rounded>
                <VIcon icon="tabler-package" />
              </VAvatar>
            </template>
            <VCardTitle class="text-truncate">{{ bag.package_sn }}</VCardTitle>
            <VCardSubtitle class="text-caption">
              {{ $t('page.bagCheck.lastRequestAt') }}:{{ formatTime(bag.last_request_at) }}
            </VCardSubtitle>
            <template #append>
              <!-- 完整 → 綠徽章;中途被打斷 → 紅徽章(此為異常,務必顯眼)。
                   一般缺件不顯示徽章(與下方「未印」統計重複)。 -->
              <VChip v-if="bag._state === 'complete'" :color="STATE_COLOR.complete" size="small" label>
                <VIcon :icon="STATE_ICON.complete" size="15" start />
                {{ $t('page.bagCheck.status.complete') }}
              </VChip>
              <VChip v-else-if="isInterrupted(bag)" :color="STATE_COLOR.interrupted" size="small" label>
                <VIcon :icon="STATE_ICON.interrupted" size="15" start />
                {{ $t('page.bagCheck.status.interrupted') }}
              </VChip>
            </template>
          </VCardItem>

          <!-- 件數統計 -->
          <VCardText class="py-2">
            <div class="d-flex justify-space-around text-center">
              <div class="bag-stat flex-column">
                <span class="bag-stat__num">{{ bag.total }}</span>
                <span class="text-caption text-medium-emphasis">{{ $t('page.bagCheck.total') }}</span>
              </div>
              <div class="bag-stat flex-column">
                <span class="bag-stat__num text-success">{{ bag.printed }}</span>
                <span class="text-caption text-medium-emphasis">{{ $t('page.bagCheck.printed') }}</span>
              </div>
              <div class="bag-stat flex-column">
                <span class="bag-stat__num" :class="bag.missing > 0 ? 'text-warning' : 'text-disabled'">{{ bag.missing }}</span>
                <span class="text-caption text-medium-emphasis">{{ $t('page.bagCheck.missing') }}</span>
              </div>
            </div>
          </VCardText>

          <!-- 訂單明細:可收合(預設收合) -->
          <template v-if="bag.orders && bag.orders.length">
            <div
              class="bag-card__toggle"
              role="button"
              tabindex="0"
              @click="toggleExpand(bag)"
              @keydown.enter="toggleExpand(bag)"
              @keydown.space.prevent="toggleExpand(bag)"
            >
              <span class="text-caption font-weight-medium text-primary">
                {{ isExpanded(bag) ? $t('page.bagCheck.collapse') : $t('page.bagCheck.expand') }}
              </span>
              <VIcon
                :icon="isExpanded(bag) ? 'tabler-chevron-up' : 'tabler-chevron-down'"
                size="18"
                class="text-primary ms-1"
              />
            </div>
            <!-- 即時顯示/隱藏:卡片高度一步到位,toggleExpand 單次 layout,Masonry 過渡讓鄰卡平滑滑動補位 -->
            <div v-show="isExpanded(bag)">
                <VTable density="compact" class="bag-card__orders">
                  <thead>
                    <tr>
                      <th class="text-center" style="width: 60px;">{{ $t('page.bagCheck.col.provider') }}</th>
                      <th class="text-start">{{ $t('page.bagCheck.col.shippingNo') }}</th>
                      <th class="text-center" style="width: 90px;">{{ $t('page.bagCheck.col.printTime') }}</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr
                      v-for="(o, i) in bag.orders"
                      :key="o.shipping_no + i"
                    >
                      <td class="text-center">{{ o.shipping_provider || '—' }}</td>
                      <td class="text-start">{{ o.shipping_no || '—' }}</td>
                      <td class="text-center">
                        <span v-if="isPrinted(o)" class="text-success">
                          {{ formatTimeShort(o.last_print_time) }}
                        </span>
                        <span v-else class="text-disabled">—</span>
                      </td>
                    </tr>
                  </tbody>
                </VTable>
              </div>
          </template>
        </VCard>
      </div>
    </template>
  </div>
</template>
