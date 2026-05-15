<script setup>
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { getConfig, updateConfig, cacheStats, cacheClear } from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const config = ref(null)
const stats = ref({ file_count: 0, total_bytes: 0, hit_count: 0, miss_count: 0, hit_rate: 0 })
const saving = ref(false)
const clearing = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const formatBytes = bytes => {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
  return `${v.toFixed(i ? 1 : 0)} ${units[i]}`
}

const totalSizeText = computed(() => formatBytes(stats.value.total_bytes))
const hitRatePct = computed(() => Math.round((stats.value.hit_rate || 0) * 100))

const load = async () => {
  config.value = await getConfig()
  if (isTauriRuntime) {
    try {
      stats.value = await cacheStats()
    } catch (e) {
      // 後端 command 尚未生效時不阻塞 UI
      errorMsg.value = `讀取快取統計失敗:${String(e?.message || e)}`
    }
  }
}
onMounted(load)

const save = async () => {
  errorMsg.value = ''
  saving.value = true
  try {
    config.value = await updateConfig(JSON.parse(JSON.stringify(config.value)))
    flashMsg.value = '已儲存(快取目錄變更會立即生效)'
    setTimeout(() => (flashMsg.value = ''), 3000)
    await load()
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    saving.value = false
  }
}

const pickCacheDir = async () => {
  if (!isTauriRuntime) {
    errorMsg.value = '瀏覽器模式無法開啟資料夾對話框'
    return
  }
  const dir = await openDialog({ directory: true, title: '選擇圖片快取目錄' })
  if (typeof dir === 'string' && dir) {
    config.value.cache.dir = dir
  }
}

const handleClear = async () => {
  clearing.value = true
  errorMsg.value = ''
  try {
    await cacheClear()
    flashMsg.value = '快取已清空'
    setTimeout(() => (flashMsg.value = ''), 3000)
    await load()
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    clearing.value = false
  }
}
</script>

<template>
  <div v-if="config">
    <AppHeader title="圖片快取設定" subtitle="面單圖檔在本機保留的方式" icon="tabler-photo">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="error" :loading="clearing" :disabled="!isTauriRuntime" @click="handleClear">
            <VIcon icon="tabler-trash" size="16" class="me-1" />清空快取
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="handleClear">
                <template #prepend><VIcon icon="tabler-trash" size="20" /></template>
                <VListItemTitle>清空快取</VListItemTitle>
              </VListItem>
            </VList>
          </VMenu>
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <VRow dense class="mb-2">
      <VCol cols="12" md="3">
        <VCard class="card-shadow stat-card">
          <VCardItem>
            <template #prepend>
              <VAvatar color="primary" variant="tonal"><VIcon icon="tabler-photo" /></VAvatar>
            </template>
            <VCardTitle>{{ stats.file_count }}</VCardTitle>
            <VCardSubtitle>快取面單數</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="3">
        <VCard class="card-shadow stat-card">
          <VCardItem>
            <template #prepend>
              <VAvatar color="info" variant="tonal"><VIcon icon="tabler-database" /></VAvatar>
            </template>
            <VCardTitle>{{ totalSizeText }}</VCardTitle>
            <VCardSubtitle>佔用空間</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="3">
        <VCard class="card-shadow stat-card">
          <VCardItem>
            <template #prepend>
              <VAvatar color="success" variant="tonal"><VIcon icon="tabler-target-arrow" /></VAvatar>
            </template>
            <VCardTitle>{{ hitRatePct }}%</VCardTitle>
            <VCardSubtitle>命中率 (hit / hit+miss)</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
      <VCol cols="12" md="3">
        <VCard class="card-shadow stat-card">
          <VCardItem>
            <template #prepend>
              <VAvatar color="warning" variant="tonal"><VIcon icon="tabler-cloud-download" /></VAvatar>
            </template>
            <VCardTitle>{{ stats.miss_count }}</VCardTitle>
            <VCardSubtitle>Miss 次數 (本日)</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <VCard>
      <VCardText>
        <div class="mb-4">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">快取目錄</VLabel>
          <div class="d-flex gap-2">
            <VTextField
              v-model="config.cache.dir"
              placeholder="留白代表使用預設位置 (app_data/cache/labels)"
              hide-details
              class="flex-grow-1"
            />
            <VBtn variant="tonal" :disabled="!isTauriRuntime" @click="pickCacheDir">
              <VIcon icon="tabler-folder-open" size="18" class="me-1" />選擇
            </VBtn>
          </div>
        </div>

        <VRow dense>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">保留天數(0 = 永久)</VLabel>
            <VNumberInput v-model="config.cache.keep_days" :min="0" />
          </VCol>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">最大容量 MB(0 = 不限)</VLabel>
            <VNumberInput v-model="config.cache.max_size_mb" :min="0" />
          </VCol>
        </VRow>
      </VCardText>
    </VCard>

    <div class="d-flex justify-center mt-4">
      <VBtn :loading="saving" color="primary" size="large" @click="save">
        <VIcon icon="tabler-device-floppy" size="18" class="me-2" />儲存設定
      </VBtn>
    </div>
  </div>
</template>

<style scoped lang="scss">
.stat-card :deep(.v-card-item) {
  padding: 10px 14px;
}
</style>
