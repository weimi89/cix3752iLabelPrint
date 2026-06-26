<script setup>
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { getConfig, updateConfig, cacheStats, cacheClear, cameraSetZoom, cameraCaptureNow } from '@/api/tauri'
import { clearProcessed } from '@/composables/usePreGenProcessed'
import AppHeader from '@/components/AppHeader.vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const config = ref(null)
const stats = ref({ file_count: 0, total_bytes: 0, hit_count: 0, miss_count: 0, hit_rate: 0 })
const saving = ref(false)
const clearing = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// 相機預覽對話框:用 MJPEG 串流(<img> 顯示 multipart/x-mixed-replace,順暢 ~10fps、不輪詢)。
// 只在對話框開啟時給 src → 串流僅在預覽時連線,關閉即斷流不殘留。
const previewDialog = ref(false)
const previewUrl = computed(() =>
  previewDialog.value && isTauriRuntime && config.value?.camera?.enabled && config.value?.server?.port
    ? `http://127.0.0.1:${config.value.server.port}/camera/preview/stream`
    : '',
)
// 對話框裡拖變焦滑桿:即時套用到執行中相機(不存檔,後端下一幀就反映到串流),同時更新表單值供之後儲存持久化
const onZoomInput = async val => {
  config.value.camera.zoom = val
  try { await cameraSetZoom(val) } catch { /* 相機未啟用 / 後端未就緒時忽略 */ }
}
// 對話框裡按「拍照」:抓當下最新一幀(含 zoom)存進存證目錄,回報存到哪
const capturing = ref(false)
const captureMsg = ref('')
const onCapture = async () => {
  capturing.value = true
  captureMsg.value = ''
  try {
    const key = await cameraCaptureNow()
    captureMsg.value = key
      ? t('page.cache.camera.captureSaved', { name: key })
      : t('page.cache.camera.captureNoFrame')
  } catch (e) {
    captureMsg.value = String(e?.message || e)
  } finally {
    capturing.value = false
  }
}

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
  // 舊設定檔可能尚無 camera 區段(後端 serde default 會補,但 web preview / 舊版需前端兜底)
  if (!config.value.camera) config.value.camera = { enabled: false, device_index: 0, jpeg_quality: 80, zoom: 1, captures_dir: '', keep_days: 90 }
  if (isTauriRuntime) {
    try {
      stats.value = await cacheStats()
    } catch (e) {
      // 後端 command 尚未生效時不阻塞 UI
      errorMsg.value = t('page.cache.statsLoadFailed', { reason: String(e?.message || e) })
    }
  }
}
onMounted(load)

const save = async () => {
  errorMsg.value = ''
  saving.value = true
  try {
    config.value = await updateConfig(JSON.parse(JSON.stringify(config.value)))
    flashMsg.value = t('page.cache.savedFlash')
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
    errorMsg.value = t('error.browserCannotOpenDialog')
    return
  }
  const dir = await openDialog({ directory: true, title: t('page.cache.pickDirTitle') })
  if (typeof dir === 'string' && dir) {
    config.value.cache.dir = dir
  }
}

const pickCapturesDir = async () => {
  if (!isTauriRuntime) {
    errorMsg.value = t('error.browserCannotOpenDialog')
    return
  }
  const dir = await openDialog({ directory: true, title: t('page.cache.camera.pickCapturesDirTitle') })
  if (typeof dir === 'string' && dir) {
    config.value.camera.captures_dir = dir
  }
}

