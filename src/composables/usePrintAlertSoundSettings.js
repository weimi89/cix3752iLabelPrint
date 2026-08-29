// 列印/查件提示音(全域 effect_1~4)設定的共用接線 — AutoPrintPage 與 CloudSettingsPage 共用。
// 兩頁設定的是同一組全域對照表(單一 localStorage 來源),改一處全域生效;
// 事件清單、storage key、套用函式集中在此,兩頁不得各寫一份(改一處漏另一處會讓兩頁設定不一致)。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSoundSettings } from '@/composables/useSoundSettings'
import { setPrintAlertSound, PRINT_ALERT_SOUND_STORAGE_KEY, PRINT_ALERT_SOUND_DEFAULTS } from '@/composables/useSoundEffects'

export function usePrintAlertSoundSettings() {
  const { t } = useI18n()

  const events = computed(() => [
    { key: 'effect_1', label: t('soundSettings.events.printSuccess') },
    { key: 'effect_2', label: t('soundSettings.events.generalError') },
    { key: 'effect_3', label: t('soundSettings.events.storeClosed') },
    { key: 'effect_4', label: t('soundSettings.events.unconfirmed') },
  ])

  const { soundSettings, isSoundSettingsDialogVisible, handleSoundSettingsSave } = useSoundSettings(
    PRINT_ALERT_SOUND_STORAGE_KEY,
    PRINT_ALERT_SOUND_DEFAULTS,
    setPrintAlertSound,
  )

  return {
    events,
    defaults: PRINT_ALERT_SOUND_DEFAULTS,
    soundSettings,
    isSoundSettingsDialogVisible,
    handleSoundSettingsSave,
  }
}
