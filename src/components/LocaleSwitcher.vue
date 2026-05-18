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
      <VBtn
        v-bind="actv"
        variant="text"
        color="default"
        class="text-none"
        :prepend-icon="'tabler-language'"
      >
        {{ currentLabel }}
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
