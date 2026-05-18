import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { AVAILABLE_LOCALES, persistLocale } from '@/plugins/i18n'

export function useLocale() {
  const { locale } = useI18n({ useScope: 'global' })

  const currentLocale = computed({
    get: () => locale.value,
    set: code => setLocale(code),
  })

  function setLocale(code) {
    if (!AVAILABLE_LOCALES.some(l => l.code === code)) return
    locale.value = code
    persistLocale(code)
    document.documentElement.setAttribute('lang', code)
  }

  return {
    currentLocale,
    availableLocales: AVAILABLE_LOCALES,
    setLocale,
  }
}
