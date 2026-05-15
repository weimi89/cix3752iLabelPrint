<script setup>
import {
  sortChannelList,
  sortChannelSave,
  dispatchProviderList,
  stickerHistoryList,
  stickerHistoryDelete,
} from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const channels = ref([]) // 後端回來的 8 筆,position L1..R4
const dispatchOptions = ref([])
const stickerHistory = ref([])
const dirty = ref(new Set()) // 紀錄哪些 position 被改過
const loading = ref(false)
const savingAll = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

const POSITION_LABELS = {
  L1: '左 1', L2: '左 2', L3: '左 3', L4: '左 4',
  R1: '右 1', R2: '右 2', R3: '右 3', R4: '右 4',
}
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
    const [list, dispatch, sticker] = await Promise.all([
      sortChannelList(),
      dispatchProviderList(),
      stickerHistoryList(),
    ])
    channels.value = list
    dispatchOptions.value = dispatch
    stickerHistory.value = sticker
    dirty.value = new Set()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
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
    flash(`已批次儲存 ${positions.length} 筆`)
    await load() // 重新拉,讓 channel_code 衝突等狀態同步
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    savingAll.value = false
  }
}

const removeStickerFromHistory = async name => {
  try {
    await stickerHistoryDelete(name)
    stickerHistory.value = stickerHistory.value.filter(n => n !== name)
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}
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
    <AppHeader title="分揀通道" subtitle="左 4 通道 / 右 4 通道，各自設定代碼、物流與貼標人員" icon="tabler-route">
      <template #actions>
        <div class="d-flex ga-2">
          <VBtn variant="outlined" :loading="loading" @click="load">
            <VIcon icon="tabler-refresh" size="16" class="me-1" />重新載入
          </VBtn>
          <VBtn
            color="primary"
            :loading="savingAll"
            :disabled="!dirty.size"
            @click="saveAll"
          >
            <VIcon icon="tabler-device-floppy" size="16" class="me-1" />
            儲存全部變更 ({{ dirty.size }})
          </VBtn>
        </div>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      瀏覽器預覽模式 — 顯示的是示範資料,實機請於桌面 App 內開啟。
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <VRow>
      <!-- 左側通道 -->
      <VCol cols="12" md="6">
        <div class="column-label">
          <VIcon icon="tabler-arrow-narrow-left" size="18" color="primary" />
          <span>左側 (L)</span>
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
                <VChip v-if="dirty.has(pos)" size="x-small" color="warning" class="channel-card__chip">未儲存</VChip>
                <VChip v-else-if="findChannel(pos).channel_code" size="x-small" color="success" variant="tonal" class="channel-card__chip">已啟用</VChip>
                <VChip v-else size="x-small" color="default" variant="tonal" class="channel-card__chip">未設定</VChip>
              </div>

              <div class="search-field">
                <label>通道代碼</label>
                <VTextField
                  v-model="findChannel(pos).channel_code"
                  placeholder="例: A01"
                  density="compact"
                  variant="outlined"
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>指派物流</label>
                <VSelect
                  v-model="findChannel(pos).dispatch_code"
                  :items="dispatchSelectItems"
                  placeholder="(未指派)"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>貼標人員</label>
                <VCombobox
                  v-model="findChannel(pos).job_sticker"
                  :items="stickerHistory"
                  placeholder="輸入或從歷史選取"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                >
                  <template #item="{ item, props: itemProps }">
                    <VListItem v-bind="itemProps" :title="item.raw">
                      <template #append>
                        <VBtn
                          icon="tabler-x"
                          size="x-small"
                          variant="text"
                          @click.stop="removeStickerFromHistory(item.raw)"
                        />
                      </template>
                    </VListItem>
                  </template>
                </VCombobox>
              </div>
            </template>
          </div>
        </div>
      </VCol>

      <!-- 右側通道 -->
      <VCol cols="12" md="6">
        <div class="column-label" style="justify-content: flex-end;">
          <span>右側 (R)</span>
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
                <VChip v-if="dirty.has(pos)" size="x-small" color="warning" class="channel-card__chip" style="margin-left: auto;">未儲存</VChip>
                <VChip v-else-if="findChannel(pos).channel_code" size="x-small" color="success" variant="tonal" class="channel-card__chip" style="margin-left: auto;">已啟用</VChip>
                <VChip v-else size="x-small" color="default" variant="tonal" class="channel-card__chip" style="margin-left: auto;">未設定</VChip>
                <VIcon class="channel-card__icon" icon="tabler-arrow-narrow-right" size="18" color="warning" />
              </div>

              <div class="search-field">
                <label>通道代碼</label>
                <VTextField
                  v-model="findChannel(pos).channel_code"
                  placeholder="例: B01"
                  density="compact"
                  variant="outlined"
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>指派物流</label>
                <VSelect
                  v-model="findChannel(pos).dispatch_code"
                  :items="dispatchSelectItems"
                  placeholder="(未指派)"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                />
              </div>
              <div class="search-field">
                <label>貼標人員</label>
                <VCombobox
                  v-model="findChannel(pos).job_sticker"
                  :items="stickerHistory"
                  placeholder="輸入或從歷史選取"
                  density="compact"
                  variant="outlined"
                  clearable
                  hide-details
                  @update:model-value="markDirty(pos)"
                >
                  <template #item="{ item, props: itemProps }">
                    <VListItem v-bind="itemProps" :title="item.raw">
                      <template #append>
                        <VBtn
                          icon="tabler-x"
                          size="x-small"
                          variant="text"
                          @click.stop="removeStickerFromHistory(item.raw)"
                        />
                      </template>
                    </VListItem>
                  </template>
                </VCombobox>
              </div>
            </template>
          </div>
        </div>
      </VCol>
    </VRow>
  </div>
</template>