const handleClear = async () => {
  clearing.value = true
  errorMsg.value = ''
  try {
    await cacheClear()
    // 連帶清掉面單預產的「已預產」前端去重記憶,否則清了後端快取、預產仍會把訂單判為已預產而略過、無法真正重跑
    clearProcessed()
    flashMsg.value = t('page.cache.clearedFlash')
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
    <AppHeader :title="$t('page.cache.title')" :subtitle="$t('page.cache.subtitle')" icon="tabler-photo">
      <template #actions>
        <div class="d-none d-md-flex ga-2">
          <VBtn color="error" :loading="clearing" :disabled="!isTauriRuntime" @click="handleClear">
            <VIcon icon="tabler-trash" size="16" class="me-1" />{{ $t('page.cache.clearCache') }}
          </VBtn>
        </div>
        <VBtn class="d-block d-md-none" icon variant="tonal" color="default" density="compact" size="34">
          <VIcon icon="tabler-playlist-add" size="22" />
          <VMenu activator="parent">
            <VList>
              <VListItem :disabled="!isTauriRuntime" @click="handleClear">
                <template #prepend><VIcon icon="tabler-trash" size="20" /></template>
                <VListItemTitle>{{ $t('page.cache.clearCache') }}</VListItemTitle>
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
            <VCardSubtitle>{{ $t('page.cache.stats.fileCount') }}</VCardSubtitle>
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
            <VCardSubtitle>{{ $t('page.cache.stats.totalSize') }}</VCardSubtitle>
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
            <VCardSubtitle>{{ $t('page.cache.stats.hitRate') }}</VCardSubtitle>
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
            <VCardSubtitle>{{ $t('page.cache.stats.missCount') }}</VCardSubtitle>
          </VCardItem>
        </VCard>
      </VCol>
    </VRow>

    <VCard>
      <VCardText>
        <div class="mb-4">
          <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.cacheDir') }}</VLabel>
          <div class="d-flex gap-2">
            <VTextField
              v-model="config.cache.dir"
              :placeholder="$t('page.cache.cacheDirPlaceholder')"
              hide-details
              class="flex-grow-1"
            />
            <VBtn variant="tonal" :disabled="!isTauriRuntime" @click="pickCacheDir">
              <VIcon icon="tabler-folder-open" size="18" class="me-1" />{{ $t('common.pick') }}
            </VBtn>
          </div>
        </div>

        <VRow dense>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.keepDays') }}</VLabel>
            <VNumberInput v-model="config.cache.keep_days" :min="0" />
          </VCol>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.maxSizeMb') }}</VLabel>
            <VNumberInput v-model="config.cache.max_size_mb" :min="0" />
          </VCol>
        </VRow>
      </VCardText>
    </VCard>

    <!-- 讀碼站快照相機:收到工控機 GET /api/parcel 當下抓 USB 相機一幀存證,佐證「沒貨卻出紙」 -->
    <VCard v-if="config.camera" class="mt-4">
      <VCardItem>
        <VCardTitle class="text-body-1">
          <VIcon icon="tabler-camera" size="20" class="me-1" />{{ $t('page.cache.camera.title') }}
        </VCardTitle>
        <VCardSubtitle>{{ $t('page.cache.camera.subtitle') }}</VCardSubtitle>
      </VCardItem>
      <VDivider />
      <VCardText>
        <VSwitch
          v-model="config.camera.enabled"
          color="primary"
          :label="$t('page.cache.camera.enable')"
          hide-details
          class="mb-1"
        />
        <div class="text-caption text-disabled mb-4">{{ $t('page.cache.camera.enableHint') }}</div>

        <!-- 相機預覽 / 對位:開對話框看即時串流並即時調變焦(拖滑桿立即生效、不必存檔) -->
        <div v-if="config.camera.enabled" class="mb-4">
          <VBtn variant="tonal" color="primary" :disabled="!isTauriRuntime" @click="previewDialog = true; captureMsg = ''">
            <VIcon icon="tabler-camera-search" size="18" class="me-1" />{{ $t('page.cache.camera.openPreview') }}
          </VBtn>
        </div>

        <VRow dense>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.camera.deviceIndex') }}</VLabel>
            <VNumberInput v-model="config.camera.device_index" :min="0" :disabled="!config.camera.enabled" />
            <div class="text-caption text-disabled mt-1">{{ $t('page.cache.camera.deviceIndexHint') }}</div>
          </VCol>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.camera.jpegQuality') }}</VLabel>
            <VNumberInput v-model="config.camera.jpeg_quality" :min="1" :max="100" :disabled="!config.camera.enabled" />
            <div class="text-caption text-disabled mt-1">{{ $t('page.cache.camera.jpegQualityHint') }}</div>
          </VCol>
        </VRow>

        <VRow dense class="mt-2">
          <VCol cols="12">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.camera.capturesDir') }}</VLabel>
            <div class="d-flex gap-2">
              <VTextField
                v-model="config.camera.captures_dir"
                :placeholder="$t('page.cache.camera.capturesDirPlaceholder')"
                hide-details
                class="flex-grow-1"
              />
              <VBtn variant="tonal" :disabled="!isTauriRuntime" @click="pickCapturesDir">
                <VIcon icon="tabler-folder-open" size="18" class="me-1" />{{ $t('common.pick') }}
              </VBtn>
            </div>
            <div class="text-caption text-disabled mt-1">{{ $t('page.cache.camera.capturesDirHint') }}</div>
          </VCol>
          <VCol cols="12" md="6">
            <VLabel class="mb-1 text-body-2" style="line-height: 15px;">{{ $t('page.cache.camera.keepDays') }}</VLabel>
            <VNumberInput v-model="config.camera.keep_days" :min="0" />
            <div class="text-caption text-disabled mt-1">{{ $t('page.cache.camera.keepDaysHint') }}</div>
          </VCol>
        </VRow>
      </VCardText>
    </VCard>

    <!-- 相機預覽對話框:即時 MJPEG 串流 + 即時變焦(拖滑桿立刻生效、不必存檔) -->
    <VDialog v-model="previewDialog" max-width="560">
      <div style="position: relative;">
        <VBtn
          icon
          variant="elevated"
          size="x-small"
          style="position: absolute; top: -12px; right: -12px; z-index: 10;"
          @click="previewDialog = false"
        >
          <VIcon icon="tabler-x" size="14" />
        </VBtn>
        <VCard>
          <VCardItem>
            <VCardTitle class="text-body-1">{{ $t('page.cache.camera.previewTitle') }}</VCardTitle>
          </VCardItem>
          <VDivider />
          <VCardText>
            <div class="camera-preview mx-auto">
              <img v-if="previewUrl" :src="previewUrl" class="camera-preview__img" alt="" />
              <div v-else class="camera-preview__waiting text-disabled text-caption">{{ $t('page.cache.camera.previewWaiting') }}</div>
            </div>
            <div class="mt-4">
              <VLabel class="mb-1 text-body-2" style="line-height: 15px;">
                {{ $t('page.cache.camera.zoom') }}:&nbsp;{{ Number(config.camera.zoom || 1).toFixed(1) }}x
              </VLabel>
              <VSlider
                :model-value="config.camera.zoom"
                :min="1"
                :max="4"
                :step="0.1"
                thumb-label
                hide-details
                @update:model-value="onZoomInput"
              />
              <div class="text-caption text-disabled">{{ $t('page.cache.camera.zoomLiveHint') }}</div>
            </div>
            <div class="d-flex align-center mt-4 ga-3">
              <span v-if="captureMsg" class="text-caption text-success" style="word-break: break-all;">{{ captureMsg }}</span>
              <VSpacer />
              <VBtn color="primary" :loading="capturing" :disabled="!previewUrl" @click="onCapture">
                <VIcon icon="tabler-camera" size="18" class="me-1" />{{ $t('page.cache.camera.capture') }}
              </VBtn>
            </div>
          </VCardText>
        </VCard>
      </div>
    </VDialog>

    <div class="d-flex justify-center mt-4">
      <VBtn :loading="saving" color="primary" size="large" @click="save">
        <VIcon icon="tabler-device-floppy" size="18" class="me-2" />{{ $t('common.saveSettings') }}
      </VBtn>
    </div>
  </div>
</template>

<style scoped lang="scss">
.stat-card :deep(.v-card-item) {
  padding: 10px 14px;
}

.camera-preview {
  max-width: 480px;
  border: 1px solid rgba(var(--v-border-color), var(--v-border-opacity));
  border-radius: 6px;
  overflow: hidden;
  background: rgba(var(--v-theme-on-surface), 0.04);
}

.camera-preview__img {
  display: block;
  width: 100%;
  height: auto;
}

.camera-preview__waiting {
  padding: 32px 12px;
  text-align: center;
}
</style>
