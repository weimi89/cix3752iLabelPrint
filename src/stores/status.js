import { defineStore } from 'pinia'
import { cloudSession, serverStatus, queueStats } from '@/api/tauri'

export const useStatusStore = defineStore('status', {
  state: () => ({
    cloud: { logged_in: false, api_base: '', user_label: null },
    server: { running: false, bind_addr: '' },
    queue: { pending: 0, sending: 0, success: 0, failed: 0 },
    refreshingAt: null,
  }),
  actions: {
    async refreshAll() {
      const [c, s, q] = await Promise.allSettled([
        cloudSession(),
        serverStatus(),
        queueStats(),
      ])
      if (c.status === 'fulfilled') this.cloud = c.value
      if (s.status === 'fulfilled') this.server = s.value
      if (q.status === 'fulfilled') this.queue = q.value
      this.refreshingAt = new Date()
    },
  },
})
