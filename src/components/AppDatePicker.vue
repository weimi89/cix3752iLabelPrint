<script setup>
import { provide, ref, computed, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocale } from 'vuetify'
import { VuetifyDateAdapter } from 'vuetify/date/adapters/vuetify'

// 共用日期選擇器:VTextField(唯讀)+ 彈出 VDatePicker。
// 封裝 PrintStatsPage 驗證過的兩個坑修法:
//  1. locale:provide 正確的 DateOptions(adapter + i18n→Intl locale mapping)+ 同步 vuetify locale,
//     月份/星期才會跟著介面語言顯示(否則顯示英文)。
//  2. 全綠:VBtn defaults color=primary 會讓日曆每格文字變主色(綠),用非 scoped 全域 CSS 壓回。
// v-model 為 'yyyy-mm-dd' 字串。
const props = defineProps({
  modelValue: { type: String, default: '' },
  label: { type: String, default: '' },
  density: { type: String, default: 'compact' },
  disabled: { type: Boolean, default: false },
  // 輸入框寬度(預設滿版);日曆 popup 寬度由下方 content-class 壓掉 min-width,不受此影響
  width: { type: String, default: '100%' },
  // 可選的最晚日期('yyyy-mm-dd');空=不限制。之後的日子在日曆上會被停用
  max: { type: String, default: '' },
})
const emit = defineEmits(['update:modelValue'])

const { locale } = useI18n()
const I18N_TO_DATE_LOCALE = { 'zh-Hant': 'zh-TW', 'vi-VN': 'vi-VN', en: 'en-US' }
const dateLocale = computed(() => I18N_TO_DATE_LOCALE[locale.value] || 'en-US')

// VDatePicker 內部 useDate() → inject DateOptions;provide 正確 adapter + locale mapping
const DateOptionsSymbol = Symbol.for('vuetify:date-options')
provide(DateOptionsSymbol, {
  adapter: VuetifyDateAdapter,
  locale: { 'zh-Hant': 'zh-TW', 'vi-VN': 'vi-VN', en: 'en-US' },
  formats: {},
})
// 強制把 vuetify locale 同步到 i18n locale,VDatePicker createInstance 才用對 Intl locale
const vuetifyLocale = useLocale()
watchEffect(() => {
  if (vuetifyLocale?.current && vuetifyLocale.current.value !== locale.value) {
    vuetifyLocale.current.value = locale.value
  }
})

const menu = ref(false)

// 字串 yyyy-mm-dd ↔ Date 互轉(VDatePicker 內部用 Date 物件)
const strToDate = s => {
  if (!s) return null
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}
const dateToStr = d => {
  if (!d) return ''
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${dd}`
}
const dateObj = computed({
  get: () => strToDate(props.modelValue),
  set: v => {
    emit('update:modelValue', dateToStr(v))
    menu.value = false
  },
})
</script>

<template>
  <VMenu v-model="menu" :close-on-content-click="false" :disabled="disabled" location="bottom start" content-class="app-date-picker-menu">
    <template #activator="{ props: act }">
      <VTextField
        v-bind="act"
        :model-value="modelValue"
        :label="label"
        :density="density"
        :disabled="disabled"
        :style="{ inlineSize: width }"
        hide-details
        readonly
        prepend-inner-icon="tabler-calendar"
      />
    </template>
    <VDatePicker
      v-model="dateObj"
      :locale="dateLocale"
      :max="strToDate(max) || undefined"
      show-adjacent-months
      hide-header
    />
  </VMenu>
</template>

<!--
  VDatePicker 被 VMenu Teleport 到 body,scoped CSS 無法套到 portal 外的 DOM,
  所以這段必須是非 scoped 全域。VBtn defaults color=primary 會讓日期文字繼承主色(綠),
  這裡強制把非選中按鈕文字色改回 on-surface;並修正預設寬度裁掉最右 column 的問題。
-->
<style lang="scss">
.v-date-picker-month .v-btn.text-primary:not(.v-btn--active),
.v-date-picker-month .v-btn:not(.v-btn--active),
.v-date-picker-months__content .v-btn.text-primary:not(.v-btn--active),
.v-date-picker-months__content .v-btn:not(.v-btn--active),
.v-date-picker-years__content .v-btn.text-primary:not(.v-btn--active),
.v-date-picker-years__content .v-btn:not(.v-btn--active),
.v-date-picker-header .v-btn.text-primary,
.v-date-picker-header .v-btn,
.v-date-picker-controls .v-btn.text-primary,
.v-date-picker-controls .v-btn {
  color: rgba(var(--v-theme-on-surface), 0.87) !important;
}
.v-date-picker-month__day--adjacent .v-btn.text-primary,
.v-date-picker-month__day--adjacent .v-btn {
  color: rgba(var(--v-theme-on-surface), 0.38) !important;
}
.v-date-picker > .v-picker__body,
.v-date-picker {
  width: auto !important;
  min-width: 328px;
}

/* 輸入框可滿版,但 VMenu 預設會把 popup min-width 對齊 activator 寬度,
   這裡壓掉,讓日曆維持自身 328px,不被全寬輸入框撐大 */
.app-date-picker-menu {
  min-width: auto !important;
}
</style>
