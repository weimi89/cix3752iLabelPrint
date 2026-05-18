<script setup>
/**
 * Table 分頁元件 — 結構完全對齊 Materio resources/js/components/TablePagination.vue
 */

const props = defineProps({
  page: { type: Number, required: true },
  perPage: { type: Number, required: true },
  total: { type: Number, required: true },
  pageSizes: { type: Array, default: () => [10, 25, 50, 100, 250, 500] },
  header: { type: Boolean, default: false },
})

const emit = defineEmits(['update:page', 'update:perPage'])

const totalPages = computed(() => Math.max(1, Math.ceil(props.total / props.perPage)))
const pageOptions = computed(() => Array.from({ length: totalPages.value }, (_, i) => i + 1))

const setPage = n => {
  const clamped = Math.max(1, Math.min(totalPages.value, n))
  if (clamped !== props.page) emit('update:page', clamped)
}

const onFirst = () => setPage(1)
const onPrev = () => setPage(props.page - 1)
const onNext = () => setPage(props.page + 1)
const onLast = () => setPage(totalPages.value)

const separator = n => new Intl.NumberFormat('en-US').format(n || 0)
</script>

<template>
  <div class="py-3 px-6">
    <div class="d-flex flex-wrap ga-2 align-center justify-center">
      <!-- 右側:跳頁 + 翻頁(header mode 也顯示) -->
      <div class="d-flex align-center ga-2 ml-md-auto" :class="{ 'hidden-page-change': !header && false }">
        <div>{{ $t('pagination.pagePrefix') }}</div>
        <div>
          <VSelect
            :items="pageOptions"
            :model-value="page"
            @update:model-value="setPage"
          />
        </div>
        <div>{{ $t('pagination.pageSuffix') }}</div>
        <div class="d-flex ga-1">
          <VBtn icon variant="text" size="small" :disabled="page === 1" @click="onFirst">
            <VIcon icon="tabler-player-skip-back" size="22" />
          </VBtn>
          <VBtn icon variant="text" size="small" :disabled="page === 1" @click="onPrev">
            <VIcon icon="tabler-chevron-left" size="22" />
          </VBtn>
          <VBtn icon variant="text" size="small" :disabled="page === totalPages" @click="onNext">
            <VIcon icon="tabler-chevron-right" size="22" />
          </VBtn>
          <VBtn icon variant="text" size="small" :disabled="page === totalPages" @click="onLast">
            <VIcon icon="tabler-player-skip-forward" size="22" />
          </VBtn>
        </div>
      </div>

      <!-- 左側:總筆數 + 每頁(header mode 隱藏) -->
      <div
        v-if="!header"
        class="d-flex flex-wrap ga-2 align-center justify-center order-sm-first"
      >
        <span class="d-md-none d-lg-flex text-nowrap">
          {{ $t('pagination.summary', { total: separator(total), pages: separator(totalPages) }) }}
        </span>
        <div class="d-flex align-center ga-2">
          <span class="text-nowrap">{{ $t('pagination.perPagePrefix') }}</span>
          <div>
            <VSelect
              :items="pageSizes.map(String)"
              :model-value="String(perPage)"
              @update:model-value="v => emit('update:perPage', Number(v))"
            />
          </div>
          <span class="text-nowrap">{{ $t('pagination.perPageSuffix') }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.text-nowrap {
  white-space: nowrap !important;
}
</style>
