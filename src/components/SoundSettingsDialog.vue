<script setup>
// 提示音設定對話框 — 對齊 cix3752iWeb 掃描頁 SoundSettingsDialog。
// 每個音效事件一列 VSelect + 試聽鈕;「儲存」才回寫父層(useSoundSettings),取消不影響現值。
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { SOUND_LIBRARY } from '@/composables/soundLibrary'

const props = defineProps({
  isDialogVisible: {
    type: Boolean,
    required: true,
  },

  // 音效事件清單,[{ key, label }],key 對應音效對照表的 key,label 由父層翻譯
  soundEvents: {
    type: Array,
    required: true,
  },

  // 目前生效的設定,{ key: 音檔路徑 }
  settings: {
    type: Object,
    required: true,
  },

  // 頁面預設值,{ key: 音檔路徑 },供「恢復預設」使用
  defaults: {
    type: Object,
    required: true,
  },
})

const emit = defineEmits(['update:isDialogVisible', 'save'])

const { t } = useI18n()

const soundOptions = computed(() => SOUND_LIBRARY.map(([key, file]) => ({
  title: t(`soundSettings.library.${key}`),
  value: file,
})))

// 對話框內編輯用的暫存設定,儲存才回寫父層
const localSettings = ref({})

// 試聽為原始音量;實際作業播放音量依各頁面音效管線(如放大倍率)為準
let previewAudio = null
const stopPreview = () => {
  if (previewAudio) {
    previewAudio.pause()
    previewAudio = null
  }
}

watch(() => props.isDialogVisible, visible => {
  if (visible) localSettings.value = { ...props.settings }
  // 關閉對話框即停止試聽:元件常駐掛載於頁面,VDialog 關閉不觸發 onBeforeUnmount,
  // 若不在此停,較長的試聽片段會在對話框消失後繼續播完。涵蓋 X / 取消 / 儲存 / 點外部所有關閉路徑。
  else stopPreview()
})

const previewSound = path => {
  stopPreview()
  if (!path) return

  previewAudio = new Audio(path)
  previewAudio.play().catch(() => {})
}

onBeforeUnmount(stopPreview)

const resetDefaults = () => {
  localSettings.value = { ...props.defaults }
}

const closeDialog = () => {
  emit('update:isDialogVisible', false)
}

const saveSettings = () => {
  emit('save', { ...localSettings.value })
  closeDialog()
}
</script>

<template>
  <VDialog
    :model-value="isDialogVisible"
    max-width="560"
    @update:model-value="closeDialog"
  >
    <div style="position: relative;">
      <VBtn
        icon
        variant="elevated"
        size="x-small"
        style="position: absolute; top: -12px; right: -12px; z-index: 10;"
        @click="closeDialog"
      >
        <VIcon icon="tabler-x" size="14" />
      </VBtn>
      <VCard>
        <div class="d-flex align-center px-4" style="min-block-size: 54px;">
          <VIcon icon="tabler-volume" size="24" class="me-2" />
          <span class="text-title-large">{{ $t('soundSettings.title') }}</span>
        </div>
        <VDivider />
        <VCardText>
          <div class="d-flex flex-column ga-4">
            <div
              v-for="event in soundEvents"
              :key="event.key"
            >
              <label class="text-body-medium font-weight-medium d-block mb-1">{{ event.label }}</label>
              <div class="d-flex align-center ga-2">
                <VSelect
                  v-model="localSettings[event.key]"
                  :items="soundOptions"
                  density="compact"
                  hide-details
                />
                <VBtn
                  icon
                  variant="text"
                  size="small"
                  color="primary"
                  @click="previewSound(localSettings[event.key])"
                >
                  <VIcon icon="tabler-player-play" size="18" />
                  <VTooltip
                    activator="parent"
                    location="bottom"
                  >
                    {{ $t('soundSettings.preview') }}
                  </VTooltip>
                </VBtn>
              </div>
            </div>
            <div class="text-body-small text-disabled">
              {{ $t('soundSettings.hint') }}
            </div>
            <div class="d-flex align-center">
              <VBtn
                color="secondary"
                variant="tonal"
                @click="resetDefaults"
              >
                {{ $t('soundSettings.resetDefaults') }}
              </VBtn>
              <VSpacer />
              <VBtn
                color="secondary"
                variant="tonal"
                class="me-2"
                @click="closeDialog"
              >
                {{ $t('common.cancel') }}
              </VBtn>
              <VBtn
                color="primary"
                variant="flat"
                @click="saveSettings"
              >
                {{ $t('common.save') }}
              </VBtn>
            </div>
          </div>
        </VCardText>
      </VCard>
    </div>
  </VDialog>
</template>
