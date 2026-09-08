<script setup>
import { sortChannelList } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import DisplayLauncher from '@/components/DisplayLauncher.vue'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 開在別的螢幕的看板子視窗:label 以 display- 開頭,此時整頁只留看板本身
let isDisplayWindow = false
if (isTauriRuntime) {
  try {
    isDisplayWindow = getCurrentWebviewWindow().label.startsWith('display-')
  } catch { /* 取不到 label 就當作主視窗 */ }
}

// 綠燈持續時間:亮完自動淡回灰。6 秒 —— 現場包裹間隔約 2~5 秒,
// 太短來不及看,太長會分不出「剛剛那件」和「這件」。
const HIGHLIGHT_MS = 6000
// 通道啟用 / 暫停是慢速變動的設定,不必跟著每件包裹重抓
const CHANNEL_POLL_MS = 10000

const channels = ref([])
const activePos = ref(null)
// 中央顯示:等待第一件時是 idle
const board = ref({ no: '', provider: '', status: 'idle', message: '' })

const leftSlots = computed(() => channels.value.filter(c => c.position?.[0] === 'L'))
const rightSlots = computed(() => channels.value.filter(c => c.position?.[0] === 'R'))

const posLabel = pos => t(`page.board.pos.${pos[0] === 'L' ? 'left' : 'right'}`, { n: pos.slice(1) })

const slotClass = pos => {
  if (pos === activePos.value) return 'slot--active'
  const ch = channels.value.find(c => c.position === pos)
  return ch && !ch.enabled ? 'slot--paused' : ''
}

// 單號拆成「前段 + 後四碼」,後四碼用更大的字級 —— 現場核對就是看後幾碼
const noHead = computed(() => {
  const s = String(board.value.no || '')
  return s.slice(0, Math.max(0, s.length - 4))
})
const noTail = computed(() => {
  const s = String(board.value.no || '')
  return s.slice(Math.max(0, s.length - 4)) || '—'
})

// 字級隨單號長度自適應(見樣式的 --len):固定字級遇到 15 碼的順豐單號會撐破中欄、蓋到燈號
const noLen = computed(() => Math.max(6, String(board.value.no || '').length))

const providerText = computed(() => {
  if (board.value.status === 'idle') return t('page.board.waiting')
  if (board.value.status === 'unassigned' && !board.value.provider) return t('page.board.unassigned')
  return board.value.provider || ''
})
const noteText = computed(() => {
  if (board.value.status === 'error') return board.value.message || ''
  if (board.value.status === 'unassigned') return t('page.board.unassigned')
  return ''
})

let fadeTimer = null
const applyEvent = payload => {
  if (!payload) return
  board.value = {
    no: payload.no || '',
    provider: payload.provider || '',
    status: payload.status || 'ok',
    message: payload.message || '',
  }
  activePos.value = payload.position || null
  if (fadeTimer) clearTimeout(fadeTimer)
  if (activePos.value) {
    fadeTimer = setTimeout(() => { activePos.value = null }, HIGHLIGHT_MS)
  }
}

const loadChannels = async () => {
  try {
    channels.value = await sortChannelList()
  } catch { /* 讀不到就沿用上一份快照,下一輪再試,不讓看板整片空掉 */ }
}

let pollTimer = null
let unlistenBoard = null
let unlistenChannel = null
let disposed = false

onMounted(async () => {
  await loadChannels()
  pollTimer = setInterval(loadChannels, CHANNEL_POLL_MS)
  if (!isTauriRuntime) return
  // 先 await 再掛監聽,期間若已切頁就要立刻解除,否則監聽會永久殘留
  const unBoard = await listen('sort-board', evt => applyEvent(evt.payload))
  if (disposed) unBoard(); else unlistenBoard = unBoard
  const unChannel = await listen('sort-channel-updated', () => loadChannels())
  if (disposed) unChannel(); else unlistenChannel = unChannel
})

onUnmounted(() => {
  disposed = true
  if (pollTimer) { clearInterval(pollTimer); pollTimer = null }
  if (fadeTimer) { clearTimeout(fadeTimer); fadeTimer = null }
  if (unlistenBoard) { unlistenBoard(); unlistenBoard = null }
  if (unlistenChannel) { unlistenChannel(); unlistenChannel = null }
})
</script>

<template>
  <div :class="{ 'board-page--kiosk': isDisplayWindow }">
    <!-- 看板子視窗(display-*)是 kiosk 模式、沒有側欄,標題列在那裡只會佔掉看板高度 -->
    <AppHeader
      v-if="!isDisplayWindow"
      :title="$t('page.board.title')"
      :subtitle="$t('page.board.subtitle')"
      icon="tabler-device-tv"
    >
      <template #actions>
        <DisplayLauncher
          route="/sort-board"
          window-label="display-sort-board"
          :title="$t('page.board.title')"
        />
      </template>
    </AppHeader>

    <div class="sort-board" :class="{ 'sort-board--kiosk': isDisplayWindow }">
    <div class="board-col">
      <div
        v-for="c in leftSlots"
        :key="c.position"
        class="slot"
        :class="slotClass(c.position)"
      >
        <span class="slot__lamp" />
        <span class="slot__name">{{ posLabel(c.position) }}</span>
      </div>
    </div>

    <div class="board-center" :class="`board-center--${board.status}`">
      <div class="board-no" :style="{ '--len': noLen }">
        <span class="board-no__head">{{ noHead }}</span>
        <span class="board-no__tail">{{ noTail }}</span>
      </div>
      <div class="board-sub">
        <div class="board-provider">{{ providerText }}</div>
        <div class="board-note">{{ noteText }}</div>
      </div>
    </div>

    <div class="board-col board-col--right">
      <div
        v-for="c in rightSlots"
        :key="c.position"
        class="slot"
        :class="slotClass(c.position)"
      >
        <span class="slot__lamp" />
        <span class="slot__name">{{ posLabel(c.position) }}</span>
      </div>
    </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
