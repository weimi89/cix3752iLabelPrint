<script setup>
import {
  sortChannelList,
  sortChannelSave,
  sortChannelSetEnabled,
  sortChannelUnassignedGet,
  sortChannelUnassignedSave,
  dispatchProviderList,
  getConfig,
  updateConfig,
} from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import PersonnelCombobox from '@/components/PersonnelCombobox.vue'
import { useStickerHistory } from '@/composables/useStickerHistory'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { listen } from '@tauri-apps/api/event'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const channels = ref([]) // 後端回來的 8 筆,position L1..R4
const dispatchOptions = ref([])
// 人員歷史名單(與掃描/自動列印頁共用同一份)
const { history: stickerHistory, reload: reloadStickerHistory, add: addStickerHistory, remove: removeSticker } = useStickerHistory()
const dirty = ref(new Set()) // 紀錄哪些 position 被改過
const originalCodes = ref(new Map()) // load 時各 position 的原始 channel_code,供「代碼有變更才驗證」的 grandfather
const loading = ref(false)
const savingAll = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

// 未設定指派物流的 fallback 通道代碼
const unassignedCode = ref('')
const unassignedDialog = ref(false)
const unassignedDraft = ref('')

// 純分揀模式總開關(存於 AppConfig.sort_only.enabled;獨立於面單路徑模式)。
// 放這頁而非「面單路徑」卡片,因純分揀是分揀行為而非面單呈現拓撲。
const sortOnly = ref(false)
const savingSortOnly = ref(false)
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
    const [list, dispatch, uCode, , cfg] = await Promise.all([
      sortChannelList(),
      dispatchProviderList(),
      sortChannelUnassignedGet(),
      reloadStickerHistory(),
      getConfig(),
    ])
    channels.value = list
    originalCodes.value = new Map(list.map(c => [c.position, (c.channel_code || '').trim()]))
    dispatchOptions.value = dispatch
    unassignedCode.value = uCode ?? ''
    sortOnly.value = !!cfg?.sort_only?.enabled
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

// 純分揀模式切換:重抓一次 config 再寫回(避免蓋掉其他頁的併發變更),熱套用即時生效。
const toggleSortOnly = async val => {
  savingSortOnly.value = true
  errorMsg.value = ''
  try {
    const cfg = await getConfig()
    cfg.sort_only = { ...(cfg.sort_only || {}), enabled: val }
    await updateConfig(cfg)
    sortOnly.value = val
    toast.success(t(val ? 'page.sort.sortOnly.enabled' : 'page.sort.sortOnly.disabled'))
  } catch (e) {
    sortOnly.value = !val // 失敗回復開關狀態
    errorMsg.value = String(e?.message || e)
  } finally {
    savingSortOnly.value = false
  }
}
// 手機遙控(同區網手機開 /control 暫停通道)→ 後端廣播 sort-channel-updated,桌面即時同步開關
let unlistenChannel = null
onMounted(async () => {
  await load()
  try {
    unlistenChannel = await listen('sort-channel-updated', evt => {
      const { position, enabled } = evt.payload || {}
      const ch = position && findChannel(position)
      if (ch && typeof enabled === 'boolean') ch.enabled = enabled
    })
  } catch (e) {
    console.warn('listen sort-channel-updated 失敗', e)
  }
})
onUnmounted(() => {
  if (unlistenChannel) { unlistenChannel(); unlistenChannel = null }
})

const markDirty = pos => dirty.value.add(pos)

const flash = msg => {
  flashMsg.value = msg
  setTimeout(() => (flashMsg.value = ''), 3000)
}

