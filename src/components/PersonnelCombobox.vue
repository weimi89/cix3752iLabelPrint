<script setup>
// 人員輸入框:可手打新名字,也可從歷史名單下拉選取;下拉項目附刪除鈕。
// 操作人員 / 貼單人員 / 貼標人員三處共用,行為一致。
// density / variant / clearable / hide-details / placeholder 等屬性由父層透傳給 VCombobox。
defineProps({
  modelValue: { type: [String, null], default: null },
  items: { type: Array, default: () => [] },
})
const emit = defineEmits(['update:modelValue', 'remove', 'remember'])

// 記住歷史的時機:只在「失焦」時把輸入框最終值寫入,而非每次 update:model-value。
// 原因:中文輸入法組字過程中 VCombobox 會逐字觸發注音/拼音殘片(如 "DW"、"D e"、"時"),
// 綁在 update 會把這些殘片全記進歷史。改用 blur 只取最終值;composing 旗標再保險一層,
// 確保組字尚未結束(如組字中直接失焦)時不誤記。
let composing = false
const onBlur = e => {
  if (composing) return
  const name = (e?.target?.value ?? '').trim()
  if (name) emit('remember', name)
}
</script>

<template>
  <VCombobox
    :model-value="modelValue"
    :items="items"
    @update:model-value="emit('update:modelValue', $event)"
    @compositionstart="composing = true"
    @compositionend="composing = false"
    @blur="onBlur"
  >
    <template #item="{ item, props: itemProps }">
      <VListItem v-bind="itemProps" :title="item.raw">
        <template #append>
          <VBtn
            icon="tabler-x"
            size="x-small"
            variant="text"
            @click.stop="emit('remove', item.raw)"
          />
        </template>
      </VListItem>
    </template>
  </VCombobox>
</template>
