<script setup>
defineProps({
  title: { type: String, required: true },
  subtitle: { type: String, default: '' },
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
    <div class="d-flex flex-column">
      <span class="app-header-card__title">{{ title }}</span>
      <span v-if="subtitle" class="app-header-card__subtitle">{{ subtitle }}</span>
    </div>
    <VSpacer />
    <slot name="actions" />
  </div>
</template>
