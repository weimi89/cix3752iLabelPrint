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

// 通道啟用 / 暫停是慢速變動的設定,不必跟著每件包裹重抓
const CHANNEL_POLL_MS = 10000

const channels = ref([])
const activePos = ref(null)
// 中央顯示:等待第一件時是 idle
const board = ref({ no: '', provider: '', status: 'idle', message: '', at: '' })

const leftSlots = computed(() => channels.value.filter(c => c.position?.[0] === 'L'))
const rightSlots = computed(() => channels.value.filter(c => c.position?.[0] === 'R'))

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
// 異常訊息在逗號處斷行:「無法列印,訂單當前狀態異常」拆成兩行比擠成一長條好讀,
// 行變短字也能放更大
const noteLines = computed(() =>
  noteText.value.split(/[\uFF0C,、]/).map(t => t.trim()).filter(Boolean),
)
// 字級除以「最長那一行」的字數:訊息長短差很多,固定字級會換行並蓋到兩側燈號
const noteLen = computed(() => Math.max(8, ...noteLines.value.map(l => l.length), 0))

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

const applyEvent = payload => {
  if (!payload) return
  board.value = {
    no: payload.no || '',
    provider: payload.provider || '',
    status: payload.status || 'ok',
    message: payload.message || '',
    at: payload.at || '',
  }
  // 綠燈一直亮到下一件進來 —— 現場隨時看得出「最後一件去了哪一格」,
  // 停機時也還看得到上一件的去向
  activePos.value = payload.position || null
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
        <span class="slot__lamp">{{ c.position }}</span>
      </div>
    </div>

    <div class="board-center" :class="`board-center--${board.status}`">
        <div v-if="board.at" class="board-time">{{ board.at }}</div>
      <div class="board-no" :style="{ '--len': noLen }">
        <span class="board-no__head">{{ noHead }}</span>
        <span class="board-no__tail">{{ noTail }}</span>
      </div>
      <div class="board-sub" :style="{ '--nlen': noteLen }">
        <div class="board-provider">{{ providerText }}</div>
        <div v-if="noteLines.length" class="board-note">
          <div v-for="(line, i) in noteLines" :key="i">{{ line }}</div>
        </div>
      </div>
    </div>

    <div class="board-col board-col--right">
      <div
        v-for="c in rightSlots"
        :key="c.position"
        class="slot"
        :class="slotClass(c.position)"
      >
        <span class="slot__lamp">{{ c.position }}</span>
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

  /* 側欄用固定寬(燈 6vw + 間距 + 位置名約 11.8vw,取 15vw 有餘裕):
     用 auto 的話中欄寬度會隨內容浮動,字級公式只能用猜的,長單號就可能貼到燈號上 */
  position: relative;
  grid-template-columns: 6.5vw 1fr 6.5vw;
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
    display: flex;
    flex: none;
    align-items: center;
    justify-content: center;

    /* 位置標示放進燈裡:掛在燈旁邊會吃掉中欄的寬度,單號就放不大 */
    color: rgba(var(--v-theme-on-background), .55);
    font-size: 2vw;
    font-weight: 800;
    letter-spacing: -.02em;
    inline-size: 5.5vw;
    block-size: 5.5vw;
    min-inline-size: 44px;
    min-block-size: 44px;
    /* 底色是淡灰,灰燈太淺會糊在背景裡,壓深到看得出「這格還沒輪到」 */
    border: .3vw solid rgba(var(--v-theme-on-background), .3);
    border-radius: 50%;
    background: rgba(var(--v-theme-on-background), .2);
    transition: background .25s ease, box-shadow .25s ease, border-color .25s ease;
  }

  &--active &__lamp {
    border-color: rgba(var(--v-theme-success), .45);
    background: rgb(var(--v-theme-success));
    color: #fff;
    box-shadow: 0 0 2.2vw rgba(var(--v-theme-success), .5);
  }

  &--paused &__lamp {
    border-color: rgba(var(--v-theme-warning), .45);
    background: rgb(var(--v-theme-warning));
    color: #fff;
    box-shadow: 0 0 1.4vw rgba(var(--v-theme-warning), .45);
  }
}

/* 單號偏上、下面留大一點的空間:異常訊息字大又長短不一,
   若把單號放在正中央,訊息一長就會把版面往下擠出去。 */
.board-center {
  display: grid;
  grid-template-rows: 0.55fr auto 1.45fr;
  justify-items: center;
  min-inline-size: 0;

  /* 中欄兩側再留一點安全邊距,單號不會貼著燈號 */
  padding-inline: 1.5vw;
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
  /* 前後段同一個顏色:淡色在監視器上反光就看不到了 */
  &__head {
    font-size: min(14vw, calc(132vw / (var(--len) + 2)));
  }

  /* 後四碼再加大:現場核對只看這幾碼 */
  &__tail {
    margin-inline-start: .06em;
    font-size: min(20.6vw, calc(194vw / (var(--len) + 2)));
  }
}

.board-provider {
  color: rgb(var(--v-theme-on-background));
  font-size: clamp(22px, 4.4vw, 86px);
  font-weight: 700;
  text-align: center;
}

/* 分揀時刻貼在單號正上方,只佔中欄(不跨到左右燈號那兩欄,也不另外吃一整列高度)。
   藍色是刻意寫死的:主題色盤裡沒有藍(info 偏青綠),這裡要的是跟紅色單號、
   黑色訊息都分得開的第三個顏色 */
.board-time {
  /* 貼在看板最上緣正中央。用絕對定位是刻意的:它不佔版面高度,
     單號、物流名、訊息與兩側燈號都留在原本的位置 */
  position: absolute;
  inset-block-start: 2vh;
  inset-inline: 0;
  color: #0D47A1;
  line-height: 1;
  font-size: clamp(24px, 6vw, 116px);
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  text-align: center;
}

/* 異常原因是現場要據以處理的資訊,不是附註 —— 用正文色與接近物流名的字級,
   遠處才看得清楚 */
.board-note {
  color: rgb(var(--v-theme-on-background));
  font-size: min(5.4vw, calc(62vw / var(--nlen)));
  font-weight: 700;
  line-height: 1.25;
  text-align: center;
}

/* 三種狀態的字色:正常白、查件異常紅、沒有指派通道黃 */
.board-center--ok .board-no { color: rgb(var(--v-theme-on-background)); }

/* 監視器會反光,狀態色一律用深一階的版本(主題色太亮) */
.board-center--error {
  .board-no, .board-provider { color: #C62828; }
}

.board-center--unassigned {
  .board-no, .board-provider { color: #E65100; }
}

.board-center--idle {
  .board-no, .board-provider { color: rgba(var(--v-theme-on-background), .2); }
}
</style>
