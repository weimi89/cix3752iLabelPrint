<script setup>
import { useThemeStore, PRIMARY_PRESETS } from '@/stores/theme'

const themeStore = useThemeStore()

const config = computed({
  get: () => themeStore.config,
  set: v => themeStore.set(v),
})

const setPrimary = c => themeStore.set({ primary: c })
const setMode = m => themeStore.set({ mode: m })
const setStyle = s => themeStore.set({ style: s })
const setLayout = l => themeStore.set({ layout: l })
const setContentWidth = w => themeStore.set({ contentWidth: w })

const close = () => { themeStore.customizerOpen = false }
const reset = () => themeStore.reset()

const isCustomPrimary = computed(
  () => !PRIMARY_PRESETS.some(p => p.value.toLowerCase() === themeStore.config.primary.toLowerCase()),
)
</script>

<template>
  <VNavigationDrawer
    v-model="themeStore.customizerOpen"
    location="right"
    temporary
    width="380"
    class="theme-customizer"
  >
    <div class="customizer-header">
      <div>
        <div class="text-h6 font-weight-bold">主題定制</div>
        <div class="text-caption text-medium-emphasis">即時自訂和預覽</div>
      </div>
      <div class="d-flex align-center" style="gap: 4px;">
        <VBtn icon="tabler-refresh" size="small" variant="text" @click="reset">
          <VIcon icon="tabler-refresh" />
        </VBtn>
        <VBtn icon="tabler-x" size="small" variant="text" @click="close">
          <VIcon icon="tabler-x" />
        </VBtn>
      </div>
    </div>
    <VDivider />

    <div class="customizer-body">
      <!-- 主題 -->
      <div class="section-label">主題</div>

      <div class="field-label">主色</div>
      <div class="color-grid">
        <button
          v-for="c in PRIMARY_PRESETS"
          :key="c.value"
          class="color-swatch"
          :class="{ 'is-selected': themeStore.config.primary.toLowerCase() === c.value.toLowerCase() }"
          :style="{ background: c.value }"
          :title="c.label"
          @click="setPrimary(c.value)"
        />
        <label class="color-swatch color-swatch--custom" :class="{ 'is-selected': isCustomPrimary }">
          <input type="color" :value="themeStore.config.primary" @input="e => setPrimary(e.target.value)" />
          <VIcon icon="tabler-pencil" size="18" />
        </label>
      </div>

      <div class="field-label mt-5">主題</div>
      <div class="mode-grid">
        <button class="mode-card" :class="{ 'is-selected': themeStore.config.mode === 'light' }" @click="setMode('light')">
          <VIcon icon="tabler-sun" size="28" />
          <span>明亮模式</span>
        </button>
        <button class="mode-card" :class="{ 'is-selected': themeStore.config.mode === 'dark' }" @click="setMode('dark')">
          <VIcon icon="tabler-moon" size="28" />
          <span>暗黑模式</span>
        </button>
        <button class="mode-card" :class="{ 'is-selected': themeStore.config.mode === 'system' }" @click="setMode('system')">
          <VIcon icon="tabler-device-desktop" size="28" />
          <span>系統模式</span>
        </button>
      </div>

      <div class="field-label mt-5">樣式</div>
      <div class="style-grid">
        <button class="style-card" :class="{ 'is-selected': themeStore.config.style === 'default' }" @click="setStyle('default')">
          <div class="style-preview style-preview--default" />
          <span>預設</span>
        </button>
        <button class="style-card" :class="{ 'is-selected': themeStore.config.style === 'bordered' }" @click="setStyle('bordered')">
          <div class="style-preview style-preview--bordered" />
          <span>有邊框</span>
        </button>
      </div>

      <div class="d-flex align-center justify-space-between mt-5">
        <span class="text-body-2">半暗色選單</span>
        <VSwitch v-model="themeStore.config.semiDark" hide-details inset density="compact" @update:model-value="v => themeStore.set({ semiDark: v })" />
      </div>

      <VDivider class="my-5" />

      <!-- 布局 -->
      <div class="section-label">布局</div>

      <div class="field-label">布局</div>
      <div class="layout-grid">
        <button class="layout-card" :class="{ 'is-selected': themeStore.config.layout === 'vertical' }" @click="setLayout('vertical')">
          <div class="layout-preview layout-preview--vertical" />
          <span>垂直</span>
        </button>
        <button class="layout-card" :class="{ 'is-selected': themeStore.config.layout === 'collapsed' }" @click="setLayout('collapsed')">
          <div class="layout-preview layout-preview--collapsed" />
          <span>收合</span>
        </button>
        <button class="layout-card" :class="{ 'is-selected': themeStore.config.layout === 'horizontal' }" @click="setLayout('horizontal')">
          <div class="layout-preview layout-preview--horizontal" />
          <span>水平</span>
        </button>
      </div>

      <div class="field-label mt-5">內容</div>
      <div class="layout-grid">
        <button class="layout-card" :class="{ 'is-selected': themeStore.config.contentWidth === 'compact' }" @click="setContentWidth('compact')">
          <div class="layout-preview layout-preview--compact" />
          <span>緊湊</span>
        </button>
        <button class="layout-card" :class="{ 'is-selected': themeStore.config.contentWidth === 'wide' }" @click="setContentWidth('wide')">
          <div class="layout-preview layout-preview--wide" />
          <span>寬鬆</span>
        </button>
      </div>
    </div>
  </VNavigationDrawer>
