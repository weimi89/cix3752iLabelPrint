<script setup>
defineProps({
  toggleVerticalOverlayNavActive: {
    type: Function,
    required: false,
    default: () => {},
  },
})
</script>

<template>
  <div class="d-flex h-100 align-center">
    <VBtn
      icon
      variant="text"
      color="default"
      class="ms-n3 d-lg-none"
      @click="toggleVerticalOverlayNavActive(true)"
    >
      <VIcon size="26" icon="tabler-menu-2" />
    </VBtn>
    <VSpacer />
    <NetworkStatusIndicator />
    <LocaleSwitcher />
  </div>
</template>
