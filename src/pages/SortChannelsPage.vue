<script setup>
import {
  sortChannelList,
  sortChannelSave,
  sortChannelUnassignedGet,
  sortChannelUnassignedSave,
  dispatchProviderList,
} from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import PersonnelCombobox from '@/components/PersonnelCombobox.vue'
import { useStickerHistory } from '@/composables/useStickerHistory'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const channels = ref([]) // 後端回來的 8 筆,position L1..R4
const dispatchOptions = ref([])
// 人員歷史名單(與掃描/自動列印頁共用同一份)
const { history: stickerHistory, reload: reloadStickerHistory, add: addStickerHistory, remove: removeSticker } = useStickerHistory()
const dirty = ref(new Set()) // 紀錄哪些 position 被改過
const loading = ref(false)
const savingAll = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

// 未設定指派物流的 fallback 通道代碼
const unassignedCode = ref('')
const unassignedDialog = ref(false)
const unassignedDraft = ref('')
const savingUnassigned = ref(false)

const openUnassignedDialog = () => {
  unassignedDraft.value = unassignedCode.value
  unassignedDialog.value = true
}

const POSITION_LABELS = computed(() => ({
  L1: t('page.sort.pos.L1'), L2: t('page.sort.pos.L2'), L3: t('page.sort.pos.L3'), L4: t('page.sort.pos.L4'),
  R1: t('page.sort.pos.R1'), R2: t('page.sort.pos.R2'), R3: t('page.sort.pos.R3'), R4: t('page.sort.pos.R4'),
}))
const LEFT_POSITIONS = ['L1', 'L2', 'L3', 'L4']
const RIGHT_POSITIONS = ['R1', 'R2', 'R3', 'R4']

const findChannel = pos => channels.value.find(c => c.position === pos)

const dispatchSelectItems = computed(() =>
  dispatchOptions.value.map(d => ({ title: `${d.name} (${d.code})`, value: d.code })),
)

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    const [list, dispatch, uCode] = await Promise.all([
      sortChannelList(),
      dispatchProviderList(),
      sortChannelUnassignedGet(),
      reloadStickerHistory(),
    ])
    channels.value = list
    dispatchOptions.value = dispatch
    unassignedCode.value = uCode ?? ''
    dirty.value = new Set()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

const saveUnassigned = async () => {
  savingUnassigned.value = true
  try {
    await sortChannelUnassignedSave(unassignedDraft.value || null)
    unassignedCode.value = unassignedDraft.value
    unassignedDialog.value = false
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    savingUnassigned.value = false
  }
}
onMounted(load)

const markDirty = pos => dirty.value.add(pos)

const flash = msg => {
  flashMsg.value = msg
  setTimeout(() => (flashMsg.value = ''), 3000)
}

const saveAll = async () => {
  if (!dirty.value.size) return
  savingAll.value = true
  errorMsg.value = ''
  try {
    const positions = Array.from(dirty.value)
    for (const pos of positions) {
      const ch = findChannel(pos)
      if (!ch) continue
      await sortChannelSave({
        position: ch.position,
        channelCode: ch.channel_code,
        dispatchCode: ch.dispatch_code,
        jobSticker: ch.job_sticker,
      })
      dirty.value.delete(pos)
    }
    flash(t('page.sort.savedFlash', { n: positions.length }))
    await load() // 重新拉,讓 channel_code 衝突等狀態同步
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    savingAll.value = false
  }
}

const removeStickerFromHistory = async name => {
  try {
    await removeSticker(name)
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}
// 輸入貼標人員當下即記入共用歷史(不必等整列儲存)
const rememberUser = name => addStickerHistory(name).catch(e => console.warn('記住人員失敗', e))
</script>

<style scoped lang="scss">
.channel-card {
  position: relative;
  border: 1px solid rgb(var(--v-theme-on-surface) / 0.08);
  border-radius: 8px;
  padding: 10px 12px 12px;
  background: rgb(var(--v-theme-surface));
  box-shadow: 0 2px 6px rgb(0 0 0 / 0.06), 0 1px 2px rgb(0 0 0 / 0.04);
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s, transform 0.15s;

  &:hover {
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.08), 0 2px 4px rgb(0 0 0 / 0.05);
  }

  &--left {
    border-left: 3px solid rgb(var(--v-theme-primary));
  }

  &--right {
    border-right: 3px solid rgb(var(--v-theme-warning));
  }

  &--dirty {
    border-color: rgb(var(--v-theme-warning));
    box-shadow: 0 0 0 1px rgb(var(--v-theme-warning) / 0.25);
  }

  &__head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  &__icon {
    flex-shrink: 0;
  }

  &__title {
    font-weight: 600;
    font-size: 0.95rem;
    line-height: 1;
  }

  &__chip {
    margin-left: auto;
    font-size: 0.7rem;
  }

  // 縮小欄位間距（每個欄位現在外包 .search-field）
  :deep(.search-field) {
    margin-bottom: 8px;

    &:last-child {
      margin-bottom: 0;
    }

    label {
      font-size: 12px;
    }
  }
}

