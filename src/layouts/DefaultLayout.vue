<script setup>
import AppNavbar from '@/components/AppNavbar.vue'
import AppLogo from '@/components/AppLogo.vue'
import { navSections } from '@/config/navConfig'
import { useThemeStore } from '@/stores/theme'

const themeStore = useThemeStore()

// 收合：rail = true；水平：暫時也走 rail 並提示；垂直：展開
const rail = computed({
  get: () => themeStore.config.layout === 'collapsed',
  set: v => themeStore.set({ layout: v ? 'collapsed' : 'vertical' }),
})
</script>

<template>
  <VNavigationDrawer
    permanent
    :rail="rail"
    rail-width="80"
    width="260"
    border="0"
  >
    <a class="app-logo" href="#/">
      <AppLogo class="app-logo__icon" />
      <h1 v-if="!rail" class="app-logo__title">智配通</h1>
      <VSpacer v-if="!rail" />
      <button
        type="button"
        class="app-logo__toggle"
        :title="rail ? '展開側欄' : '收合側欄'"
        @click.prevent.stop="rail = !rail"
      >
        <VIcon :icon="rail ? 'tabler-chevron-right' : 'tabler-circle-dot'" size="20" />
      </button>
    </a>

    <VList nav>
      <template v-for="section in navSections" :key="section.title">
        <VListSubheader v-if="!rail">{{ section.title }}</VListSubheader>
        <template v-for="item in section.items" :key="item.title">
          <VListGroup v-if="item.children" :value="item.title">
            <template #activator="{ props: actv }">
              <VListItem v-bind="actv" :prepend-icon="item.icon" :title="item.title" />
            </template>
            <VListItem
              v-for="child in item.children"
              :key="child.title"
              :to="child.to"
              :title="child.title"
              class="nav-sub-item"
            />
          </VListGroup>
          <VListItem
            v-else
            :to="item.to"
            :prepend-icon="item.icon"
            :title="item.title"
          />
        </template>
      </template>
    </VList>
  </VNavigationDrawer>

  <VMain>
    <div class="layout-content-wrapper">
      <AppNavbar />
      <main class="layout-page-content">
        <slot />
      </main>
    </div>
  </VMain>
</template>

<style scoped>
.app-logo {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 20px 2px 20px 8px;
  margin: 0 12px;
  min-height: 71px;
  color: rgb(var(--v-theme-primary));
  text-decoration: none;
}
.app-logo__icon { flex-shrink: 0; }
.app-logo__title {
  font-size: 22px;
  font-weight: 700;
  line-height: normal;
  color: rgba(var(--v-theme-on-surface), 0.9);
  margin: 0;
}
.app-logo__toggle {
  inline-size: 28px;
  block-size: 28px;
  border-radius: 50%;
  background: transparent;
  border: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: rgba(var(--v-theme-on-surface), 0.55);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.app-logo__toggle:hover {
  background: rgba(var(--v-theme-primary), 0.08);
  color: rgb(var(--v-theme-primary));
}

.layout-content-wrapper { padding: 0; }

.layout-page-content {
  padding: 24px;
  margin: 4px 110px 0;
}

@media (max-width: 1599px) {
  .layout-page-content { margin: 4px 24px 0; }
}
</style>
