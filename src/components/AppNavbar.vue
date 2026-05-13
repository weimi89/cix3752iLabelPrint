<script setup>
import { useThemeStore } from '@/stores/theme'
import UserProfileMenu from './UserProfileMenu.vue'

const themeStore = useThemeStore()

const cycleThemeMode = () => {
  const order = ['light', 'dark', 'system']
  const idx = order.indexOf(themeStore.config.mode)
  themeStore.set({ mode: order[(idx + 1) % order.length] })
}

const themeIcon = computed(() => {
  const m = themeStore.config.mode
  if (m === 'light') return 'tabler-sun'
  if (m === 'dark') return 'tabler-moon'
  return 'tabler-device-desktop'
})
</script>

<template>
  <header class="layout-navbar">
    <div class="layout-navbar__content">
      <VSpacer />
      <button class="layout-navbar__btn" title="切換主題模式" @click="cycleThemeMode">
        <VIcon :icon="themeIcon" size="22" />
      </button>
      <button class="layout-navbar__btn" title="待辦事項">
        <VIcon icon="tabler-checkbox" size="22" />
      </button>
      <UserProfileMenu />
    </div>
  </header>
</template>

<style scoped lang="scss">
.layout-navbar {
  block-size: 54px;
  background: transparent;
  position: sticky;
  inset-block-start: 0;
  z-index: 11;
  padding: 0 110px;

  &__content {
    display: flex;
    align-items: center;
    block-size: 100%;
    padding: 0 24px;
  }

  &__btn {
    inline-size: 38px;
    block-size: 38px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(var(--v-theme-on-surface), 0.78);
    transition: background 0.15s, color 0.15s;
    cursor: pointer;
    background: transparent;
    border: 0;
    margin-inline: 2px;

    &:hover {
      background: rgba(var(--v-theme-on-surface), 0.06);
      color: rgb(var(--v-theme-primary));
    }
  }
}

@media (max-width: 1599px) {
  .layout-navbar { padding: 0 24px; }
  .layout-navbar__content { padding: 0; }
}
</style>

<style lang="scss">
.app-navbar__avatar,
.layout-navbar__avatar {
  position: relative;
  margin-inline-start: 0.25rem;
  cursor: pointer;

  &::after {
    content: '';
    position: absolute;
    inset-block-end: 1px;
    inset-inline-end: 1px;
    inline-size: 10px;
    block-size: 10px;
    border-radius: 50%;
    background: rgb(var(--v-theme-success));
    border: 2px solid rgb(var(--v-theme-surface));
    opacity: 0.5;
  }
  &.is-online::after { opacity: 1; }
}
.app-navbar__avatar-img,
.layout-navbar__avatar-img {
  background: rgba(var(--v-theme-primary), 0.12) !important;
}
</style>
