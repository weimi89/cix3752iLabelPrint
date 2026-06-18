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

// 袋卡狀態:missing(有缺漏) / complete(完整)。
// 載入失敗(散單 / 未登入 / 雲端失敗)後端不建卡,前端不需處理該狀態。
const bagState = bag => ((bag.missing || 0) > 0 ? 'missing' : 'complete')
const STATE_COLOR = { missing: 'warning', complete: 'success' }
const STATE_ICON = { missing: 'tabler-alert-circle', complete: 'tabler-circle-check' }

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
}

// 袋清單變動 → 重載 Masonry;清單清空 → 銷毀(容器會被 v-if 移除,需丟棄舊實例)
watch(bags, async val => {
  if (!val.length) {
    ro?.disconnect()
    masonry?.destroy()
    masonry = null
    return
  }
  await initMasonry()
})

onMounted(async () => {
  await load()
  await initMasonry()
  // 即時:後端每次更新袋件清單就 emit,前端直接套用快照(不輪詢、不重查雲端)
  if (isTauriRuntime) {
    unlisten = await listen('bag-check-updated', evt => {
      bags.value = evt.payload || []
    })
  }
})
onUnmounted(() => {
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

    <!-- 無資料 -->
    <VCard v-if="!bags.length" class="py-10">
      <div class="d-flex flex-column align-center justify-center text-medium-emphasis">
        <VIcon icon="tabler-package-off" size="48" class="mb-3" />
        <div class="text-h6 mb-1">{{ $t('page.bagCheck.empty') }}</div>
        <div class="text-body-2">{{ $t('page.bagCheck.emptyHint') }}</div>
      </div>
    </VCard>

    <!-- 袋卡:Masonry 瀑布流(row-major,4 個一列填滿),展開/收合/刷新由 ResizeObserver 自動重排 -->
    <div v-else ref="masonryEl" class="bag-masonry">
      <!-- columnWidth 量測基準(不參與佈局,僅供 Masonry 計算欄寬) -->
      <div class="bag-masonry__sizer" />
      <VCard v-for="bag in bags" :key="bag.package_sn" class="bag-card bag-masonry__item">
          <VCardItem>
            <template #prepend>
              <VAvatar :color="STATE_COLOR[bagState(bag)]" variant="tonal" rounded>
                <VIcon icon="tabler-package" />
              </VAvatar>
            </template>
            <VCardTitle class="text-truncate">{{ bag.package_sn }}</VCardTitle>
            <VCardSubtitle class="text-caption">
              {{ $t('page.bagCheck.lastRequestAt') }}:{{ formatTime(bag.last_request_at) }}
            </VCardSubtitle>
            <template #append>
              <!-- 未印(missing)狀態的徽章與下方「未印」統計重複,故只在完整時顯示 -->
              <VChip v-if="bagState(bag) === 'complete'" :color="STATE_COLOR.complete" size="small" label>
                <VIcon :icon="STATE_ICON.complete" size="15" start />
                {{ $t('page.bagCheck.status.complete') }}
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
  </div>
</template>
