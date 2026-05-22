<script setup>
import { useTheme } from 'vuetify'
import { hexToRgb } from '@layouts/utils'
import DefaultLayout from '@/layouts/DefaultLayout.vue'
import { useThemeApply } from '@/composables/useThemeApply'
import { installZoomShortcuts } from '@/composables/useZoom'

useThemeApply()
installZoomShortcuts()

const { global } = useTheme()
</script>

<template>
  <VApp :style="`--v-global-theme-primary: ${hexToRgb(global.current.value.colors.primary)}`">
    <DefaultLayout>
      <RouterView />
    </DefaultLayout>
  </VApp>
</template>
