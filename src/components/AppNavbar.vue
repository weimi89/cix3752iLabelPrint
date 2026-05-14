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
    <!-- 漢堡:< lg(992px) 顯示,點擊打開 overlay sidebar -->
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
  </div>
</template>