const saveAll = async () => {
  if (!dirty.value.size) return
  savingAll.value = true
  errorMsg.value = ''
  const positions = Array.from(dirty.value)
  const errors = []
  let savedCount = 0
  // 逐筆獨立儲存:單一筆失敗(格式/衝突/網路)不中斷其他有效編輯。
  for (const pos of positions) {
    const ch = findChannel(pos)
    if (!ch) { dirty.value.delete(pos); continue }
    const code = (ch.channel_code || '').trim()
    // 通道代碼是分揀機格口機器碼(工控機讀它路由),限英數與 - _、長度 ≤ 16。
    // 只在代碼「有變更」時前端預檢(對齊後端 grandfather:既有不合規舊值不擋,仍可改其他欄位)。
    if (code && code !== (originalCodes.value.get(pos) || '') && !/^[A-Za-z0-9_-]{1,16}$/.test(code)) {
      errors.push(t('page.sort.channelCodeInvalid', { code }))
      continue // 保留 dirty 供修正,不影響其他筆
    }
    try {
      await sortChannelSave({
        position: ch.position,
        channelCode: ch.channel_code,
        dispatchCodes: ch.dispatch_codes,
        jobSticker: ch.job_sticker,
      })
      dirty.value.delete(pos)
      savedCount++
    } catch (e) {
      errors.push(String(e?.message || e))
    }
  }
  if (savedCount > 0) flash(t('page.sort.savedFlash', { n: savedCount }))
  if (errors.length) {
    // 有失敗:顯示錯誤且不 reload,保留未存的編輯供使用者修正後重存
    errorMsg.value = errors.join('\n')
  } else {
    await load() // 全部成功才重拉,讓 channel_code 衝突等狀態同步
  }
  savingAll.value = false
}

