<script setup>
import { ref, onMounted } from 'vue'
import { getVersion } from '@tauri-apps/api/app'

defineProps({
  toggleVerticalOverlayNavActive: {
    type: Function,
    required: false,
    default: () => {},
  },
})

const appVersion = ref('')

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch {
    // 非 Tauri 環境(如純瀏覽器預覽)取不到版本,留空即可
  }
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
    <span
      v-if="appVersion"
      class="text-black font-weight-medium"
    >v{{ appVersion }}</span>
    <VSpacer />
    <NetworkStatusIndicator />
    <LocaleSwitcher />
  </div>
</template>
