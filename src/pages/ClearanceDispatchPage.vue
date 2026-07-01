<script setup>
import { ref, reactive, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import { cloudClearanceOptions, cloudClearanceDispatch } from '@/api/tauri'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { useStickyValue } from '@/composables/useStickyValue'
import AppHeader from '@/components/AppHeader.vue'
import AppBulkInput from '@/components/AppBulkInput.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'

const { t } = useI18n()

const todayStr = () => new Date().toISOString().slice(0, 10)

// 司機 / 倉庫 本地記憶:切頁或重啟後沿用上次選擇,不需重選(倉庫兩頁共用同一實體倉)
const driverName = useStickyValue('clearance.dispatch.driver', '')
const storageCode = useStickyValue('clearance.storage', '33843')

// 日期不記憶:每次開頁預設今天才正確
const form = reactive({
  shipping_date: todayStr(),
})

const packageSnList = ref([])
const bulkRef = ref(null)
const submitting = ref(false)

// 倉庫退回固定兩處,司機由雲端帶回歷史清單(下拉可選,亦可自由輸入新值;送出後寫回雲端)
const options = reactive({ storages: [{ value: '33843', title: '桃園' }, { value: '41466', title: '台中' }], drivers: [] })

const focusScanner = async () => {
  await nextTick()
  bulkRef.value?.focusInput?.()
}

const loadOptions = async () => {
  try {
    const data = await cloudClearanceOptions()
    if (Array.isArray(data?.storages) && data.storages.length) options.storages = data.storages
    options.drivers = Array.isArray(data?.drivers) ? data.drivers.map(o => o.value ?? o) : []
  } catch (e) {
    // 選項抓取失敗不阻塞操作:倉庫退回預設、司機仍可自由輸入
    toast(t('page.clearanceDispatch.optionsError', { msg: errorMessageFromException(e) }), { type: 'warning' })
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
    toast(t('page.clearanceDispatch.errNoPackage'), { type: 'warning' })
    return
  }
  if (!driverName.value || !String(driverName.value).trim()) {
    toast(t('page.clearanceDispatch.errNoDriver'), { type: 'warning' })
    return
  }

  submitting.value = true
  try {
    const res = await cloudClearanceDispatch({
      transportPackageSn: packageSnList.value.join(','),
      driverName: String(driverName.value).trim(),
      shippingDate: form.shipping_date,
      storageCode: storageCode.value,
    })
    if (res?.success === false) {
      toast(res.message || t('page.clearanceDispatch.submitError', { msg: '' }), { type: 'error' })
      return
    }
    toast(res?.message || t('page.clearanceDispatch.submitOk', { n: res?.dispatched ?? packageSnList.value.length }), { type: 'success' })
    clearScans()
  } catch (e) {
    toast(t('page.clearanceDispatch.submitError', { msg: errorMessageFromException(e) }), { type: 'error' })
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.clearanceDispatch.title')" :subtitle="$t('page.clearanceDispatch.subtitle')" icon="tabler-truck-delivery" />

    <VRow>
      <VCol cols="12" md="4">
        <VCard>
          <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
            <VIcon size="22" icon="tabler-clipboard-text" class="me-2" />
            <span>{{ $t('page.clearanceDispatch.settingsTitle') }}</span>
          </VCardTitle>
          <VCardText class="pa-5">
            <div class="text-body-2 text-medium-emphasis mb-1">{{ $t('page.clearanceDispatch.shippingDate') }}</div>
            <div class="mb-4">
              <AppDatePicker v-model="form.shipping_date" />
            </div>

            <div class="text-body-2 text-medium-emphasis mb-1">{{ $t('page.clearanceDispatch.driverName') }}</div>
            <VCombobox
              v-model="driverName"
              :items="options.drivers"
              :placeholder="$t('page.clearanceDispatch.driverNamePlaceholder')"
              variant="outlined"
              density="compact"
              clearable
              hide-details
              class="mb-4"
            />

            <div class="text-body-2 font-weight-medium mb-2">{{ $t('page.clearanceDispatch.storageCode') }}</div>
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
              color="info"
              block
              :loading="submitting"
              :disabled="packageSnList.length === 0"
              @click="handleSubmit"
            >
              <VIcon icon="tabler-truck-delivery" size="18" class="me-1" />
              {{ $t('page.clearanceDispatch.submitBtn') }}{{ packageSnList.length > 0 ? ` (${packageSnList.length})` : '' }}
            </VBtn>
          </VCardText>
        </VCard>
      </VCol>

      <VCol cols="12" md="8">
        <VCard>
          <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
            <VIcon size="22" icon="tabler-scan" class="me-2" />
            <span>{{ $t('page.clearanceDispatch.scanTitle') }}</span>
            <VSpacer />
            <VBtn v-if="packageSnList.length > 0" size="small" variant="tonal" color="error" @click="clearScans">
              <VIcon icon="tabler-trash" size="16" class="me-1" />{{ $t('common.clearAll') }}
            </VBtn>
          </VCardTitle>
          <VCardText class="pa-5">
            <AppBulkInput
              ref="bulkRef"
              v-model="packageSnList"
              :label="$t('page.clearanceDispatch.scanLabel')"
              :placeholder="$t('page.clearanceDispatch.scanPlaceholder')"
            />
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>
