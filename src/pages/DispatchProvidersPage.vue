<script setup>
import {
  dispatchProviderList,
  dispatchProviderUpsert,
  dispatchProviderDelete,
  getConfig,
} from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'
import { useI18n } from 'vue-i18n'
import { errorMessageFromException } from '@/composables/useLabelStatus'

const { t } = useI18n()
const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const items = ref([])
const loading = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')
const isDirectPrintMode = ref(false)

const dialogOpen = ref(false)
const editingOriginalCode = ref(null) // null 代表新增
const form = ref({ code: '', name: '', sort_order: 0, print_profile: '' })

const deleteOpen = ref(false)
const deleteTarget = ref(null)

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    items.value = await dispatchProviderList()
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  } finally {
    loading.value = false
  }
}

// direct_print 模式下面單不經雲端 print_profile 出單,故該欄非必填(本機印表機設定在「分揀通道」頁)
const loadMeta = async () => {
  if (!isTauriRuntime) return
  try {
    const cfg = await getConfig()
    isDirectPrintMode.value = cfg?.label_path?.mode === 'direct_print'
  } catch {}
}
onMounted(() => { load(); loadMeta() })

const openCreate = () => {
  editingOriginalCode.value = null
  form.value = { code: '', name: '', sort_order: items.value.length, print_profile: '' }
  dialogOpen.value = true
}

const openEdit = row => {
  editingOriginalCode.value = row.code
  form.value = { ...row, print_profile: row.print_profile ?? '' }
  dialogOpen.value = true
}

const flash = msg => {
  flashMsg.value = msg
  setTimeout(() => (flashMsg.value = ''), 3000)
}

const save = async () => {
  errorMsg.value = ''
  const code = (form.value.code || '').trim()
  const name = (form.value.name || '').trim()
  const printProfile = (form.value.print_profile || '').trim()
  if (!code || !name) {
    errorMsg.value = t('page.dispatch.errCodeNameRequired')
    return
  }
  if (!isDirectPrintMode.value && !printProfile) {
    errorMsg.value = t('page.dispatch.errPrintProfileRequired')
    return
  }
  if (editingOriginalCode.value === null
      && items.value.some(it => it.code === code)) {
    errorMsg.value = t('page.dispatch.errCodeDuplicate', { code })
    return
  }
  try {
    await dispatchProviderUpsert({
      code,
      name,
      sortOrder: Number(form.value.sort_order) || 0,
      printProfile: printProfile || null,
    })
    dialogOpen.value = false
    flash(t(editingOriginalCode.value === null ? 'page.dispatch.flashCreated' : 'page.dispatch.flashUpdated'))
    await load()
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  }
}

const askDelete = row => {
  deleteTarget.value = row
  deleteOpen.value = true
}

const confirmDelete = async () => {
  if (!deleteTarget.value) return
  errorMsg.value = ''
  try {
    await dispatchProviderDelete(deleteTarget.value.code)
    deleteOpen.value = false
    flash(t('page.dispatch.flashDeleted', { name: deleteTarget.value.name }))
    deleteTarget.value = null
    await load()
  } catch (e) {
    errorMsg.value = errorMessageFromException(e)
  }
}
</script>

