import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import vuetify from './plugins/vuetify'

import 'vuetify/styles'
// 對齊 cix3752iWeb：先載入整套 Materio @core template SCSS（v-field/v-card/v-list/v-table 細節覆寫）
import '@core-scss/template/index.scss'
import './styles/main.scss'

const app = createApp(App)

app.use(createPinia())
app.use(router)
app.use(vuetify)

app.mount('#app')
