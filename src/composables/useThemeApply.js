import { watch } from 'vue'
import { useTheme } from 'vuetify'
import { useThemeStore, darken } from '@/stores/theme'

/**
 * 把 pinia themeStore 的設定同步到 Vuetify 主題與 document class
 * 在 App.vue 內 setup 一次即可
 */
export const useThemeApply = () => {
  const theme = useTheme()
  const themeStore = useThemeStore()

  const resolveMode = () => {
    const m = themeStore.config.mode
    if (m === 'system') {
      return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    }
    return m
  }

  const apply = () => {
    const mode = resolveMode()
    theme.global.name.value = mode

    // 動態覆蓋 primary 色（兩個 theme 都改）
    const primary = themeStore.config.primary
    const primaryDarken = darken(primary)
    for (const name of ['light', 'dark']) {
      const t = theme.themes.value[name]
      if (!t) continue
      t.colors.primary = primary
      t.colors['primary-darken-1'] = primaryDarken
    }

    // 樣式：有邊框 → body class
    document.documentElement.classList.toggle('style-bordered', themeStore.config.style === 'bordered')
    document.documentElement.classList.toggle('style-default', themeStore.config.style !== 'bordered')
    document.documentElement.classList.toggle('content-wide', themeStore.config.contentWidth === 'wide')
    document.documentElement.classList.toggle('content-compact', themeStore.config.contentWidth === 'compact')
    document.documentElement.classList.toggle('semi-dark', !!themeStore.config.semiDark)
  }

  // 監聽 store + system 主題變化
  watch(() => ({ ...themeStore.config }), apply, { deep: true, immediate: true })

  window.matchMedia?.('(prefers-color-scheme: dark)')
    .addEventListener?.('change', () => {
      if (themeStore.config.mode === 'system') apply()
    })

  return { apply }
}
