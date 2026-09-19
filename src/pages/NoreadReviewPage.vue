<script setup>
import { noreadReviewList, noreadReviewTag, getConfig, NOREAD_TAGS } from '@/api/tauri'
import { listen } from '@/api/events'
import AppHeader from '@/components/AppHeader.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import TablePagination from '@/components/TablePagination.vue'
import { useI18n } from 'vue-i18n'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { hasBackend } from '@/api/runtime'
import { mediaUrl as buildMediaUrl } from '@/api/media'
import { toast } from 'vue3-toastify'

const { t } = useI18n()

const pad = n => String(n).padStart(2, '0')
const today = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` }
const day = ref(today())
const tag = ref('')
const untaggedOnly = ref(false)
const page = ref(1)
const pageSize = ref(24)
const items = ref([])
const total = ref(0)
const dayTotal = ref(0)
const tagged = ref(0)
const tagCounts = ref([])
const loading = ref(false)
const errorMsg = ref('')
const busyId = ref(null)

// 存證照走本機 server 的 /captures(桌面帶權杖、網頁版相對路徑,見 api/media.js)
const serverPort = ref(18080)
const photoUrl = key => (key ? buildMediaUrl(`/captures/${encodeURI(key)}`, serverPort.value) : '')

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const r = await noreadReviewList({ day: day.value, tag: tag.value || null, untaggedOnly: untaggedOnly.value, limit: pageSize.value, offset: (page.value - 1) * pageSize.value })
    items.value = r.items
    total.value = r.total
    dayTotal.value = r.day_total
    tagged.value = r.tagged
    tagCounts.value = r.tag_counts
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  } finally {
    loading.value = false
  }
}
const search = () => { page.value = 1; load() }
watch([day, tag, untaggedOnly], search)
watch(pageSize, search)
watch(page, load)

// 同一張再按同一個原因 = 清除
const setTag = async (item, code) => {
  const next = item.tag === code ? null : code
  busyId.value = item.response_id
  try {
    await noreadReviewTag(item.response_id, next)
    await load()
  } catch (e) {
    toast(errorMessageFromException(e), { type: 'error' })
  } finally {
    busyId.value = null
  }
}

const tagPct = c => (tagged.value ? Math.round((c.count / tagged.value) * 100) : 0)
const tagColor = code => ({ label_back: 'warning', glare: 'info', damaged: 'error', small: 'secondary', position: 'primary', no_label: 'error', other: 'secondary' }[code] || 'secondary')

const viewer = ref({ open: false, url: '', title: '' })
const viewerError = ref(false)
const openPhoto = item => {
  if (!item.photo_path) return
  viewerError.value = false
  viewer.value = { open: true, url: photoUrl(item.photo_path), title: item.created_at }
}

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
          <VSwitch v-model="untaggedOnly" :label="$t('page.noreadReview.untaggedOnly')" color="primary" hide-details density="compact" />
          <VSpacer />
          <div class="d-flex align-center gap-4">
            <div class="text-center">
              <div class="text-body-small text-medium-emphasis">{{ $t('page.noreadReview.dayTotal') }}</div>
              <div class="text-headline-small font-weight-bold text-warning">{{ dayTotal }}</div>
            </div>
            <div class="text-center">
              <div class="text-body-small text-medium-emphasis">{{ $t('page.noreadReview.tagged') }}</div>
              <div class="text-headline-small font-weight-bold">{{ tagged }}<span class="text-body-small text-medium-emphasis"> / {{ dayTotal }}</span></div>
            </div>
          </div>
        </div>
        <!-- 各原因件數:點一個只看那個原因,再點一次回全部 -->
        <div class="d-flex flex-wrap gap-2 mt-3">
          <VChip :color="tag ? 'secondary' : 'primary'" :variant="tag ? 'tonal' : 'flat'" size="small" label @click="tag = ''">{{ $t('page.noreadReview.allTags') }} {{ tagged }}</VChip>
          <VChip v-for="c in tagCounts" :key="c.tag" :color="tagColor(c.tag)" :variant="tag === c.tag ? 'flat' : 'tonal'" size="small" label @click="tag = tag === c.tag ? '' : c.tag">
            {{ $t(`page.noreadReview.tag.${c.tag}`) }} {{ c.count }}<span v-if="c.count" class="ms-1 text-medium-emphasis">({{ tagPct(c) }}%)</span>
          </VChip>
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
          <VCard v-for="it in items" :key="it.response_id" variant="outlined" class="photo-card" :class="{ 'photo-card--tagged': it.tag }">
            <div class="photo-box" :class="{ 'cursor-pointer': it.photo_path }" @click="openPhoto(it)">
              <img v-if="it.photo_path" :src="photoUrl(it.photo_path)" alt="" loading="lazy" />
              <div v-else class="text-body-small text-disabled text-center px-2">{{ $t('page.noreadReview.noPhoto') }}</div>
            </div>
            <div class="px-2 pt-1 d-flex align-center justify-space-between">
              <span class="text-body-small text-medium-emphasis">{{ it.created_at.slice(11, 19) }}</span>
              <VChip v-if="it.tag" size="x-small" :color="tagColor(it.tag)" label>{{ $t(`page.noreadReview.tag.${it.tag}`) }}</VChip>
            </div>
            <div class="px-2 pb-2 pt-1 d-flex flex-wrap gap-1">
              <VBtn v-for="code in NOREAD_TAGS" :key="code" size="x-small" :color="tagColor(code)" :variant="it.tag === code ? 'flat' : 'tonal'" :loading="busyId === it.response_id" class="tag-btn" @click="setTag(it, code)">{{ $t(`page.noreadReview.tag.${code}`) }}</VBtn>
            </div>
          </VCard>
        </div>
      </VCardText>
      <VDivider />
      <TablePagination v-model:page="page" v-model:per-page="pageSize" :total="total" :page-sizes="[12, 24, 48, 96]" />
    </VCard>

    <VDialog v-model="viewer.open" max-width="900">
      <div style="position: relative;">
        <VBtn icon variant="elevated" size="x-small" style="position: absolute; top: -12px; right: -12px; z-index: 10;" @click="viewer.open = false"><VIcon icon="tabler-x" size="14" /></VBtn>
        <VCard>
          <VCardItem><VCardTitle>{{ viewer.title }}</VCardTitle></VCardItem>
          <VDivider />
          <VCardText class="text-center">
            <img v-if="viewer.url && !viewerError" :src="viewer.url" class="viewer-img" alt="" @error="viewerError = true" />
            <div v-else class="py-8 text-medium-emphasis">{{ $t('page.noreadReview.noPhoto') }}</div>
          </VCardText>
        </VCard>
      </div>
    </VDialog>
  </div>
</template>

<style scoped>
.photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
.photo-card--tagged { border-color: rgba(var(--v-theme-primary), 0.5); }
.photo-box { aspect-ratio: 4 / 3; background: rgba(var(--v-theme-on-surface), 0.04); display: flex; align-items: center; justify-content: center; overflow: hidden; }
.photo-box img { inline-size: 100%; block-size: 100%; object-fit: cover; }
.tag-btn { padding-inline: 6px !important; min-inline-size: 0 !important; }
.viewer-img { max-inline-size: 100%; max-block-size: 70vh; }
</style>
