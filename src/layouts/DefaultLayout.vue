<script setup>
import { VerticalNavLayout } from '@layouts'
import AppNavbar from '@/components/AppNavbar.vue'
import { navItems } from '@/config/navConfig'
import { useSkins } from '@core/composable/useSkins'

const { layoutAttrs } = useSkins()
</script>

<template>
  <VerticalNavLayout
    :home-url="'/'"
    :nav-items="navItems"
    :vertical-nav-attrs="layoutAttrs.verticalNavAttrs"
  >
    <!-- 👉 Navbar -->
    <template #navbar="{ toggleVerticalOverlayNavActive }">
      <AppNavbar :toggle-vertical-overlay-nav-active="toggleVerticalOverlayNavActive" />
    </template>

    <!-- 👉 Pages -->
    <slot />
  </VerticalNavLayout>
</template>

<style lang="scss">
/* 載入 @layouts plugin 的 layout styles */
@use "@layouts/styles/default-layout";
</style>
