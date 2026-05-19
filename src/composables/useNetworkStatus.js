import { ref, computed, onBeforeUnmount } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { networkHealthGet, networkHealthCheck } from '@/api/tauri'

const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 全域單例:任何元件 useNetworkStatus() 共享同一份狀態
const osOnline = ref(typeof navigator !== 'undefined' ? navigator.onLine : true)
const snapshot = ref(null)
const isChecking = ref(false)
let initialized = false
let unlistenEvent = null

function attachBrowserListeners() {
  if (typeof window === 'undefined') return
  window.addEventListener('online', () => { osOnline.value = true })
  window.addEventListener('offline', () => { osOnline.value = false })
}

async function attachTauriListener() {
  if (!isTauri || unlistenEvent) return
  unlistenEvent = await listen('network-status', evt => {
    snapshot.value = evt.payload
  })
}

async function init() {
  if (initialized) return
  initialized = true
  attachBrowserListeners()
  try {
    snapshot.value = await networkHealthGet()
  } catch {
    // 後端尚未就緒,等 event 進來
  }
  await attachTauriListener()
}

export function useNetworkStatus() {
  init()

  const anchor = computed(() => snapshot.value?.anchor ?? null)
  const cloudApi = computed(() => snapshot.value?.cloud_api ?? null)
  const checkedAtMs = computed(() => snapshot.value?.checked_at_ms ?? 0)
  const anchorFailStreak = computed(() => snapshot.value?.anchor_fail_streak ?? 0)
  const cloudFailStreak = computed(() => snapshot.value?.cloud_fail_streak ?? 0)
  const failThreshold = computed(() => snapshot.value?.fail_threshold ?? 2)
  const effectiveIntervalSecs = computed(() => snapshot.value?.effective_interval_secs ?? 15)

  // 經抖動緩衝:streak < threshold 時不算 down
  const anchorEffectiveOk = computed(() => snapshot.value?.anchor_effective_ok ?? true)
  const cloudEffectiveOk = computed(() => snapshot.value?.cloud_effective_ok ?? null)

  const isAnchorOk = computed(() => anchor.value?.kind === 'ok')
  const isCloudOk = computed(() =>
    cloudApi.value?.kind === 'reachable' || cloudApi.value?.kind === 'not_configured',
  )

  // 整體燈號:OS 斷一定紅;經緩衝後的公網斷一定紅;雲端確定斷算降級;雲端未設定獨立狀態(不混進 ok)
  const overall = computed(() => {
    if (!osOnline.value) return 'down'
    if (!anchorEffectiveOk.value) return 'down'
    if (cloudApi.value?.kind === 'not_configured') return 'unconfigured'
    if (cloudEffectiveOk.value === false) return 'degraded'
    return 'ok'
  })

  async function checkNow() {
    if (isChecking.value) return
    isChecking.value = true
    try {
      snapshot.value = await networkHealthCheck()
    } finally {
      isChecking.value = false
    }
  }

  return {
    osOnline,
    anchor,
    cloudApi,
    checkedAtMs,
    anchorFailStreak,
    cloudFailStreak,
    failThreshold,
    effectiveIntervalSecs,
    anchorEffectiveOk,
    cloudEffectiveOk,
    isAnchorOk,
    isCloudOk,
    overall,
    isChecking,
    checkNow,
    snapshot,
  }
}

// 提供給 App 卸載時清理 listener(可選)
export function disposeNetworkStatus() {
  if (unlistenEvent) {
    unlistenEvent()
    unlistenEvent = null
  }
  initialized = false
}
