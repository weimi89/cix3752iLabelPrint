<script setup>
// 多組單號輸入框:單行輸入,貼上多行(Excel 直欄)時把換行轉成空白,不讓瀏覽器把換行吃掉黏成一串。
// 單號以逗號 / 空白 / 換行分隔,後端拆開後做精確比對(不做模糊)。
const model = defineModel({ type: String, default: '' })
const emit = defineEmits(['search'])

const onPaste = e => {
  const text = e.clipboardData?.getData('text') ?? ''
  if (!/[\r\n\t]/.test(text)) return // 單行貼上交給瀏覽器預設處理
  e.preventDefault()
  const norm = text.replace(/[\r\n\t]+/g, ' ').replace(/\s+/g, ' ').trim()
  const el = e.target
  const cur = model.value || ''
  const start = el.selectionStart ?? cur.length
  const end = el.selectionEnd ?? cur.length
  const before = cur.slice(0, start)
  const after = cur.slice(end)
  model.value = `${before}${before && !/\s$/.test(before) ? ' ' : ''}${norm}${after && !/^\s/.test(after) ? ' ' : ''}${after}`
}
</script>

<template>
  <VTextField
    v-model="model"
    :placeholder="$t('common.multiNoPlaceholder')"
    density="compact"
    hide-details
    variant="outlined"
    clearable
    @paste="onPaste"
    @keyup.enter="emit('search')"
  />
</template>