<template>
  <div>
    <AppHeader :title="$t('page.dispatch.title')" :subtitle="$t('page.dispatch.subtitle')" icon="tabler-truck-delivery">
      <template #actions>
        <VBtn color="primary" @click="openCreate">
          <VIcon icon="tabler-plus" size="16" class="me-1" />{{ $t('page.dispatch.create') }}
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      {{ $t('page.sort.browserAlert') }}
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <VCard>
      <VTable hover>
        <thead>
          <tr>
            <th class="text-center" style="width: 80px;">{{ $t('page.dispatch.col.order') }}</th>
            <th style="min-width: 100px;">{{ $t('page.dispatch.col.code') }}</th>
            <th style="min-width: 160px;">{{ $t('page.dispatch.col.name') }}</th>
            <th style="min-width: 180px;">{{ $t('page.dispatch.col.printProfile') }}</th>
            <th class="text-end" style="width: 125px;">{{ $t('page.dispatch.col.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!items.length">
            <td colspan="5">
              <div class="py-3 d-flex align-center justify-center text-disabled">
                <VIcon icon="tabler-package-off" size="20" class="me-1" />
                <span class="text-md">{{ $t('page.dispatch.emptyHint') }}</span>
              </div>
            </td>
          </tr>
          <tr v-for="row in items" :key="row.code">
            <td class="text-center text-disabled">{{ row.sort_order }}</td>
            <td class="text-center font-weight-medium">{{ row.code }}</td>
            <td class="text-center">{{ row.name }}</td>
            <td class="text-center text-disabled">{{ row.print_profile || '—' }}</td>
            <td class="text-center">
              <VBtn icon="tabler-edit" variant="text" color="primary" size="small" @click="openEdit(row)" />
              <VBtn icon="tabler-trash" variant="text" color="error" size="small" @click="askDelete(row)" />
            </td>
          </tr>
        </tbody>
      </VTable>
    </VCard>

    <VDialog v-model="dialogOpen" max-width="480" persistent>
      <div style="position: relative;">
        <VBtn
          icon
          variant="elevated"
          size="x-small"
          style="position: absolute; top: -12px; right: -12px; z-index: 10;"
          @click="dialogOpen = false"
        >
          <VIcon icon="tabler-x" size="14" />
        </VBtn>
      <VCard>
        <VCardTitle>{{ $t(editingOriginalCode === null ? 'page.dispatch.create' : 'common.edit') }}</VCardTitle>
        <VCardText>
          <div class="search-field mb-3">
            <label>{{ $t('page.dispatch.col.code') }}</label>
            <VTextField
              v-model="form.code"
              :placeholder="$t('page.dispatch.codePlaceholder')"
              :disabled="editingOriginalCode !== null"
              variant="outlined"
              density="compact"
              hide-details
            />
          </div>
          <div class="search-field mb-3">
            <label>{{ $t('page.dispatch.col.name') }}</label>
            <VTextField
              v-model="form.name"
              :placeholder="$t('page.dispatch.namePlaceholder')"
              variant="outlined"
              density="compact"
              hide-details
            />
          </div>
          <div class="search-field mb-3">
            <label>{{ $t('page.dispatch.col.printProfile') }}</label>
            <VTextField
              v-model="form.print_profile"
              :placeholder="$t('page.dispatch.printProfilePlaceholder')"
              variant="outlined"
              density="compact"
              hide-details
            />
          </div>
          <div class="search-field">
            <label>{{ $t('page.dispatch.col.order') }}</label>
            <VNumberInput
              v-model="form.sort_order"
              :min="0"
              placeholder="0"
              :hint="$t('page.dispatch.orderHint')"
            />
          </div>
        </VCardText>
        <VCardActions>
          <VSpacer />
          <VBtn @click="dialogOpen = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="primary" variant="elevated" @click="save">{{ $t('common.save') }}</VBtn>
        </VCardActions>
      </VCard>
      </div>
    </VDialog>

    <VDialog v-model="deleteOpen" max-width="420" persistent>
      <div style="position: relative;">
        <VBtn
          icon
          variant="elevated"
          size="x-small"
          style="position: absolute; top: -12px; right: -12px; z-index: 10;"
          @click="deleteOpen = false"
        >
          <VIcon icon="tabler-x" size="14" />
        </VBtn>
      <VCard>
        <VCardTitle>{{ $t('page.dispatch.confirmDelete') }}</VCardTitle>
        <VCardText v-if="deleteTarget">
          <i18n-t keypath="page.dispatch.confirmDeleteBody" tag="span">
            <template #name><span class="font-weight-bold">{{ deleteTarget.name }}</span></template>
            <template #code>{{ deleteTarget.code }}</template>
          </i18n-t>
          <br>
          <span class="text-body-small text-disabled">{{ $t('page.dispatch.confirmDeleteHint') }}</span>
        </VCardText>
        <VCardActions>
          <VSpacer />
          <VBtn @click="deleteOpen = false">{{ $t('common.cancel') }}</VBtn>
          <VBtn color="error" variant="elevated" @click="confirmDelete">{{ $t('common.delete') }}</VBtn>
        </VCardActions>
      </VCard>
      </div>
    </VDialog>
  </div>
</template>
