import VueToastify from 'vue3-toastify'
import 'vue3-toastify/dist/index.css'

export default function (app) {
  app.use(VueToastify, {
    autoClose: 3000,
    position: 'bottom-right',
    newestOnTop: true,
  })
}
