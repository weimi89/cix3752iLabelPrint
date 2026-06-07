<script setup>
import { bagCheckSnapshot, bagCheckClear } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'

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

// 袋卡狀態:load_failed(清單載入失敗) / missing(有缺漏) / complete(完整)
const bagState = bag => {
  if (bag.status === 'load_failed') return 'load_failed'
  return (bag.missing || 0) > 0 ? 'missing' : 'complete'
}
const STATE_COLOR = { load_failed: 'warning', missing: 'warning', complete: 'success' }
const STATE_ICON = { load_failed: 'tabler-alert-triangle', missing: 'tabler-alert-circle', complete: 'tabler-circle-check' }

const isPrinted = o => !!(o.last_print_time && String(o.last_print_time).trim())
const formatTime = s => (s ? String(s).replace('T', ' ').slice(0, 19) : '')

onMounted(async () => {
  await load()
  // 即時:後端每次更新袋件清單就 emit,前端直接套用快照(不輪詢、不重查雲端)
  if (isTauriRuntime) {
    unlisten = await listen('bag-check-updated', evt => {
      bags.value = evt.payload || []
    })
  }
})
onUnmounted(() => {
  if (unlisten) { unlisten(); unlisten = null }
})
</script>

<style scoped lang="scss">
.bag-card {
  // 不設 block-size:100%,且外層 VRow 用 align="start" 不強制等高;
  // 否則內容較少(列數少)的卡片會被硬拉高,撐開上半部使統計區與他卡對不齊。
  // 各卡頂端對齊、上半部自然一致,卡片總高依袋內件數不同而異(屬正常)。
  // flex 容器內 text-truncate 需要 min-width:0 才生效(防長袋號換行)
  :deep(.v-card-item__content) {
    min-inline-size: 0;
  }
}
.bag-card__orders {
  th, td { white-space: nowrap; }
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

    <!-- 袋卡 -->
    <VRow v-else dense align="start">
      <VCol v-for="bag in bags" :key="bag.package_sn" cols="12" md="4">
        <VCard class="bag-card">
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
              <VChip :color="STATE_COLOR[bagState(bag)]" size="small" label>
                <VIcon :icon="STATE_ICON[bagState(bag)]" size="15" start />
                <template v-if="bagState(bag) === 'load_failed'">{{ $t('page.bagCheck.status.loadFailed') }}</template>
                <template v-else-if="bagState(bag) === 'missing'">{{ $t('page.bagCheck.status.missing', { n: bag.missing }) }}</template>
                <template v-else>{{ $t('page.bagCheck.status.complete') }}</template>
              </VChip>
            </template>
          </VCardItem>

          <!-- 載入失敗說明 -->
          <VCardText v-if="bag.status === 'load_failed'" class="pt-0">
            <VAlert type="warning" variant="tonal" density="compact" class="mb-3">
              {{ $t('page.bagCheck.loadFailedHint') }}<template v-if="bag.message"> ({{ bag.message }})</template>
            </VAlert>
          </VCardText>

          <!-- 件數統計(僅清單已載入) -->
          <VCardText v-else class="py-2">
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

          <VDivider />

          <!-- 訂單清單 -->
          <VTable density="compact" class="bag-card__orders">
            <thead>
              <tr>
                <th class="text-center" style="width: 44px;">#</th>
                <th class="text-start">{{ $t('page.bagCheck.col.shippingNo') }}</th>
                <th class="text-center" style="width: 60px;">{{ $t('page.bagCheck.col.provider') }}</th>
                <th class="text-center" style="min-width: 150px;">{{ $t('page.bagCheck.col.printTime') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(o, i) in bag.orders"
                :key="o.shipping_no + i"
              >
                <td class="text-center text-disabled">{{ i + 1 }}</td>
                <td class="text-start">{{ o.shipping_no || '—' }}</td>
                <td class="text-center">{{ o.shipping_provider || '—' }}</td>
                <td class="text-center">
                  <span v-if="isPrinted(o)" class="text-success d-inline-flex align-center gap-1">
                    <VIcon icon="tabler-circle-check" size="16" />{{ formatTime(o.last_print_time) }}
                  </span>
                  <span v-else class="text-disabled">—</span>
                </td>
              </tr>
            </tbody>
          </VTable>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>
