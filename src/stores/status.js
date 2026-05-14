import { defineStore } from 'pinia'
import { cloudSession, serverStatus, queueStats, cacheStats, dailyStats } from '@/api/tauri'

export const useStatusStore = defineStore('status', {
  state: () => ({
    cloud: { logged_in: false, api_base: '', user_label: null },
    server: { running: false, bind_addr: '' },
    queue: { pending: 0, sending: 0, success: 0, failed: 0 },
    cache: { file_count: 0, total_bytes: 0, hit_count: 0, miss_count: 0, hit_rate: 0 },
    today: { date: '', request_count: 0, success_count: 0, cache_hit: 0, cache_miss: 0 },
    refreshingAt: null,
  }),
  getters: {
    successRate(state) {
      const req = state.today.request_count
      if (!req) return 0
      return Math.round((state.today.success_count / req) * 100)
    },
    cacheHitRatePct(state) {
      return Math.round((state.cache.hit_rate || 0) * 100)
    },
  },
  actions: {
    async refreshAll() {
      const [c, s, q, ca, d] = await Promise.allSettled([
        cloudSession(),
        serverStatus(),
        queueStats(),
        cacheStats(),
        dailyStats({ days: 1 }),
      ])
      if (c.status === 'fulfilled') this.cloud = c.value
      if (s.status === 'fulfilled') this.server = s.value
      if (q.status === 'fulfilled') this.queue = q.value
      if (ca.status === 'fulfilled') this.cache = ca.value
      if (d.status === 'fulfilled' && d.value.length > 0) {
        this.today = d.value[0]
      } else if (d.status === 'fulfilled') {
        this.today = { date: '', request_count: 0, success_count: 0, cache_hit: 0, cache_miss: 0 }
      }
      this.refreshingAt = new Date()
    },
  },
})