</template>

<style scoped lang="scss">
.customizer-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.875rem 1rem;
}

.customizer-body {
  padding: 1rem 1rem 2rem;
}

.section-label {
  display: inline-block;
  font-size: 0.75rem;
  font-weight: 600;
  background: rgba(var(--v-theme-primary), 0.12);
  color: rgb(var(--v-theme-primary));
  padding: 2px 12px;
  border-radius: 4px;
  margin-block-end: 1rem;
}

.field-label {
  font-size: 0.875rem;
  font-weight: 500;
  margin-block-end: 0.5rem;
}

// 色塊
.color-grid {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}
.color-swatch {
  inline-size: 44px;
  block-size: 44px;
  border-radius: 8px;
  border: 2px solid transparent;
  cursor: pointer;
  outline: 2px solid transparent;
  outline-offset: 2px;
  transition: outline-color 0.15s;

  &.is-selected {
    outline-color: currentColor;
  }
  &--custom {
    background: transparent !important;
    border: 1px dashed rgba(var(--v-theme-on-surface), 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(var(--v-theme-on-surface), 0.6);
    position: relative;
    overflow: hidden;

    input[type="color"] {
      position: absolute;
      inset: 0;
      opacity: 0;
      cursor: pointer;
    }
  }
}

// 模式卡片
.mode-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 0.5rem;
}
.mode-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  padding: 0.75rem 0.5rem;
  border-radius: 8px;
  border: 1px solid rgba(var(--v-theme-on-surface), 0.12);
  background: transparent;
  cursor: pointer;
  font-size: 0.8125rem;
  color: rgba(var(--v-theme-on-surface), 0.85);

  &.is-selected {
    background: rgba(var(--v-theme-primary), 0.10);
    border-color: rgb(var(--v-theme-primary));
    color: rgb(var(--v-theme-primary));
  }
}

// 樣式 / 布局卡片
.style-grid, .layout-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 0.5rem;
}
.layout-grid {
  grid-template-columns: repeat(3, 1fr);
}
.layout-grid:has(.layout-card:nth-child(2):last-child) {
  grid-template-columns: repeat(2, 1fr);
}
.style-card, .layout-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem;
  border-radius: 8px;
  border: 1px solid rgba(var(--v-theme-on-surface), 0.12);
  background: transparent;
  cursor: pointer;
  font-size: 0.8125rem;

  &.is-selected {
    border-color: rgb(var(--v-theme-primary));
    box-shadow: 0 0 0 1px rgb(var(--v-theme-primary));
  }
}

// 預覽縮圖（簡單線稿）
.style-preview, .layout-preview {
  inline-size: 100%;
  block-size: 56px;
  border-radius: 4px;
  background: rgba(var(--v-theme-on-surface), 0.04);
  position: relative;
  overflow: hidden;
}
.style-preview--default::before,
.style-preview--bordered::before {
  content: '';
  position: absolute;
  inset-block-start: 6px;
  inset-inline: 6px;
  block-size: 6px;
  background: rgba(var(--v-theme-on-surface), 0.25);
  border-radius: 3px;
}
.style-preview--bordered {
  border: 1px solid rgba(var(--v-theme-on-surface), 0.25);
}
.layout-preview--vertical::before,
.layout-preview--collapsed::before {
  content: '';
  position: absolute;
  inset-block: 6px;
  inset-inline-start: 6px;
  inline-size: 14px;
  background: rgba(var(--v-theme-on-surface), 0.2);
  border-radius: 2px;
}
.layout-preview--collapsed::before { inline-size: 6px; }
.layout-preview--horizontal::before {
  content: '';
  position: absolute;
  inset-block-start: 6px;
  inset-inline: 6px;
  block-size: 6px;
  background: rgba(var(--v-theme-on-surface), 0.2);
  border-radius: 2px;
}
.layout-preview--compact::after,
.layout-preview--wide::after {
  content: '';
  position: absolute;
  inset-block-start: 12px;
  inset-block-end: 6px;
  inset-inline-start: 12px;
  inset-inline-end: 12px;
  border-radius: 3px;
  background: rgba(var(--v-theme-on-surface), 0.08);
}
.layout-preview--wide::after {
  inset-inline-start: 4px;
  inset-inline-end: 4px;
}
</style>
