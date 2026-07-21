import { createVuetify } from 'vuetify'
import { aliases } from 'vuetify/iconsets/mdi'
import { createVueI18nAdapter } from 'vuetify/locale/adapters/vue-i18n'
import { h } from 'vue'
import { useI18n } from 'vue-i18n'
import { Icon } from '@iconify/vue'

import { getI18n } from '@/plugins/i18n'
import defaults from './vuetify-defaults'

// 正規化 Materio 的 dash 寫法（tabler-xxx / mdi-xxx）成 iconify 的 prefix:name 格式
const normalizeIconName = name => {
  if (!name || typeof name !== 'string') return name
  if (name.includes(':')) return name
  if (name.startsWith('tabler-')) return `tabler:${name.slice(7)}`
  if (name.startsWith('mdi-')) return `mdi:${name.slice(4)}`
  if (name.startsWith('fa6-solid-')) return `fa6-solid:${name.slice(10)}`
  return name
}

const iconify = {
  component: props => h(Icon, { ...props, icon: normalizeIconName(props.icon) }),
}

const lightTheme = {
  dark: false,
  colors: {
    primary: '#7367F0',
    'on-primary': '#fff',
    'primary-darken-1': '#675DD8',
    secondary: '#808390',
    'on-secondary': '#fff',
    'secondary-darken-1': '#737682',
    success: '#28C76F',
    'on-success': '#fff',
    'success-darken-1': '#24B364',
    info: '#00BAD1',
    'on-info': '#fff',
    'info-darken-1': '#00A7BC',
    warning: '#FF9F43',
    'on-warning': '#fff',
    'warning-darken-1': '#E68F3C',
    error: '#FF4C51',
    'on-error': '#fff',
    'error-darken-1': '#E64449',
    background: '#F8F7FA',
    'on-background': '#2F2B3D',
    surface: '#fff',
    'on-surface': '#2F2B3D',
    'grey-50': '#FAFAFA',
    'grey-100': '#F5F5F5',
    'grey-200': '#EEEEEE',
    'grey-300': '#E0E0E0',
    'grey-400': '#BDBDBD',
    'grey-500': '#9E9E9E',
    'grey-600': '#757575',
    'grey-700': '#616161',
    'grey-800': '#424242',
    'grey-900': '#212121',
  },
  variables: {
    'border-color': '#2F2B3D',
    'border-opacity': 0.12,
    'high-emphasis-opacity': 0.9,
    'medium-emphasis-opacity': 0.7,
    'hover-opacity': 0.06,
    'focus-opacity': 0.1,
    'selected-opacity': 0.08,
    'activated-opacity': 0.16,
    'pressed-opacity': 0.14,
    'shadow-key-umbra-color': '#2F2B3D',
    'shadow-xs-opacity': 0.10,
    'shadow-sm-opacity': 0.12,
    'shadow-md-opacity': 0.14,
    'shadow-lg-opacity': 0.16,
    'shadow-xl-opacity': 0.18,
    'track-bg': '#F1F0F2',
  },
}

const darkTheme = {
  dark: true,
  colors: {
    ...lightTheme.colors,
    background: '#25293C',
    'on-background': '#E1DEF5',
    surface: '#2F3349',
    'on-surface': '#E1DEF5',
  },
  variables: {
    ...lightTheme.variables,
    'border-color': '#E1DEF5',
    'shadow-key-umbra-color': '#131120',
    'track-bg': '#3A3F57',
  },
}

export default createVuetify({
  // 讓 Vuetify 內建元件標籤($vuetify.input.clear / $vuetify.close 等)走 vue-i18n,
  // 訊息已在 plugins/i18n 併入各語系的 $vuetify(內建 zhHant / vi)。
  // 未接時 Vuetify 用自己那套(無 zh-Hant 訊息)→ 每個 clearable input / dialog 都噴
  // 「Translation key not found」警告,dev 模式下每筆警告序列化整棵元件樹(含 devtools-api 巨物)
  // → GB 級 log 灌爆、切頁與操作嚴重卡頓。
  locale: {
    adapter: createVueI18nAdapter({ i18n: getI18n(), useI18n }),
  },
  theme: {
    defaultTheme: 'light',
    themes: { light: lightTheme, dark: darkTheme },
  },
  icons: {
    defaultSet: 'iconify',
    aliases,
    sets: { iconify },
  },
  defaults,
})
