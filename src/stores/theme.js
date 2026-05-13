import { defineStore } from 'pinia'

const STORAGE_KEY = 'cix3752iLabelPrint.themeConfig'

const DEFAULT_CONFIG = {
  primary: '#7367F0',
  mode: 'system',          // light / dark / system
  style: 'default',        // default / bordered
  semiDark: false,
  layout: 'vertical',      // vertical / collapsed / horizontal
  contentWidth: 'wide',    // compact / wide
}

const load = () => {
  try {
    return { ...DEFAULT_CONFIG, ...(JSON.parse(localStorage.getItem(STORAGE_KEY) || 'null') || {}) }
  } catch {
    return { ...DEFAULT_CONFIG }
  }
}

export const useThemeStore = defineStore('theme', {
  state: () => ({
    config: load(),
    customizerOpen: false,
  }),
  actions: {
    persist() {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.config))
    },
    set(patch) {
      this.config = { ...this.config, ...patch }
      this.persist()
    },
    reset() {
      this.config = { ...DEFAULT_CONFIG }
      this.persist()
    },
  },
})

// 預設主色 preset
export const PRIMARY_PRESETS = [
  { value: '#7367F0', label: '紫' },
  { value: '#0D9394', label: '青' },
  { value: '#FFB400', label: '橘黃' },
  { value: '#FF4C51', label: '紅' },
  { value: '#16B1FF', label: '藍' },
]

// 推算 darken-1：把色相略暗 8%（cix3752iWeb 用 #675DD8 對 #7367F0）
export const darken = hex => {
  const n = parseInt(hex.replace('#', ''), 16)
  let r = (n >> 16) & 0xff, g = (n >> 8) & 0xff, b = n & 0xff
  const f = 0.91
  r = Math.round(r * f); g = Math.round(g * f); b = Math.round(b * f)
  return `#${[r, g, b].map(v => v.toString(16).padStart(2, '0')).join('')}`
}
