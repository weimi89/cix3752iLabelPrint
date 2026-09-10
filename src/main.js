import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import vuetify from './plugins/vuetify'
import toastify from './plugins/toastify'
import i18n from './plugins/i18n'
import { createLayouts } from '@layouts'
import { themeConfig } from '@themeConfig'
import { setUnauthorizedHandler } from '@/api/rpc'
import { useWebAuth } from '@/composables/useWebAuth'

// Vuetify 4 layer 順序 + 選擇性 CSS reset,必須排在 vuetify/styles 之前
import './styles/vuetify-layers.css'
import 'vuetify/styles'
// OverlayScrollbars 套件自己的 viewport scroll CSS (sidebar nav scrollbar 必需)
// 之前在 @layouts/styles/index.scss 用 @use 引入,但 Vite/Sass 對 .css 檔的 @use 沒實際載入規則 → 改 JS import
import 'overlayscrollbars/overlayscrollbars.css'
// 對齊 Materio:整套 Materio @core template SCSS(v-field/v-card/v-list/v-table 細節覆寫)
import '@core-scss/template/index.scss'
import './styles/main.scss'

// 資料請求收到 401(session 逾期、或後端重啟清掉了 session)時,把人帶回登入頁。
// 少了這段,逾期後畫面會停在原地一直跳「尚未登入」的錯誤,使用者不知道要去哪重新登入。
setUnauthorizedHandler(() => {
  const { markUnauthenticated } = useWebAuth()
  markUnauthenticated()
  if (router.currentRoute.value.name !== 'login') {
    const from = router.currentRoute.value.fullPath
    router.replace({ name: 'login', query: from === '/' ? {} : { redirect: from } })
  }
})

const app = createApp(App)

app.use(createPinia())
app.use(router)
i18n(app)
app.use(vuetify)
toastify(app)
// @layouts plugin (初始化 layoutConfig + cookie 同步)
app.use(createLayouts(themeConfig))

app.mount('#app')
