<script setup>
import { useStatusStore } from '@/stores/status'
import AppHeader from '@/components/AppHeader.vue'

const status = useStatusStore()
let timer = null

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

onMounted(async () => {
  if (isTauriRuntime) {
    await status.refreshAll()
    timer = setInterval(() => status.refreshAll(), 5000)
  }
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
})

const formatBytes = bytes => {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
  return `${v.toFixed(i ? 1 : 0)} ${units[i]}`
}
</script>

<template>
  <div>
    <AppHeader title="儀表板" subtitle="本地分揀中介服務即時狀態" icon="tabler-layout-dashboard" />

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      瀏覽器預覽模式 — 狀態資料來自 Tauri 後端,請於桌面 App 內查看實際數值。
    </VAlert>

    <!-- 上半:即時狀態(Middleware / 雲端) -->
    <VRow dense>
      <VCol cols="12" md="6">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar :color="status.server.running ? 'success' : 'error'" variant="tonal">
                <VIcon icon="tabler-server-bolt" />
              </VAvatar>
            </template>
            <VCardTitle>Middleware</VCardTitle>
            <VCardSubtitle>{{ status.server.bind_addr || '未啟動' }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>

      <VCol cols="12" md="6">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar :color="status.cloud.logged_in ? 'success' : 'warning'" variant="tonal">
                <VIcon icon="tabler-cloud-check" />
              </VAvatar>
            </template>
            <VCardTitle>雲端連線</VCardTitle>
            <VCardSubtitle>{{ status.cloud.logged_in ? status.cloud.api_base : '尚未登入' }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <!-- 下半:本日統計(請求/成功率/cache 命中率/快取容量) -->
    <VRow dense>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-arrows-exchange" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.today.request_count }}</VCardTitle>
            <VCardSubtitle>本日請求數</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="success" variant="tonal">
                <VIcon icon="tabler-circle-check" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.successRate }}%</VCardTitle>
            <VCardSubtitle>本日成功率 ({{ status.today.success_count }}/{{ status.today.request_count }})</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="warning" variant="tonal">
                <VIcon icon="tabler-target-arrow" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.cacheHitRatePct }}%</VCardTitle>
            <VCardSubtitle>Cache 命中率 ({{ status.cache.hit_count }} hit / {{ status.cache.miss_count }} miss)</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-database" />
              </VAvatar>
            </template>
            <VCardTitle>{{ status.cache.file_count }}</VCardTitle>
            <VCardSubtitle>快取面單 / {{ formatBytes(status.cache.total_bytes) }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <VAlert v-if="!status.cloud.logged_in && isTauriRuntime" type="warning" variant="tonal" class="mt-4" icon="tabler-alert-triangle">
      尚未登入雲端 API — 請至「雲端 API」頁完成登入,否則工控機查包裹會回 401。
    </VAlert>
  </div>
</template>
