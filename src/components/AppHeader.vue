<script setup>
defineProps({
  title: { type: String, required: true },
  subtitle: { type: String, default: '' },
  // 手機用的短版說明:標題旁只有一行多的寬度,長說明會疊成三四行把整張頁首撐高。
  // 沒給就沿用 subtitle
  subtitleShort: { type: String, default: '' },
  icon: { type: String, default: '' },
})
</script>

<template>
  <div class="app-header-card">
    <VIcon
      v-if="icon"
      :icon="icon"
      size="32"
      class="app-header-card__icon"
    />
    <div class="app-header-card__text">
      <span class="app-header-card__title">{{ title }}</span>
      <span v-if="subtitle" class="app-header-card__subtitle d-none d-sm-inline">{{ subtitle }}</span>
      <span v-if="subtitleShort || subtitle" class="app-header-card__subtitle d-sm-none">{{ subtitleShort || subtitle }}</span>
    </div>
    <VSpacer />
    <div class="app-header-card__actions">
      <slot name="actions" />
    </div>
  </div>
</template>
