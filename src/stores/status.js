import { defineStore } from 'pinia'
import { cloudSession, serverStatus, queueStats, cacheStats, dailyStats, printStatsSummary } from '@/api/tauri'

export const useStatusStore = defineStore('status', {
  state: () => ({
    cloud: { logged_in: false, api_base: '', user_label: null },
    server: { running: false, bind_addr: '' },
    queue: { pending: 0, sending: 0, success: 0, failed: 0, cancelled: 0 },
    cache: { file_count: 0, total_bytes: 0, hit_count: 0, miss_count: 0, hit_rate: 0 },
    today: { date: '', request_count: 0, success_count: 0, noread_count: 0, cache_hit: 0, cache_miss: 0 },
    // Navbar chip 顯示:本場累計 + 過去 24 小時(對齊 PrintStatsPage 的新 KPI)
    printStats: { since_reset: 0, past_24h: 0, since_reset_at: '' },
    refreshingAt: null,
  }),
  getters: {
    // 有效請求數 = 總請求 − NoRead(讀碼失敗未打雲端,不計入雲端查詢成功率分母)。
    // 成功率與儀表板「({ok}/{total})」共用此定義,避免兩處各算一份而不一致。
    effectiveTodayRequests(state) {
      return Math.max(0, state.today.request_count - (state.today.noread_count || 0))
    },
    successRate() {
      const req = this.effectiveTodayRequests
      if (req <= 0) return 0
      return Math.round((this.today.success_count / req) * 100)
    },
    cacheHitRatePct(state) {
      return Math.round((state.cache.hit_rate || 0) * 100)
    },
  },
  actions: {
    async refreshPrintStats() {
      try {
        const s = await printStatsSummary()
        if (s) this.printStats = {
          since_reset: s.since_reset || 0,
          past_24h: s.past_24h || 0,
          since_reset_at: s.since_reset_at || '',
        }
      } catch (e) {
        console.warn('印單統計載入失敗', e)
      }
    },
    async refreshAll() {
      const [c, s, q, ca, d, p] = await Promise.allSettled([
        cloudSession(),
        serverStatus(),
        queueStats(),
        cacheStats(),
        dailyStats({ days: 1 }),
        printStatsSummary(),
      ])
      if (c.status === 'fulfilled') this.cloud = c.value
      if (s.status === 'fulfilled') this.server = s.value
      if (q.status === 'fulfilled') this.queue = q.value
      if (ca.status === 'fulfilled') this.cache = ca.value
      if (d.status === 'fulfilled' && d.value.length > 0) {
        this.today = d.value[0]
      } else if (d.status === 'fulfilled') {
        this.today = { date: '', request_count: 0, success_count: 0, noread_count: 0, cache_hit: 0, cache_miss: 0 }
      }
      if (p.status === 'fulfilled' && p.value) {
        this.printStats = {
          since_reset: p.value.since_reset || 0,
          past_24h: p.value.past_24h || 0,
          since_reset_at: p.value.since_reset_at || '',
        }
      }
      this.refreshingAt = new Date()
    },
  },
})
