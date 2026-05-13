<script setup>
import DefaultLayout from '@/layouts/DefaultLayout.vue'
import ThemeCustomizer from '@/components/ThemeCustomizer.vue'
import { useThemeApply } from '@/composables/useThemeApply'

useThemeApply()
</script>

<template>
  <VApp>
    <DefaultLayout>
      <RouterView />
    </DefaultLayout>
    <ThemeCustomizer />
  </VApp>
</template>
