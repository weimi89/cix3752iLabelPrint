<script setup>
import { useStatusStore } from '@/stores/status'
import AppHeader from '@/components/AppHeader.vue'

const status = useStatusStore()
let timer = null

onMounted(async () => {
  await status.refreshAll()
  timer = setInterval(() => status.refreshAll(), 5000)
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
})
</script>

<template>
  <div>
    <AppHeader title="首頁狀態" subtitle="本地分揀中介服務即時狀態" icon="tabler-layout-dashboard" />

    <VRow>
      <VCol cols="12" md="6" lg="3">
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

      <VCol cols="12" md="6" lg="3">
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

      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal">
                <VIcon icon="tabler-truck-loading" />
              </VAvatar>
            </template>
            <VCardTitle>Queue 待送</VCardTitle>
            <VCardSubtitle>{{ status.queue.pending }} 筆 pending / {{ status.queue.failed }} 失敗</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>

      <VCol cols="12" md="6" lg="3">
        <VCard class="card-shadow">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal">
                <VIcon icon="tabler-rosette-discount-check" />
              </VAvatar>
            </template>
            <VCardTitle>已成功推送</VCardTitle>
            <VCardSubtitle>{{ status.queue.success }} 筆</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <VAlert v-if="!status.cloud.logged_in" type="warning" variant="tonal" class="mt-4" icon="tabler-alert-triangle">
      尚未登入雲端 API — 請至「雲端 API」頁完成登入，否則工控機查包裹會回 401。
    </VAlert>
  </div>
</template>
