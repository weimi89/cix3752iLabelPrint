import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vuetify from 'vite-plugin-vuetify'
import AutoImport from 'unplugin-auto-import/vite'
import Components from 'unplugin-vue-components/vite'
import { fileURLToPath, URL } from 'node:url'

const host = process.env.TAURI_DEV_HOST

export default defineConfig(async () => ({
  plugins: [
    vue(),
    vuetify({ autoImport: true }),
    AutoImport({
      imports: [
        'vue',
        'vue-router',
        '@vueuse/core',
        'pinia',
      ],
      dirs: [
        './src/@core/utils',
        './src/@core/composable',
        './src/composables',
      ],
      vueTemplate: true,
      dts: 'src/auto-imports.d.ts',
      eslintrc: { enabled: false },
    }),
    Components({
      dirs: ['src/components'],
      dts: 'src/components.d.ts',
    }),
  ],
  resolve: {
    alias: {
      // 對齊 cix3752iWeb 的 alias 一覽
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      '@core': fileURLToPath(new URL('./src/@core', import.meta.url)),
      '@core-scss': fileURLToPath(new URL('./src/styles/@core', import.meta.url)),
      '@layouts': fileURLToPath(new URL('./src/@layouts', import.meta.url)),
      '@styles': fileURLToPath(new URL('./src/styles', import.meta.url)),
      '@configured-variables': fileURLToPath(new URL('./src/styles/variables/_template.scss', import.meta.url)),
      '@images': fileURLToPath(new URL('./src/assets/images', import.meta.url)),
      '@themeConfig': fileURLToPath(new URL('./themeConfig.js', import.meta.url)),
      '@variables': fileURLToPath(new URL('./src/@variables', import.meta.url)),
    },
  },
  publicDir: 'public',
  clearScreen: false,
  server: {
    port: 11420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 11421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  // vite-plugin-vuetify 的 autoImport 是「按需」解析元件,初次掃描掃不到只在某頁/對話框才用到的元件;
  // 這些元件在 dev 期首次渲染時才被發現 → Vite 觸發整頁硬 reload 重新 optimize。該 reload 撞進
  // Tauri webview 初始化時機會把 SPA router 卡死(切不了頁)。在此預先宣告,啟動即 bundle、不再 reload。
  optimizeDeps: {
    include: [
      'vuetify/components/VImg',
      'vuetify/components/VSwitch',
      'vuetify/components/VProgressCircular',
      'vuetify/components/VTooltip',
      'vuetify/components/VDialog',
      'vuetify/components/VSlider',
    ],
  },
}))
