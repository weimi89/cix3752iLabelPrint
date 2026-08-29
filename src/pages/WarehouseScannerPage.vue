<script setup>
// 入倉驗單 — 掃描包裹入倉。業務邏輯全在雲端 WarehouseScannerService,本頁透過
// api/tauri.js → Tauri command → 雲端 api_v1 取結果;箱標走本地印表機列印。
// 操作流程與雲端 cix3752iWeb 入倉頁一致(兩端行為需同步);音效同樣走 useAmplifiedSound 放大 +
// 入箱成功播預錄箱號人聲 box-{serial}.mp3,缺檔退回 useSpeech 雙語 TTS。
import { ref, reactive, computed, watch, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue3-toastify'
import {
  warehouseOptions, warehouseCreatePackage, warehouseExamine,
  warehouseRemoveGoods, warehouseRemovePackage, warehouseLabelData,
  warehousePrintLabels, listPrinters,
} from '@/api/tauri'
import { errorMessageFromException } from '@/composables/useLabelStatus'
import { speak, speechLangOf } from '@/composables/useSpeech'
import { useAmplifiedSound } from '@/composables/useAmplifiedSound'
import { useSoundSettings } from '@/composables/useSoundSettings'
import AppHeader from '@/components/AppHeader.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import SoundSettingsDialog from '@/components/SoundSettingsDialog.vue'
import { localTodayStr } from '@/utils/localDate'

const { t, locale } = useI18n()

const SCANNER_USER_KEY = 'cix3752iLabelPrint.scannerUser'
const PRINTER_KEY = 'cix3752iLabelPrint.warehousePrinter'

const todayStr = () => localTodayStr()

// 選項(倉庫 / 物流商)由雲端帶回
const options = reactive({ warehouses: {}, providers: [] })

// 表單資料
const formData = reactive({
  storage_warehouse: '41466',
  return_provider: '7',
  return_date: todayStr(),
  serial_number: 1,
  return_remarks: '',
  return_bulky: false,
  scanner_user: localStorage.getItem(SCANNER_USER_KEY) || '',
  shipment_no: '',
  segment_barcode: '',
  continuous: false,
  serial_number_end: 1,
})
watch(() => formData.scanner_user, v => localStorage.setItem(SCANNER_USER_KEY, v || ''))

// 狀態
const currentPackageSn = ref('')
const currentStorageWarehouse = ref('')
// 已建箱身分:建箱/載箱成功時記下當時的(倉庫/物流商/日期/箱號)。examine 以此 tuple 比對
// 目前表單,判斷「箱號是否已建立且未被改動」——**不可自行重算雲端 package_sn 字串格式**,
// 避免與雲端 WarehouseScannerService 的組法耦合(補零/日期切法一有差異就全擋、功能失效)。
const builtBox = ref(null)
const currentBoxTuple = () => ({
  warehouse: formData.storage_warehouse,
  provider: formData.return_provider,
  date: formData.return_date,
  serial: formData.serial_number,
})
const goodsList = ref([])
const goodsTotal = ref(0)
const enableSegment = ref(false)
const isLoading = ref(false)

// 印表機(箱標本地列印)
const printers = ref([])
const selectedPrinter = ref(localStorage.getItem(PRINTER_KEY) || '')
watch(selectedPrinter, v => localStorage.setItem(PRINTER_KEY, v || ''))

// 輸入框參考
const shipmentNoInput = ref(null)
const segmentBarcodeInput = ref(null)

// 音效:忠實對齊雲端入倉頁 — Web Audio 放大(倉庫吵)+ 入箱成功播「預錄箱號人聲」。
// beep 用中介端內建音效,可自訂(SoundSettingsDialog,選擇存 localStorage 各台獨立記憶);
// success 帶箱號時改播 box-{serial}.mp3(缺檔 > 500 才退回 TTS),不受自訂影響。
const WAREHOUSE_SOUND_DEFAULTS = {
  success: '/sounds/effect-10.mp3', // 僅在無箱號時作 beep(實務上入箱恆有箱號 → 播人聲)
  error: '/sounds/effect-05.mp3',
  warning: '/sounds/effect-08.mp3', // 重複入箱提示
  private: '/sounds/effect-09.mp3',
  privateSupplier: '/sounds/effect-07.mp3',
}

// 不開放自訂 success:入箱成功恆帶箱號 → playSound('success', serial) 一律走箱號人聲 box-{serial}.mp3
// 分支,success beep 永不播放。若列為可自訂事件,操作員改了卻毫無效果(設定與行為不一致),故不列入。
const WAREHOUSE_SOUND_EVENTS = computed(() => [
  { key: 'error', label: t('soundSettings.events.warehouseError') },
  { key: 'warning', label: t('soundSettings.events.warehouseRepeat') },
  { key: 'private', label: t('soundSettings.events.warehousePrivate') },
  { key: 'privateSupplier', label: t('soundSettings.events.warehousePrivateSupplier') },
])

const { soundSettings, isSoundSettingsDialogVisible, handleSoundSettingsSave } = useSoundSettings(
  'cix3752iLabelPrint.warehouseScannerSounds',
  WAREHOUSE_SOUND_DEFAULTS,
  (key, path) => setSound(key, path),
)

const { playSound, setSound } = useAmplifiedSound(
  { ...soundSettings.value },
  {
    // 序號 > 500 無預錄檔時退回雙語 TTS(依當前 locale;越南語需機器自備語音包)
    speakFallback: (n) => {
      try { speak([{ text: t('page.warehouseScanner.intakeBox', { n }), lang: speechLangOf(locale.value) }]) } catch { /* 忽略 */ }
    },
  },
)

onMounted(async () => {
  try {
    const o = await warehouseOptions()
    options.warehouses = o.warehouses || {}
    options.providers = o.providers || []
  } catch (e) {
    toast(t('page.warehouseScanner.optionsError', { msg: errorMessageFromException(e) }), { type: 'warning' })
  }
  try { printers.value = await listPrinters() } catch { /* 列印機列舉失敗不阻擋掃描 */ }
  if (!selectedPrinter.value && printers.value.length) selectedPrinter.value = printers.value[0].name
  nextTick(() => shipmentNoInput.value?.focus())
})

// 開始箱號變更時同步結束箱號
watch(() => formData.serial_number, (newVal) => {
  if (newVal >= formData.serial_number_end) formData.serial_number_end = newVal
})
watch(() => formData.serial_number_end, (newVal) => {
  if (newVal <= formData.serial_number) formData.serial_number = newVal
})

// 二段條碼開關
watch(enableSegment, (enabled) => {
  if (enabled) {
    nextTick(() => segmentBarcodeInput.value?.focus())
  } else {
    formData.segment_barcode = ''
    nextTick(() => shipmentNoInput.value?.focus())
  }
})

const handleShipmentNoKeydown = (e) => {
  if (e.key !== 'Enter') return
  e.preventDefault()
  if (!formData.shipment_no) return
  if (enableSegment.value && !formData.segment_barcode) {
    segmentBarcodeInput.value?.focus()
  } else {
    examinePackage()
  }
}

const handleSegmentBarcodeKeydown = (e) => {
  if (e.key !== 'Enter') return
  e.preventDefault()
  if (!formData.segment_barcode) return
  if (!formData.shipment_no) {
    shipmentNoInput.value?.focus()
  } else {
    examinePackage()
  }
}

// 共用日期 / 箱號驗證
const validateBase = () => {
  const errors = []
  if (!formData.return_date) errors.push(t('page.warehouseScanner.errDateRequired'))
  if (isNaN(formData.serial_number) || formData.serial_number < 1) errors.push(t('page.warehouseScanner.errSerialMin'))
  return errors
}

// 儲存建檔 / 列印貼標
const createPackage = async (printLabel = false) => {
  const errors = validateBase()
  if (errors.length) { toast(errors.join('、'), { type: 'error', timeout: 3000 }); return }

  isLoading.value = true
  try {
    const data = await warehouseCreatePackage({
      storage_warehouse: formData.storage_warehouse,
      return_provider: formData.return_provider,
      return_date: formData.return_date,
      serial_number: formData.serial_number,
      return_remarks: formData.return_remarks,
    })

    if (data.respond_code === 'ERROR-DATE-FORMAT') {
      toast(t('page.warehouseScanner.errDateFormat'), { type: 'error', timeout: 2000 })
      return
    }
    if (data.respond_code === 'FIND-PACKAGE-GOODS') {
      currentStorageWarehouse.value = data.storage_warehouse
      currentPackageSn.value = data.package_sn
      builtBox.value = currentBoxTuple()  // 記下已建箱身分,供 examine 比對
      goodsList.value = data.goods_list || []
      goodsTotal.value = data.goods_total || 0
      toast(t('page.warehouseScanner.boxLoaded'), { type: 'success', timeout: 2000 })

      if (printLabel) await printLabels()
    } else {
      // 未映射的 respond_code(新錯誤碼 / 雲端未部署對應版本)→ 不可靜默,顯示原始碼+訊息
      toast(data.respond_message || t('page.warehouseScanner.requestFailed') + `（${data.respond_code || '?'}）`, { type: 'error', timeout: 3000 })
    }
  } catch (e) {
    toast(errorMessageFromException(e) || t('page.warehouseScanner.requestFailed'), { type: 'error' })
  } finally {
    isLoading.value = false
  }
}

// 驗單入倉
const examinePackage = async () => {
  const errors = validateBase()
  if (!formData.scanner_user.trim()) errors.push(t('page.warehouseScanner.errScannerUserRequired'))

  // 檢查箱號是否已建立且表單未被改動:比對 builtBox 身分 tuple(不重算雲端 package_sn 格式)
  const b = builtBox.value
  const cur = currentBoxTuple()
  const boxReady = !!currentPackageSn.value && !!b &&
    b.warehouse === cur.warehouse && b.provider === cur.provider &&
    b.date === cur.date && b.serial === cur.serial
  if (!boxReady) errors.push(t('page.warehouseScanner.errBoxNotSaved'))

  if (errors.length) { toast(errors.join('、'), { type: 'error', timeout: 3000 }); return }

  // beforeSend 連刷:先快照送出值,立即清空入倉條碼並聚焦,讓掃描槍在網路往返期間就能連刷下一筆。
  const payload = { ...formData }
  formData.shipment_no = ''
  formData.segment_barcode = ''
  nextTick(() => shipmentNoInput.value?.focus())

  try {
    const data = await warehouseExamine(payload)
    if (data.return_remarks) formData.return_remarks = data.return_remarks

    switch (data.respond_code) {
      case 'ERROR-DATE-FORMAT':
        playSound('error'); toast(t('page.warehouseScanner.errDateFormat'), { type: 'error', timeout: 2000 }); break
      case 'ERROR-WAREHOUSE':
        playSound('error'); toast(t('page.warehouseScanner.errWarehouse'), { type: 'error', timeout: 2000 }); break
      case 'NO-GOODS-DATA':
        playSound('error'); toast(t('page.warehouseScanner.errNoGoods'), { type: 'error', timeout: 2000 }); break
      case 'PRIVATE-GOODS':
        playSound('private'); toast(t('page.warehouseScanner.privateGoods'), { type: 'warning', timeout: 2000 }); break
      case 'PRIVATE-SUPPLIER':
        playSound('privateSupplier'); toast(t('page.warehouseScanner.privateSupplier'), { type: 'warning', timeout: 2000 }); break
      case 'REPEAT-PRIVATE':
        playSound('warning'); toast(t('page.warehouseScanner.repeat', { sn: data.package_sn || '' }), { type: 'warning', timeout: 2000 }); break
      case 'FIND-PACKAGE-GOODS':
        // 入箱成功:播預錄箱號人聲「入第N箱」。用**送出時的快照** payload.serial_number,
        // 不用即時 formData.serial_number(連刷時可能已被下一次操作改動 → 報錯箱號)。
        playSound('success', payload.serial_number)
        toast(t('page.warehouseScanner.intakeOk'), { type: 'success', timeout: 2000 })
        currentStorageWarehouse.value = data.storage_warehouse
        currentPackageSn.value = data.package_sn
        builtBox.value = {
          warehouse: payload.storage_warehouse, provider: payload.return_provider,
          date: payload.return_date, serial: payload.serial_number,
        }
        goodsList.value = data.goods_list || []
        goodsTotal.value = data.goods_total || 0
        break
      default:
        // 未映射的 respond_code(新碼 / 雲端結構變動)→ 播錯誤音 + 顯示原始碼+訊息,不可靜默漏件
        playSound('error')
        toast(data.respond_message || t('page.warehouseScanner.requestFailed') + `（${data.respond_code || '?'}）`, { type: 'error', timeout: 3000 })
    }
  } catch (e) {
    playSound('error')
    toast(errorMessageFromException(e) || t('page.warehouseScanner.requestFailed'), { type: 'error' })
  }
}

// 移除單一商品
const removeGoods = async (shipmentNo) => {
  try {
    const data = await warehouseRemoveGoods(shipmentNo)
    if (data.respond_code === 'FIND-PACKAGE-GOODS') {
      goodsList.value = data.goods_list || []
      goodsTotal.value = data.goods_total || 0
      toast(t('page.warehouseScanner.goodsRemoved'), { type: 'success', timeout: 2000 })
    } else {
      // 未映射的 respond_code → 顯示原始碼+訊息,不靜默(避免以為移除成功)
      toast(data.respond_message || t('page.warehouseScanner.removeFailed') + `（${data.respond_code || '?'}）`, { type: 'error', timeout: 3000 })
    }
  } catch (e) {
    toast(errorMessageFromException(e) || t('page.warehouseScanner.removeFailed'), { type: 'error' })
  }
}

// 刪除整個箱號
const removePackage = async () => {
  if (!currentPackageSn.value) return
  try {
    const data = await warehouseRemovePackage(currentStorageWarehouse.value, currentPackageSn.value)
    // 雲端明確回錯誤碼時不可靜默當成刪除成功:顯示原始碼+訊息且保留箱號狀態
    const code = data?.respond_code
    if (code && /ERROR|FAIL|NOT/i.test(code)) {
      toast(data?.respond_message || t('page.warehouseScanner.removeFailed') + `（${code}）`, { type: 'error', timeout: 3000 })
      return
    }
    currentPackageSn.value = ''
    currentStorageWarehouse.value = ''
    builtBox.value = null
    goodsList.value = []
    goodsTotal.value = 0
    toast(t('page.warehouseScanner.boxRemoved'), { type: 'success', timeout: 2000 })
  } catch (e) {
    toast(errorMessageFromException(e) || t('page.warehouseScanner.removeFailed'), { type: 'error' })
  }
}

// 箱標本地列印(取雲端箱標資料 → 渲染 PNG → 本地印表機)
const printLabels = async () => {
  if (!currentPackageSn.value) { toast(t('page.warehouseScanner.errNoBox'), { type: 'warning' }); return }
  if (!selectedPrinter.value) { toast(t('page.warehouseScanner.errNoPrinter'), { type: 'error' }); return }
  try {
    const r = await warehouseLabelData(
      currentStorageWarehouse.value,
      currentPackageSn.value,
      formData.continuous ? formData.serial_number_end : 0,
      formData.continuous,
    )
    if (r.error) { toast(r.error, { type: 'error' }); return }
    const n = await warehousePrintLabels(selectedPrinter.value, r.labels || [])
    toast(t('page.warehouseScanner.printOk', { n }), { type: 'success', timeout: 2000 })
  } catch (e) {
    toast(errorMessageFromException(e) || t('page.warehouseScanner.printFailed'), { type: 'error' })
  }
}
</script>

<template>
  <div>
    <AppHeader
      :title="t('page.warehouseScanner.title')"
      :subtitle="t('page.warehouseScanner.subtitle')"
      icon="tabler-package-import"
    >
      <template #actions>
        <VBtn
          variant="text"
          color="default"
          @click="isSoundSettingsDialogVisible = true"
        >
          <VIcon size="18" icon="tabler-volume" class="me-1" />
          {{ t('soundSettings.title') }}
        </VBtn>
      </template>
    </AppHeader>

    <!-- 提示音設定(入倉 beep;箱號人聲不受影響) -->
    <SoundSettingsDialog
      v-model:is-dialog-visible="isSoundSettingsDialogVisible"
      :sound-events="WAREHOUSE_SOUND_EVENTS"
      :settings="soundSettings"
      :defaults="WAREHOUSE_SOUND_DEFAULTS"
      @save="handleSoundSettingsSave"
    />

    <VRow>
      <!-- 左側設定面板 -->
      <VCol cols="12" lg="4">
        <VCard class="mb-3">
          <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
            <VIcon size="22" icon="tabler-box" class="me-2" />
            <span>{{ t('page.warehouseScanner.panelCreate') }}</span>
          </VCardTitle>
          <VCardText>
            <!-- 倉庫選擇 -->
            <div class="my-4">
              <label class="text-body-large font-weight-medium d-block mb-2">{{ t('page.warehouseScanner.warehouse') }}</label>
              <VRadioGroup v-model="formData.storage_warehouse" inline hide-details>
                <VRadio
                  v-for="(name, value) in options.warehouses"
                  :key="value"
                  :label="name"
                  :value="value"
                />
              </VRadioGroup>
            </div>

            <!-- 物流商選擇 -->
            <div class="mb-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.provider') }}</label>
              <VSelect
                v-model="formData.return_provider"
                :items="options.providers"
                item-title="title"
                item-value="value"
                density="compact"
                hide-details
              />
            </div>

            <!-- 日期 -->
            <div class="mb-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.date') }}</label>
              <AppDatePicker v-model="formData.return_date" density="compact" />
            </div>

            <!-- 開始箱號 -->
            <div class="mb-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.startSerial') }}</label>
              <VNumberInput
                v-model="formData.serial_number"
                :min="1"
                :max="999"
                control-variant="stacked"
                density="compact"
                hide-details
              />
            </div>

            <!-- 備註 -->
            <div class="mb-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.remarks') }}</label>
              <VTextField
                v-model="formData.return_remarks"
                autocomplete="off"
                density="compact"
                hide-details
              />
            </div>

            <!-- 大件勾選 -->
            <VCheckbox v-model="formData.return_bulky" :label="t('page.warehouseScanner.bulky')" hide-details />
          </VCardText>

          <VDivider />

          <VCardText class="d-flex gap-3 justify-space-between">
            <VBtn color="success" :loading="isLoading" @click="createPackage(true)">
              <VIcon icon="tabler-printer" class="me-1" />
              {{ t('page.warehouseScanner.printLabel') }}
            </VBtn>
            <VBtn color="primary" :loading="isLoading" @click="createPackage(false)">
              <VIcon icon="tabler-device-floppy" class="me-1" />
              {{ t('page.warehouseScanner.save') }}
            </VBtn>
          </VCardText>

          <VDivider />

          <VCardText>
            <!-- 結束箱號(連續列印)-->
            <VCheckbox v-model="formData.continuous" :label="t('page.warehouseScanner.endSerial')" hide-details class="mb-2" />
            <VNumberInput
              v-model="formData.serial_number_end"
              :min="1"
              :max="999"
              :disabled="!formData.continuous"
              control-variant="stacked"
              density="compact"
              hide-details
            />

            <!-- 箱標印表機 -->
            <div class="mt-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.printer') }}</label>
              <VSelect
                v-model="selectedPrinter"
                :items="printers"
                item-title="name"
                item-value="name"
                density="compact"
                hide-details
              />
            </div>
          </VCardText>
        </VCard>
      </VCol>

      <!-- 右側操作面板 -->
      <VCol cols="12" lg="8">
        <VCard class="mb-3">
          <VCardTitle class="d-flex align-center px-4 py-3 bg-grey-300">
            <VIcon size="22" icon="tabler-scan" class="me-2" />
            <span>{{ t('page.warehouseScanner.panelScan') }}</span>
            <VSpacer />
            <span class="text-body-medium me-2">{{ t('page.warehouseScanner.segmentBarcode') }}</span>
            <VSwitch v-model="enableSegment" hide-details density="compact" class="flex-grow-0" style="transform: scale(0.75); transform-origin: right center;" />
          </VCardTitle>
          <VCardText>
            <!-- 操作人員 -->
            <div class="my-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.scannerUser') }}</label>
              <VTextField
                v-model="formData.scanner_user"
                autocomplete="off"
                density="compact"
                hide-details
              />
            </div>

            <!-- 包裹條碼 -->
            <div class="mb-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.shipmentNo') }}</label>
              <VTextField
                ref="shipmentNoInput"
                v-model="formData.shipment_no"
                autocomplete="off"
                density="compact"
                hide-details
                @keydown="handleShipmentNoKeydown"
              />
            </div>

            <!-- 二段條碼(啟用開關在本卡標題右上) -->
            <div class="mb-4">
              <label class="text-body-medium d-block mb-1">{{ t('page.warehouseScanner.segmentBarcode') }}</label>
              <VTextField
                ref="segmentBarcodeInput"
                v-model="formData.segment_barcode"
                :disabled="!enableSegment"
                autocomplete="off"
                density="compact"
                hide-details
                @keydown="handleSegmentBarcodeKeydown"
              />
            </div>
          </VCardText>
        </VCard>

        <!-- 已入條碼表格 -->
        <VCard>
          <VCardTitle class="bg-success text-white py-3 d-flex align-center justify-space-between">
            <div>
              {{ t('page.warehouseScanner.scannedCount') }} <span class="text-error font-weight-bold">{{ goodsTotal }}</span>
            </div>
            <VBtn v-if="currentPackageSn" color="error" size="small" @click="removePackage">
              {{ t('page.warehouseScanner.removeBox') }}
            </VBtn>
          </VCardTitle>
          <VCardText class="pa-0">
            <VTable hover>
              <thead>
                <tr>
                  <th class="text-center" style="width: 60px;"></th>
                  <th class="text-center" style="width: 180px;">{{ t('page.warehouseScanner.sellerName') }}</th>
                  <th class="text-center">{{ t('page.warehouseScanner.shipmentNo') }}</th>
                  <th class="text-center d-none d-md-table-cell" style="width: 180px;">{{ t('page.warehouseScanner.intakeTime') }}</th>
                  <th class="text-center" style="width: 120px;">{{ t('page.warehouseScanner.scannerUser') }}</th>
                </tr>
              </thead>
              <tbody>
                <template v-if="goodsList.length > 0">
                  <tr v-for="item in goodsList" :key="item.log_id">
                    <td class="text-center">
                      <VBtn icon variant="text" color="error" size="small" @click="removeGoods(item.shipment_no)">
                        <VIcon icon="tabler-trash" size="20" />
                      </VBtn>
                    </td>
                    <td class="text-center">{{ item.suppliers_seller }}</td>
                    <td class="text-center">{{ item.shipment_no }}</td>
                    <td class="text-center d-none d-md-table-cell">{{ item.log_time }}</td>
                    <td class="text-center">{{ item.scanner_user }}</td>
                  </tr>
                </template>
                <template v-else>
                  <tr>
                    <td colspan="5">
                      <div class="py-4 d-flex align-center justify-center text-medium-emphasis">
                        <VIcon icon="tabler-alert-circle" size="20" class="me-1" />
                        <span>{{ t('page.warehouseScanner.noData') }}</span>
                      </div>
                    </td>
                  </tr>
                </template>
              </tbody>
            </VTable>
          </VCardText>
        </VCard>
      </VCol>
    </VRow>
  </div>
</template>
