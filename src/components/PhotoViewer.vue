<script setup>
/**
 * 照片放大檢視：Viewer.js 全螢幕檢視器（滾輪／手勢縮放、拖曳、1:1、旋轉、翻轉），
 * 讀碼異常存證照與包裹查詢紀錄的快照／面單都開它。
 * 圖先預載：載不到（404、檔案已清理）就不開檢視器、發 `error` 讓頁面用自己的文案提示。
 */
import Viewer from 'viewerjs'
import 'viewerjs/dist/viewer.css'

const props = defineProps({
  modelValue: Boolean,
  url: { type: String, default: '' },
  title: { type: String, default: '' },
})
const emit = defineEmits(['update:modelValue', 'error'])

const holder = ref(null)
let viewer = null
let opening = 0

const destroy = () => {
  if (viewer) { viewer.destroy(); viewer = null }
  if (holder.value) holder.value.innerHTML = ''
}

const open = url => {
  const seq = ++opening
  destroy()
  const el = document.createElement('img')
  el.alt = ''
  el.onload = () => {
    if (seq !== opening || !holder.value) return
    holder.value.appendChild(el)
    viewer = new Viewer(el, {
      navbar: false,
      title: () => props.title,
      // 單張：不要上一張／下一張與播放
      toolbar: { zoomIn: 1, zoomOut: 1, oneToOne: 1, reset: 1, rotateLeft: 1, rotateRight: 1, flipHorizontal: 1, flipVertical: 1 },
      zIndex: 3000,
      transition: false,
      hidden: () => emit('update:modelValue', false),
    })
    viewer.show()
  }
  el.onerror = () => {
    if (seq !== opening) return
    emit('error')
    emit('update:modelValue', false)
  }
  el.src = url
}

watch(() => [props.modelValue, props.url], ([isOpen, url]) => {
  if (isOpen && url) open(url)
  else { opening++; destroy() }
}, { immediate: true })

onBeforeUnmount(() => { opening++; destroy() })
</script>

<template>
  <!-- 檢視器自己會開全螢幕遮罩；這個容器只用來掛那張隱藏的 img -->
  <div ref="holder" class="photo-viewer__holder" />
</template>

<style scoped>
.photo-viewer__holder { display: none; }
</style>
