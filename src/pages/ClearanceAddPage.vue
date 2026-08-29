<script setup>
import { ref, reactive, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { cloudClearanceOptions, cloudClearanceStore } from '@/api/tauri'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { useStickyValue } from '@/composables/useStickyValue'
import AppHeader from '@/components/AppHeader.vue'
import AppBulkInput from '@/components/AppBulkInput.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { localTodayStr } from '@/utils/localDate'

const { t } = useI18n()

const todayStr = () => localTodayStr()

// 清關公司 / 倉庫 本地記憶:切頁或重啟後沿用上次選擇,不需重選(倉庫兩頁共用同一實體倉)
const clearanceCompany = useStickyValue('clearance.add.company', '')
const storageCode = useStickyValue('clearance.storage', '33843')

// 日期不記憶:每次開頁預設今天才正確
const form = reactive({
  clearance_date: todayStr(),
})

const packageSnList = ref([])
const bulkRef = ref(null)
const submitting = ref(false)

// 倉庫退回固定兩處,清關公司由雲端帶回歷史清單(下拉可選,亦可自由輸入新值;送出後寫回雲端)
const options = reactive({ storages: [{ value: '33843', title: '桃園' }, { value: '41466', title: '台中' }], clearance_companies: [] })

const focusScanner = async () => {
  await nextTick()
  bulkRef.value?.focusInput?.()
}

const loadOptions = async () => {
  try {
    const data = await cloudClearanceOptions()
    if (Array.isArray(data?.storages) && data.storages.length) options.storages = data.storages
    options.clearance_companies = Array.isArray(data?.clearance_companies) ? data.clearance_companies.map(o => o.value ?? o) : []
  } catch (e) {
    // 選項抓取失敗不阻塞操作:倉庫退回預設、公司仍可自由輸入
    toast(t('page.clearanceAdd.optionsError', { msg: errorMessageFromException(e) }), { type: 'warning' })
  }
}

onMounted(async () => {
  await loadOptions()
  focusScanner()
})

const clearScans = () => {
  packageSnList.value = []
  focusScanner()
}

const handleSubmit = async () => {
  if (packageSnList.value.length === 0) {
    toast(t('page.clearanceAdd.errNoPackage'), { type: 'warning' })
    return
  }
  if (!clearanceCompany.value || !String(clearanceCompany.value).trim()) {
    toast(t('page.clearanceAdd.errNoCompany'), { type: 'warning' })
    return
  }

  submitting.value = true
  try {
    const res = await cloudClearanceStore({
      transportPackageSn: packageSnList.value.join(','),
      clearanceCompany: String(clearanceCompany.value).trim(),
      clearanceDate: form.clearance_date,
      storageCode: storageCode.value,
    })
    if (res?.success === false) {
      toast(res.message || t('page.clearanceAdd.submitError', { msg: '' }), { type: 'error' })
      return
    }
    toast(res?.message || t('page.clearanceAdd.submitOk', { n: res?.upserted ?? packageSnList.value.length }), { type: 'success' })
    clearScans()
  } catch (e) {
    toast(t('page.clearanceAdd.submitError', { msg: errorMessageFromException(e) }), { type: 'error' })
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.clearanceAdd.title')" :subtitle="$t('page.clearanceAdd.subtitle')" icon="tabler-scan" />

    <VRow>
      <VCol cols="12" md="4">
        <VCard>
          <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
            <VIcon size="22" icon="tabler-clipboard-text" class="me-2" />
            <span>{{ $t('page.clearanceAdd.settingsTitle') }}</span>
          </VCardTitle>
          <VCardText class="pa-5">
            <div class="text-body-medium text-medium-emphasis mb-1">{{ $t('page.clearanceAdd.clearanceDate') }}</div>
            <div class="mb-4">
              <AppDatePicker v-model="form.clearance_date" />
            </div>

            <div class="text-body-medium text-medium-emphasis mb-1">{{ $t('page.clearanceAdd.clearanceCompany') }}</div>
            <VCombobox
              v-model="clearanceCompany"
              :items="options.clearance_companies"
              :placeholder="$t('page.clearanceAdd.clearanceCompanyPlaceholder')"
              variant="outlined"
              density="compact"
              clearable
              hide-details
              class="mb-4"
            />

            <div class="text-body-medium font-weight-medium mb-2">{{ $t('page.clearanceAdd.storageCode') }}</div>
            <VRadioGroup v-model="storageCode" inline hide-details>
              <VRadio
                v-for="s in options.storages"
                :key="s.value"
                :label="s.title"
                :value="s.value"
              />
            </VRadioGroup>

            <VDivider class="my-4" />

            <VBtn
              color="primary"
              block
              :loading="submitting"
              :disabled="packageSnList.length === 0"
              @click="handleSubmit"
            >
              <VIcon icon="tabler-device-floppy" size="18" class="me-1" />
              {{ $t('page.clearanceAdd.submitBtn') }}{{ packageSnList.length > 0 ? ` (${packageSnList.length})` : '' }}
            </VBtn>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" md="8">
        <VCard>
          <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
            <VIcon size="22" icon="tabler-scan" class="me-2" />
            <span>{{ $t('page.clearanceAdd.scanTitle') }}</span>
            <VSpacer />
            <VBtn v-if="packageSnList.length > 0" size="small" variant="tonal" color="error" @click="clearScans">
              <VIcon icon="tabler-trash" size="16" class="me-1" />{{ $t('common.clearAll') }}
            </VBtn>
          </VCardTitle>
          <VCardText class="pa-5">
            <AppBulkInput
              ref="bulkRef"
              v-model="packageSnList"
              :label="$t('page.clearanceAdd.scanLabel')"
              :placeholder="$t('page.clearanceAdd.scanPlaceholder')"
            />
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>