// 分揀進行中的快速暫停 / 啟用(即時生效,獨立於整列儲存)
const pausing = ref(new Set())
const togglePause = async (pos, val) => {
  const ch = findChannel(pos)
  if (!ch) return
  const prev = ch.enabled
  ch.enabled = val // 樂觀更新,失敗再還原
  pausing.value.add(pos)
  try {
    await sortChannelSetEnabled(pos, val)
    toast(
      t(val ? 'page.sort.pause.resumedFlash' : 'page.sort.pause.pausedFlash', { pos: POSITION_LABELS.value[pos] }),
      { type: val ? 'success' : 'warning' },
    )
  } catch (e) {
    ch.enabled = prev
    toast(`${t('page.sort.pause.failed')}: ${String(e?.message || e)}`, { type: 'error' })
  } finally {
    pausing.value.delete(pos)
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

  // 暫停中:整張卡淡化 + 斜紋底,一眼看出此通道暫不分揀(欄位本身仍可編輯)
  &--paused {
    background: repeating-linear-gradient(
      135deg,
      rgb(var(--v-theme-warning) / 0.05),
      rgb(var(--v-theme-warning) / 0.05) 8px,
      rgb(var(--v-theme-warning) / 0.1) 8px,
      rgb(var(--v-theme-warning) / 0.1) 16px
    );

    .channel-card__title {
      opacity: 0.6;
    }
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

.pause-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
  padding: 2px 4px 2px 8px;
  border-radius: 6px;
  border: 1px solid rgb(var(--v-theme-on-surface) / 0.08);

  &__text {
    font-size: 0.8rem;
    font-weight: 600;
  }

  &__switch {
    margin-left: auto;
    flex: 0 0 auto;
  }

  &--on {
    .pause-row__text { color: rgb(var(--v-theme-success)); }
  }

  &--off {
    border-color: rgb(var(--v-theme-warning) / 0.5);
    background: rgb(var(--v-theme-warning) / 0.08);

    .pause-row__text { color: rgb(var(--v-theme-warning)); }
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

    <!-- 純分揀模式總開關(只回分揀通道、不出面單、不記印單) -->
    <VCard variant="outlined" class="mb-4" :color="sortOnly ? 'primary' : undefined">
      <div class="d-flex align-center ga-3 px-4 py-3">
        <VIcon
          :icon="sortOnly ? 'tabler-arrows-split-2' : 'tabler-printer'"
          size="18"
          :color="sortOnly ? 'primary' : 'medium-emphasis'"
          class="flex-shrink-0"
        />
        <div class="flex-grow-1">
          <div class="text-body-2 font-weight-medium">{{ $t('label.settings.sortOnly') }}</div>
          <div class="text-caption text-medium-emphasis">{{ $t('label.settings.sortOnlyHint') }}</div>
        </div>
        <VSwitch
          :model-value="sortOnly"
          :loading="savingSortOnly"
          color="primary"
          inset
          hide-details
          density="compact"
          class="flex-shrink-0"
          @update:model-value="toggleSortOnly"
        />
      </div>
    </VCard>

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
            :class="{
              'channel-card--dirty': dirty.has(pos),
              'channel-card--paused': findChannel(pos) && findChannel(pos).channel_code && !findChannel(pos).enabled,
            }"
          >
            <template v-if="findChannel(pos)">
              <div class="channel-card__head">
                <VIcon class="channel-card__icon" icon="tabler-arrow-narrow-left" size="18" color="primary" />
                <span class="channel-card__title">{{ POSITION_LABELS[pos] }}</span>
                <VChip v-if="dirty.has(pos)" size="x-small" color="warning" class="channel-card__chip">{{ $t('page.sort.status.unsaved') }}</VChip>
                <VChip v-else-if="findChannel(pos).channel_code && !findChannel(pos).enabled" size="x-small" color="warning" variant="flat" class="channel-card__chip">{{ $t('page.sort.status.paused') }}</VChip>
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
                  v-model="findChannel(pos).dispatch_codes"
                  :items="dispatchSelectItems"
                  :placeholder="$t('page.sort.dispatchUnassigned')"
                  density="compact"
                  variant="outlined"
                  multiple
                  chips
                  closable-chips
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
              <!-- 快速暫停開關:分揀進行中即時生效,僅已設定通道代碼者顯示 -->
              <div
                v-if="findChannel(pos).channel_code"
                class="pause-row"
                :class="findChannel(pos).enabled ? 'pause-row--on' : 'pause-row--off'"
              >
                <VIcon
                  :icon="findChannel(pos).enabled ? 'tabler-player-play-filled' : 'tabler-player-pause-filled'"
                  size="16"
                  :color="findChannel(pos).enabled ? 'success' : 'warning'"
                />
                <span class="pause-row__text">
                  {{ findChannel(pos).enabled ? $t('page.sort.status.enabled') : $t('page.sort.status.paused') }}
                </span>
                <VSwitch
                  :model-value="findChannel(pos).enabled"
                  color="success"
                  density="compact"
                  hide-details
                  inset
                  :loading="pausing.has(pos)"
                  class="pause-row__switch"
                  @update:model-value="togglePause(pos, $event)"
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
            :class="{
              'channel-card--dirty': dirty.has(pos),
              'channel-card--paused': findChannel(pos) && findChannel(pos).channel_code && !findChannel(pos).enabled,
            }"
          >
            <template v-if="findChannel(pos)">
              <div class="channel-card__head">
                <span class="channel-card__title">{{ POSITION_LABELS[pos] }}</span>
                <VChip v-if="dirty.has(pos)" size="x-small" color="warning" class="channel-card__chip" style="margin-left: auto;">{{ $t('page.sort.status.unsaved') }}</VChip>
                <VChip v-else-if="findChannel(pos).channel_code && !findChannel(pos).enabled" size="x-small" color="warning" variant="flat" class="channel-card__chip" style="margin-left: auto;">{{ $t('page.sort.status.paused') }}</VChip>
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
                  v-model="findChannel(pos).dispatch_codes"
                  :items="dispatchSelectItems"
                  :placeholder="$t('page.sort.dispatchUnassigned')"
                  density="compact"
                  variant="outlined"
                  multiple
                  chips
                  closable-chips
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
              <!-- 快速暫停開關:分揀進行中即時生效,僅已設定通道代碼者顯示 -->
              <div
                v-if="findChannel(pos).channel_code"
                class="pause-row"
                :class="findChannel(pos).enabled ? 'pause-row--on' : 'pause-row--off'"
              >
                <VIcon
                  :icon="findChannel(pos).enabled ? 'tabler-player-play-filled' : 'tabler-player-pause-filled'"
                  size="16"
                  :color="findChannel(pos).enabled ? 'success' : 'warning'"
                />
                <span class="pause-row__text">
                  {{ findChannel(pos).enabled ? $t('page.sort.status.enabled') : $t('page.sort.status.paused') }}
                </span>
                <VSwitch
                  :model-value="findChannel(pos).enabled"
                  color="success"
                  density="compact"
                  hide-details
                  inset
                  :loading="pausing.has(pos)"
                  class="pause-row__switch"
                  @update:model-value="togglePause(pos, $event)"
                />
              </div>
            </template>
          </div>
        </div>
      </VCol>
    </VRow>
  </div>
</template>
