// 可自訂提示音的設定管理 composable — 對齊 cix3752iWeb 掃描頁 useSoundSettings 設計。
// 提示音選擇存 localStorage(各台機器獨立記憶),搭配 SoundSettingsDialog 讓操作員自選音效,
// 儲存後透過 applySound 即時生效,不需重新整理。
import { ref } from 'vue'
import { toast } from 'vue3-toastify'
import { useI18n } from 'vue-i18n'
import { KNOWN_SOUND_PATHS } from '@/composables/soundLibrary'

/**
 * 讀取已儲存的提示音設定;資料損壞或路徑異常時逐鍵退回預設值
 *
 * @param {string} storageKey localStorage key
 * @param {Object} defaultSounds 預設音效對照表,key 為音效名稱,value 為音檔路徑
 * @returns {Object} 合併後的音效對照表
 */
export function loadStoredSoundMap(storageKey, defaultSounds) {
  try {
    const stored = JSON.parse(localStorage.getItem(storageKey)) || {}
    const merged = {}

    for (const key of Object.keys(defaultSounds)) {
      const path = stored[key]

      // 只接受確實存在於內建音效庫的路徑;跨版本音檔被移除/更名或 localStorage 被污染時,
      // 格式正確但已不存在的路徑會退回預設,避免 new Audio(path).play() 靜默失敗 → 提示音無聲。
      merged[key] = (typeof path === 'string' && KNOWN_SOUND_PATHS.has(path))
        ? path
        : defaultSounds[key]
    }

    return merged
  } catch {
    return { ...defaultSounds }
  }
}

/**
 * @param {string} storageKey localStorage key,各用途唯一
 * @param {Object} defaultSounds 預設音效對照表,供「恢復預設」與損壞退回使用
 * @param {(key:string, path:string)=>void} applySound 套用單一音效變更(重載 buffer / 作廢快取)
 * @returns {{ soundSettings: import('vue').Ref, isSoundSettingsDialogVisible: import('vue').Ref, handleSoundSettingsSave: Function }}
 */
export function useSoundSettings(storageKey, defaultSounds, applySound) {
  const { t } = useI18n()
  const soundSettings = ref(loadStoredSoundMap(storageKey, defaultSounds))
  const isSoundSettingsDialogVisible = ref(false)

  // 儲存提示音設定:只套用有變更的音效
  const handleSoundSettingsSave = newSettings => {
    for (const [key, path] of Object.entries(newSettings)) {
      if (soundSettings.value[key] !== path) applySound(key, path)
    }

    soundSettings.value = newSettings

    try {
      localStorage.setItem(storageKey, JSON.stringify(newSettings))
    } catch {
      // localStorage 不可用時僅本次生效,不影響作業
    }

    toast(t('soundSettings.saved'), { type: 'success', timeout: 2000 })
  }

  return {
    soundSettings,
    isSoundSettingsDialogVisible,
    handleSoundSettingsSave,
  }
}
