<script setup>
// 格口配置(左右各幾格):設定好就很少再動,放在服務設定頁,不佔分揀通道頁的版面。
// 存檔後後端廣播 sort-layout-updated,開著的分揀通道頁會自己重新載入
import { sortChannelList, sortLayoutGet, sortLayoutSave } from '@/api/tauri'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const LAYOUT_MAX = 10
const layout = ref({ left: 0, right: 0 })
const draft = ref({ left: 5, right: 5 })
const channels = ref([])
const loading = ref(false)
const saving = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const posNumber = pos => Number(String(pos ?? '').slice(1)) || 0
const posLabel = pos => t(String(pos ?? '')[0] === 'R' ? 'page.sort.posRight' : 'page.sort.posLeft', { n: posNumber(pos) })

const draftNum = k => Number(draft.value[k])
const valid = computed(() => ['left', 'right'].every(k => Number.isInteger(draftNum(k)) && draftNum(k) >= 1 && draftNum(k) <= LAYOUT_MAX))
const changed = computed(() => draftNum('left') !== layout.value.left || draftNum('right') !== layout.value.right)

// 縮減會移除的位置;其中已設通道代碼的要特別點名,分揀機那邊也得跟著調
const removals = computed(() => channels.value
  .filter(c => posNumber(c.position) > (draftNum(c.position[0] === 'L' ? 'left' : 'right') || 0))
  .sort((a, b) => a.position.localeCompare(b.position, undefined, { numeric: true })))
const removalsWithCode = computed(() => removals.value.filter(c => (c.channel_code || '').trim()))

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const [lay, list] = await Promise.all([sortLayoutGet(), sortChannelList()])
    layout.value = { left: Number(lay?.left) || 0, right: Number(lay?.right) || 0 }
    draft.value = { ...layout.value }
    channels.value = list || []
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  } finally {
    loading.value = false
  }
}

const save = async () => {
  if (!valid.value || !changed.value) return
  saving.value = true
  errorMsg.value = ''
  flashMsg.value = ''
  try {
    const saved = await sortLayoutSave({ left: draftNum('left'), right: draftNum('right') })
    flashMsg.value = t('page.sort.layout.saved', { l: saved.left, r: saved.right })
    await load()
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<template>
  <VCard class="mb-4">
    <VCardItem>
      <VCardTitle class="text-body-large font-weight-medium">{{ $t('page.sort.layout.title') }}</VCardTitle>
      <VCardSubtitle class="text-body-small text-wrap">{{ $t('page.sort.layout.hint') }}</VCardSubtitle>
    </VCardItem>
    <VCardText>
      <div class="layout-row">
        <VNumberInput
          v-model="draft.left"
          :min="1"
          :max="LAYOUT_MAX"
          :step="1"
          :label="$t('page.sort.layout.left')"
          :disabled="loading"
          control-variant="stacked"
          density="compact"
          hide-details
        />
        <VNumberInput
          v-model="draft.right"
          :min="1"
          :max="LAYOUT_MAX"
          :step="1"
          :label="$t('page.sort.layout.right')"
          :disabled="loading"
          control-variant="stacked"
          density="compact"
          hide-details
        />
        <VBtn
          :color="removals.length ? 'warning' : 'primary'"
          :loading="saving"
          :disabled="loading || !valid || !changed"
          @click="save"
        >
          <VIcon icon="tabler-adjustments" size="16" class="me-1" />
          {{ $t('page.sort.layout.save') }}
        </VBtn>
      </div>
      <VAlert v-if="removals.length" type="warning" variant="tonal" density="compact" class="mt-3">
        <div>{{ $t('page.sort.layout.removeWarn', { list: removals.map(c => posLabel(c.position)).join('、') }) }}</div>
        <div v-if="removalsWithCode.length" class="mt-1 font-weight-medium">
          {{ $t('page.sort.layout.removeWarnCode', { list: removalsWithCode.map(c => `${posLabel(c.position)} (${c.channel_code})`).join('、') }) }}
        </div>
      </VAlert>
      <VAlert v-if="errorMsg" type="error" variant="tonal" density="compact" class="mt-3">{{ errorMsg }}</VAlert>
      <VAlert v-if="flashMsg" type="success" variant="tonal" density="compact" class="mt-3">{{ flashMsg }}</VAlert>
    </VCardText>
  </VCard>
</template>

<style scoped>
.layout-row {
  display: grid;
  grid-template-columns: minmax(0, 180px) minmax(0, 180px) auto;
  gap: 12px;
  align-items: center;
  justify-content: start;
}

@media (max-width: 639.98px) {
  .layout-row {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }

  .layout-row > .v-btn {
    grid-column: 1 / -1;
  }
}
</style>
