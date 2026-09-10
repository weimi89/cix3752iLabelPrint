import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vuetify from 'vite-plugin-vuetify'
import AutoImport from 'unplugin-auto-import/vite'
import Components from 'unplugin-vue-components/vite'
import { fileURLToPath, URL } from 'node:url'

const host = process.env.TAURI_DEV_HOST

export default defineConfig(async () => ({
  plugins: [
    {
      // 正式版的旗標由後端在 index.html 注入(見 server/assets.rs);dev 由 Vite 提供
      // 頁面,後端插不到手,這裡補上同一個旗標,讓開發與正式跑在同一條路徑上。
      name: 'cix-web-runtime-flag',
      apply: 'serve',
      transformIndexHtml: html =>
        html.replace('<head>', '<head><script>window.__CIX_WEB__=true</script>'),
    },
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
    // 開發時用瀏覽器直接開 http://localhost:11420 就是網頁版(連真後端、有熱更新)。
    // 沒有這段的話,每改一次網頁版都得先 yarn build 才看得到結果。
    // Tauri 桌面視窗也載這個位址,但它走 IPC 不打這些路徑,互不影響。
    proxy: Object.fromEntries(
      ['/rpc', '/auth', '/events', '/api', '/images', '/captures', '/camera', '/healthz']
        .map(p => [p, { target: 'http://127.0.0.1:18080', changeOrigin: false }]),
    ),
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    // macOS 非 Windows 走 safari14.1:Vite 8(Rolldown)無法把解構降到 safari13,14.1 是最低可過。
    // ⚠️ 這抬高 macOS 執行下限 → tauri.conf.json 已設 macOS.minimumSystemVersion=11.3(Big Sur 11.3
    // 首個內建 WebKit 14.1)把限制擋在安裝端,避免舊 macOS 更新後白屏。兩者需一起調整。
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari14.1',
    // ⚠️ Vite 8 起 esbuild 不再自帶(改可選 peer),minify:'esbuild' 與 build.target 降級都依賴
    // package.json 的 esbuild devDependency。勿移除該 devDep,CI 也勿改用 --omit=dev / npm ci --production。
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // 打包切分:預設會把 Vuetify 依元件拆成幾十個小檔,在低速高延遲的現場網路下,
    // 每個檔都要一次來回,請求數本身就成了瓶頸。把框架類合併成少數幾包,
    // 但**圖表庫維持獨立** —— 它只有統計頁用得到,併進共用包等於每個人都要多載半 MB。
    rolldownOptions: {
      output: {
        advancedChunks: {
          groups: [
            { name: 'vendor-charts', test: /node_modules[\\/](echarts|vue-echarts|zrender)/ },
            { name: 'vendor-vuetify', test: /node_modules[\\/]vuetify/ },
            { name: 'vendor', test: /node_modules/ },
          ],
        },
      },
    },
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
