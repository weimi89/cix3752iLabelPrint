<script setup>
import { useLocale } from '@/composables/useLocale'

const { currentLocale, availableLocales, setLocale } = useLocale()

const currentLabel = computed(() =>
  availableLocales.find(l => l.code === currentLocale.value)?.label ?? currentLocale.value,
)
</script>

<template>
  <VMenu offset="8">
    <template #activator="{ props: actv }">
      <!-- 手機只留圖示:導覽列在手機同時要放統計數字、清關進度、登出、網路狀態,
           「繁體中文」四個字一擺,數字一大整列就被擠出畫面 -->
      <VBtn
        v-bind="actv"
        variant="text"
        color="default"
        class="text-none locale-btn"
      >
        <VIcon icon="tabler-language" size="22" />
        <span class="d-none d-sm-inline ms-1">{{ currentLabel }}</span>
      </VBtn>
    </template>
    <VList density="compact" min-width="160" class="py-1">
      <VListItem
        v-for="locale in availableLocales"
        :key="locale.code"
        :active="locale.code === currentLocale"
        :title="locale.label"
        @click="setLocale(locale.code)"
      >
        <template #append>
          <VIcon
            v-if="locale.code === currentLocale"
            icon="tabler-check"
            size="18"
            color="primary"
          />
        </template>
      </VListItem>
    </VList>
  </VMenu>
</template>

<style scoped lang="scss">
@media (max-width: 599.98px) {
  // 疊三層 .v-btn 才蓋得過全域按鈕樣式的 20px 內距(那條也是 !important)
  .v-btn.v-btn.locale-btn {
    min-inline-size: 0;
    padding-inline: 8px !important;
  }
}
</style>