/* 子視窗:外層要先撐滿外殼,裡面的看板才有 100% 可依循
   (外殼 KioskShell 是 100vh 並自帶內距,這樣接手就不必去硬算那段內距) */
.board-page--kiosk { block-size: 100%; }

/* 顏色一律取 App 主題(Vuetify)變數,不另外調一套 —— 主題改色時看板要跟著走。
   字級用 vw 隨視窗縮放,現場隔幾公尺也看得清楚。 */
.sort-board {
  display: grid;

  /* 左右欄只要放得下燈與位置名就好,剩下的寬度全給中間的單號 ——
     固定比例會讓單號兩側留一大片空白,字放不大 */
  grid-template-columns: auto 1fr auto;
  gap: 1.5vw;
  min-block-size: calc(100vh - 12rem);
  padding: 1.5vh 1vw;
  border-radius: 12px;

  /* 子視窗是整片看板:吃滿外殼(KioskShell 已是 100vh 並自帶內距),
     這裡用 100% 而非 100vh —— 用 100vh 會多出外殼內距的高度、擠出一條捲軸 */
  &--kiosk {
    block-size: 100%;
    min-block-size: 0;
    border-radius: 0;
  }
  /* 看板整片佔滿視窗,用頁面底色(background)而非卡片白(surface) */
  background: rgb(var(--v-theme-background));
  color: rgb(var(--v-theme-on-background));
}

.board-col {
  display: flex;
  flex-direction: column;
  justify-content: space-evenly;

  /* 燈與燈之間留出明顯間距:現場是隔幾公尺在看,擠在一起會分不出亮的是哪一格 */
  gap: 3vh;

  &--right .slot { flex-direction: row-reverse; }
}

.slot {
  display: flex;
  align-items: center;
  gap: 1vw;

  &__lamp {
    flex: none;
    inline-size: 6vw;
    block-size: 6vw;
    min-inline-size: 44px;
    min-block-size: 44px;
    /* 底色是淡灰,灰燈太淺會糊在背景裡,壓深到看得出「這格還沒輪到」 */
    border: .3vw solid rgba(var(--v-theme-on-background), .3);
    border-radius: 50%;
    background: rgba(var(--v-theme-on-background), .2);
    transition: background .25s ease, box-shadow .25s ease, border-color .25s ease;
  }

  &__name {
    color: rgba(var(--v-theme-on-background), .38);
    font-size: clamp(16px, 2.4vw, 46px);
    font-weight: 700;
    transition: color .25s ease;
  }

  &--active &__lamp {
    border-color: rgba(var(--v-theme-success), .45);
    background: rgb(var(--v-theme-success));
    box-shadow: 0 0 2.2vw rgba(var(--v-theme-success), .5);
  }
  &--active &__name { color: rgb(var(--v-theme-success)); }

  &--paused &__lamp {
    border-color: rgba(var(--v-theme-warning), .45);
    background: rgb(var(--v-theme-warning));
    box-shadow: 0 0 1.4vw rgba(var(--v-theme-warning), .45);
  }
  &--paused &__name { color: rgb(var(--v-theme-warning)); }
}

/* 單號要落在整個畫面的正中央(上下、左右都置中)。
   上下各留一條等高的彈性列,單號夾在中間 —— 物流名放到下面那條裡,
   才不會把單號往上推。 */
.board-center {
  display: grid;
  grid-template-rows: 1fr auto 1fr;
  justify-items: center;
  min-inline-size: 0;
}

.board-no { grid-row: 2; }

.board-sub {
  display: flex;
  flex-direction: column;
  gap: 1.2vh;
  grid-row: 3;
  align-self: start;
  padding-block-start: 2.5vh;
}

.board-no {
  display: flex;
  align-items: baseline;
  justify-content: center;
  max-inline-size: 100%;
  font-variant-numeric: tabular-nums;
  font-weight: 800;
  line-height: 1;
  white-space: nowrap;

  /* 除以長度:短單號放到最大,長單號自動縮到塞得下,不會蓋到兩側燈號 */
  &__head {
    font-size: min(9.4vw, calc(104vw / (var(--len) + 2)));
    opacity: .78;
  }

  /* 後四碼再加大:現場核對只看這幾碼 */
  &__tail {
    margin-inline-start: .06em;
    font-size: min(13.8vw, calc(153vw / (var(--len) + 2)));
  }
}

.board-provider {
  color: rgba(var(--v-theme-on-background), .68);
  font-size: clamp(18px, 3.2vw, 64px);
  font-weight: 700;
  text-align: center;
}

.board-note {
  color: rgba(var(--v-theme-on-background), .5);
  font-size: clamp(13px, 1.6vw, 30px);
  text-align: center;
}

/* 三種狀態的字色:正常白、查件異常紅、沒有指派通道黃 */
.board-center--ok .board-no { color: rgb(var(--v-theme-on-background)); }

.board-center--error {
  .board-no, .board-provider { color: rgb(var(--v-theme-error)); }
}

.board-center--unassigned {
  .board-no, .board-provider { color: rgb(var(--v-theme-warning)); }
}

.board-center--idle {
  .board-no, .board-provider { color: rgba(var(--v-theme-on-background), .2); }
}
</style>
