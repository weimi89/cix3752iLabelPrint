<script setup>
// 看板(kiosk)子視窗外殼:全螢幕無邊框時沒有系統視窗按鈕,
// 故提供浮動關閉鈕 + Esc 快捷鍵讓現場可關閉。
import { onBeforeUnmount, onMounted } from 'vue'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const win = (() => {
  try { return getCurrentWebviewWindow() } catch { return null }
})()

const close = async () => {
  try { await win?.close() } catch (e) { console.warn('關閉看板失敗', e) }
}

const onKey = e => {
  if (e.key === 'Escape') close()
}

onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <div class="kiosk-root">
    <button class="kiosk-exit" :title="`${t('page.display.exit')} (Esc)`" @click="close">
      <VIcon icon="tabler-x" size="18" />
    </button>
    <slot />
  </div>
</template>

<style scoped>
.kiosk-root {
  height: 100vh;
  overflow: auto;
  background: rgb(var(--v-theme-background));
  padding: 12px;
}

.kiosk-exit {
  position: fixed;
  top: 8px;
  right: 8px;
  z-index: 9999;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgb(0 0 0 / 0.4);
  color: #fff;
  opacity: 0.18;
  cursor: pointer;
  border: none;
  transition: opacity 0.15s, transform 0.15s;
}

.kiosk-exit:hover {
  opacity: 1;
  transform: scale(1.08);
}
</style>
