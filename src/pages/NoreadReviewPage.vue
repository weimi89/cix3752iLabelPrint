<script setup>
import { noreadReviewList, getConfig } from '@/api/tauri'
import { listen } from '@/api/events'
import AppHeader from '@/components/AppHeader.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import TablePagination from '@/components/TablePagination.vue'
import PhotoViewer from '@/components/PhotoViewer.vue'
import { useI18n } from 'vue-i18n'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { hasBackend } from '@/api/runtime'
import { mediaUrl as buildMediaUrl } from '@/api/media'
import { toast } from 'vue3-toastify'

const { t } = useI18n()

const pad = n => String(n).padStart(2, '0')
const today = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` }
const day = ref(today())
const page = ref(1)
const pageSize = ref(24)
const items = ref([])
const total = ref(0)
const loading = ref(false)
const errorMsg = ref('')

// 存證照走本機 server 的 /captures(桌面帶權杖、網頁版相對路徑,見 api/media.js)
const serverPort = ref(18080)
const photoUrl = key => (key ? buildMediaUrl(`/captures/${encodeURI(key)}`, serverPort.value) : '')

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const r = await noreadReviewList({ day: day.value, limit: pageSize.value, offset: (page.value - 1) * pageSize.value })
    items.value = r.items
    total.value = r.total
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  } finally {
    loading.value = false
  }
}
const search = () => { page.value = 1; load() }
watch(day, search)
watch(pageSize, search)
watch(page, load)

const viewer = ref({ open: false, url: '', title: '' })
const openPhoto = item => {
  if (!item.photo_path) return
  viewer.value = { open: true, url: photoUrl(item.photo_path), title: item.created_at }
}
const photoMissing = () => toast(t('page.noreadReview.noPhoto'), { type: 'warning', autoClose: 3000 })

let unlisten = null
onMounted(async () => {
  if (hasBackend) {
    try { serverPort.value = (await getConfig())?.server?.port || 18080 } catch { /* 用預設埠 */ }
  }
  await load()
  // 新的讀碼失敗進來(今天)就補上
  let timer = null
  unlisten = listen('parcel-query-logged', () => { if (day.value === today() && page.value === 1) { clearTimeout(timer); timer = setTimeout(load, 1500) } })
})
onBeforeUnmount(() => unlisten?.())
</script>

<template>
  <div>
    <AppHeader :title="$t('page.noreadReview.title')" :subtitle="$t('page.noreadReview.subtitle')" :subtitle-short="$t('page.noreadReview.subtitleShort')" icon="tabler-barcode-off" />

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <!-- 篩選 + 當天統計 -->
    <VCard class="mb-3">
      <VCardText>
        <div class="d-flex flex-wrap align-center gap-x-4 gap-y-3">
          <div style="inline-size: 180px;"><AppDatePicker v-model="day" :label="$t('page.noreadReview.day')" :max="today()" /></div>
          <VSpacer />
          <div class="text-center">
            <div class="text-body-small text-medium-emphasis">{{ $t('page.noreadReview.dayTotal') }}</div>
            <div class="text-headline-small font-weight-bold text-warning">{{ total }}</div>
          </div>
        </div>
        <div class="text-body-small text-disabled mt-2">{{ $t('page.noreadReview.hint') }}</div>
      </VCardText>
    </VCard>

    <VCard>
      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" :page-sizes="[12, 24, 48, 96]" header />
      <VDivider />
      <VCardText>
        <div v-if="!items.length" class="py-6 text-center text-medium-emphasis">{{ $t('page.noreadReview.noItems') }}</div>
        <div v-else class="photo-grid">
          <VCard v-for="it in items" :key="it.response_id" variant="outlined" class="photo-card">
            <div class="photo-box" :class="{ 'cursor-pointer': it.photo_path }" @click="openPhoto(it)">
              <img v-if="it.photo_path" :src="photoUrl(it.photo_path)" alt="" loading="lazy" />
              <div v-else class="text-body-small text-disabled text-center px-2">{{ $t('page.noreadReview.noPhoto') }}</div>
            </div>
            <div class="px-2 py-1 text-body-small text-medium-emphasis">{{ it.created_at.slice(11, 19) }}</div>
          </VCard>
        </div>
      </VCardText>
      <VDivider />
      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" :page-sizes="[12, 24, 48, 96]" />
    </VCard>

    <PhotoViewer v-model="viewer.open" :url="viewer.url" :title="viewer.title" @error="photoMissing" />
  </div>
</template>

<style scoped>
.photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
.photo-box { aspect-ratio: 4 / 3; background: rgba(var(--v-theme-on-surface), 0.04); display: flex; align-items: center; justify-content: center; overflow: hidden; }
.photo-box img { inline-size: 100%; block-size: 100%; object-fit: cover; }
</style>