.unassigned-setting-row {
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;

  &--unset {
    border-color: rgb(var(--v-theme-warning) / 0.5);
    border-left: 3px solid rgb(var(--v-theme-warning));

    &:hover {
      box-shadow: 0 0 0 1px rgb(var(--v-theme-warning) / 0.3);
    }
  }

  &--set {
    border-color: rgb(var(--v-theme-primary) / 0.4);
    border-left: 3px solid rgb(var(--v-theme-primary));

    &:hover {
      box-shadow: 0 0 0 1px rgb(var(--v-theme-primary) / 0.2);
    }
  }
}

.column-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
  letter-spacing: 0.05em;
  color: rgb(var(--v-theme-on-surface) / 0.65);
  margin-bottom: 8px;
  font-size: 0.85rem;
}
</style>

<template>
  <div>
    <AppHeader :title="$t('page.sort.title')" :subtitle="$t('page.sort.subtitle')" icon="tabler-route">
      <template #actions>
        <div class="d-flex ga-2">
          <VBtn variant="outlined" :loading="loading" @click="load">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
          </VBtn>
          <VBtn
            color="primary"
            :loading="savingAll"
            :disabled="!dirty.size"
            @click="saveAll"
          >
            <VIcon icon="tabler-device-floppy" size="16" class="me-1" />
            {{ $t('page.sort.saveAll', { n: dirty.size }) }}
          </VBtn>
        </div>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      {{ $t('page.sort.browserAlert') }}
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <!-- 未設定指派物流 fallback 設定入口 -->
    <VCard
      variant="outlined"
      class="mb-4 unassigned-setting-row"
      :class="unassignedCode ? 'unassigned-setting-row--set' : 'unassigned-setting-row--unset'"
      @click="openUnassignedDialog"
    >
      <div class="d-flex align-center ps-4 ga-3" style="min-height: 52px;">
        <VIcon
          :icon="unassignedCode ? 'tabler-check' : 'tabler-alert-triangle'"
          size="18"
          :color="unassignedCode ? 'primary' : 'warning'"
          class="flex-shrink-0"
        />
        <span class="text-body-2 flex-grow-1">未指派物流通道 — 預設回傳代碼</span>
        <VBtn
          :color="unassignedCode ? 'primary' : 'warning'"
          :variant="unassignedCode ? 'flat' : 'tonal'"
          size="large"
          class="px-5 font-weight-bold rounded-s-0"
          style="align-self: stretch; height: auto;"
        >
          <VIcon :icon="unassignedCode ? 'tabler-pencil' : 'tabler-settings'" size="15" class="me-2" />
          <template v-if="unassignedCode">{{ $t('page.sort.unassigned.current', { code: unassignedCode }) }}</template>
          <template v-else>{{ $t('page.sort.status.unset') }}</template>
        </VBtn>
      </div>
    </VCard>

    <!-- 未設定指派物流 fallback 設定 Dialog -->
    <VDialog v-model="unassignedDialog" max-width="420" persistent>
      <div style="position: relative;">
        <VBtn
          icon
          variant="elevated"
          size="x-small"
          style="position: absolute; top: -12px; right: -12px; z-index: 10;"
          @click="unassignedDialog = false"
        >
          <VIcon icon="tabler-x" size="14" />
        </VBtn>
      <VCard>
        <VCardTitle class="d-flex align-center ga-2 pt-4 px-5">
          <VIcon icon="tabler-question-mark" size="18" color="secondary" />
          {{ $t('page.sort.unassigned.label') }}
        </VCardTitle>
        <VCardText class="px-5 pb-2">
          <div class="text-caption text-medium-emphasis mb-4">{{ $t('page.sort.unassigned.hint') }}</div>
          <VTextField
            v-model="unassignedDraft"
            :label="$t('page.sort.unassigned.placeholder')"
            density="compact"
            variant="outlined"
            autofocus
            clearable
            @keyup.enter="saveUnassigned"
          />
        </VCardText>
        <VCardActions class="px-5 pb-4">
          <VSpacer />
          <VBtn variant="text" @click="unassignedDialog = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="secondary" variant="elevated" :loading="savingUnassigned" @click="saveUnassigned">
            {{ $t('page.sort.unassigned.save') }}
          </VBtn>
        </VCardActions>
      </VCard>
      </div>
    </VDialog>

    <VRow>
      <!-- 左側通道 -->
      <VCol cols="12" md="6">
        <div class="column-label">
          <VIcon icon="tabler-arrow-narrow-left" size="18" color="primary" />
          <span>{{ $t('page.sort.leftColumn') }}</span>
        </div>
        <div class="d-flex flex-column ga-4">
          <div
            v-for="pos in LEFT_POSITIONS"
            :key="pos"
            class="channel-card channel-card--left"
            :class="{ 'channel-card--dirty': dirty.has(pos) }"
          >
            <template v-if="findChannel(pos)">
              <div class="channel-card__head">
                <VIcon class="channel-card__icon" icon="tabler-arrow-narrow-left" size="18" color="primary" />
                <span class="channel-card__title">{{ POSITION_LABELS[pos] }}</span>
                <VChip v-if="dirty.has(pos)" size="x-small" color="warning" class="channel-card__chip">{{ $t('page.sort.status.unsaved') }}</VChip>
                <VChip v-else-if="findChannel(pos).channel_code" size="x-small" color="success" variant="tonal" class="channel-card__chip">{{ $t('page.sort.status.enabled') }}</VChip>
                <VChip v-else size="x-small" color="default" variant="tonal" class="channel-card__chip">{{ $t('page.sort.status.unset') }}</VChip>
              </div>

              <div class="search-field">
                <label>{{ $t('page.sort.channelCode') }}</label>
                <VTextField
                  v-model="findChannel(pos).channel_code"
                  :placeholder="$t('page.sort.channelCodeExampleL')"
                  density="compact"
                  variant="outlined"
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>{{ $t('page.sort.dispatch') }}</label>
                <VSelect
                  v-model="findChannel(pos).dispatch_code"
                  :items="dispatchSelectItems"
                  :placeholder="$t('page.sort.dispatchUnassigned')"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>{{ $t('page.sort.sticker') }}</label>
                <PersonnelCombobox
                  v-model="findChannel(pos).job_sticker"
                  :items="stickerHistory"
                  :placeholder="$t('page.sort.stickerPlaceholder')"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                  @remember="rememberUser"
                  @remove="removeStickerFromHistory"
                />
              </div>
            </template>
          </div>
        </div>
      </VCol>

      <!-- 右側通道 -->
      <VCol cols="12" md="6">
        <div class="column-label" style="justify-content: flex-end;">
          <span>{{ $t('page.sort.rightColumn') }}</span>
          <VIcon icon="tabler-arrow-narrow-right" size="18" color="warning" />
        </div>
        <div class="d-flex flex-column ga-4">
          <div
            v-for="pos in RIGHT_POSITIONS"
            :key="pos"
            class="channel-card channel-card--right"
            :class="{ 'channel-card--dirty': dirty.has(pos) }"
          >
            <template v-if="findChannel(pos)">
              <div class="channel-card__head">
                <span class="channel-card__title">{{ POSITION_LABELS[pos] }}</span>
                <VChip v-if="dirty.has(pos)" size="x-small" color="warning" class="channel-card__chip" style="margin-left: auto;">{{ $t('page.sort.status.unsaved') }}</VChip>
                <VChip v-else-if="findChannel(pos).channel_code" size="x-small" color="success" variant="tonal" class="channel-card__chip" style="margin-left: auto;">{{ $t('page.sort.status.enabled') }}</VChip>
                <VChip v-else size="x-small" color="default" variant="tonal" class="channel-card__chip" style="margin-left: auto;">{{ $t('page.sort.status.unset') }}</VChip>
                <VIcon class="channel-card__icon" icon="tabler-arrow-narrow-right" size="18" color="warning" />
              </div>

              <div class="search-field">
                <label>{{ $t('page.sort.channelCode') }}</label>
                <VTextField
                  v-model="findChannel(pos).channel_code"
                  :placeholder="$t('page.sort.channelCodeExampleR')"
                  density="compact"
                  variant="outlined"
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>{{ $t('page.sort.dispatch') }}</label>
                <VSelect
                  v-model="findChannel(pos).dispatch_code"
                  :items="dispatchSelectItems"
                  :placeholder="$t('page.sort.dispatchUnassigned')"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>{{ $t('page.sort.sticker') }}</label>
                <PersonnelCombobox
                  v-model="findChannel(pos).job_sticker"
                  :items="stickerHistory"
                  :placeholder="$t('page.sort.stickerPlaceholder')"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                  @remember="rememberUser"
                  @remove="removeStickerFromHistory"
                />
              </div>
            </template>
          </div>
        </div>
      </VCol>
    </VRow>
  </div>
</template>
