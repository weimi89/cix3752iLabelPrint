import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import vuetify from './plugins/vuetify'
import toastify from './plugins/toastify'
import { createLayouts } from '@layouts'
import { themeConfig } from '@themeConfig'

import 'vuetify/styles'
// 對齊 Materio:整套 Materio @core template SCSS(v-field/v-card/v-list/v-table 細節覆寫)
import '@core-scss/template/index.scss'
import './styles/main.scss'

const app = createApp(App)

app.use(createPinia())
app.use(router)
app.use(vuetify)
toastify(app)
// @layouts plugin (初始化 layoutConfig + cookie 同步)
app.use(createLayouts(themeConfig))

app.mount('#app')
