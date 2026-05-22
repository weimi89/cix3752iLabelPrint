import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import vuetify from './plugins/vuetify'
import toastify from './plugins/toastify'
import i18n from './plugins/i18n'
import { createLayouts } from '@layouts'
import { themeConfig } from '@themeConfig'

import 'vuetify/styles'
// OverlayScrollbars 套件自己的 viewport scroll CSS (sidebar nav scrollbar 必需)
// 之前在 @layouts/styles/index.scss 用 @use 引入,但 Vite/Sass 對 .css 檔的 @use 沒實際載入規則 → 改 JS import
import 'overlayscrollbars/overlayscrollbars.css'
// 對齊 Materio:整套 Materio @core template SCSS(v-field/v-card/v-list/v-table 細節覆寫)
import '@core-scss/template/index.scss'
import './styles/main.scss'

const app = createApp(App)

app.use(createPinia())
app.use(router)
i18n(app)
app.use(vuetify)
toastify(app)
// @layouts plugin (初始化 layoutConfig + cookie 同步)
app.use(createLayouts(themeConfig))

app.mount('#app')
