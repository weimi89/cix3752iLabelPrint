<script setup>
const props = defineProps({
  modelValue: {
    type: Array,
    default: () => [],
  },
  label: {
    type: String,
    default: '訂單編號',
  },
  hint: {
    type: String,
    default: '可掃描連續輸入，或貼上多筆，以換行 / 逗號 / 空白分隔',
  },
})
const emit = defineEmits(['update:modelValue'])

const text = ref(props.modelValue.join('\n'))

const sync = () => {
  const list = text.value
    .split(/[\s,]+/)
    .map(s => s.trim())
    .filter(Boolean)
  emit('update:modelValue', list)
}

watch(() => props.modelValue, v => {
  const joined = v.join('\n')
  if (joined !== text.value) text.value = joined
})

const onKeyup = e => {
  if (e.key === 'Enter') sync()
}
</script>

<template>
  <VTextarea
    v-model="text"
    :label="label"
    :hint="hint"
    persistent-hint
    rows="6"
    auto-grow
    clearable
    @keyup="onKeyup"
    @blur="sync"
  />
</template>
