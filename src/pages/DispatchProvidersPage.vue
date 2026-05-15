<script setup>
import {
  dispatchProviderList,
  dispatchProviderUpsert,
  dispatchProviderDelete,
} from '@/api/tauri'
import AppHeader from '@/components/AppHeader.vue'

const isTauriRuntime = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const items = ref([])
const loading = ref(false)
const errorMsg = ref('')
const flashMsg = ref('')

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
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}
onMounted(load)

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
    errorMsg.value = '代碼與名稱皆不可為空'
    return
  }
  if (!printProfile) {
    errorMsg.value = '列印設定為必填'
    return
  }
  // 新增時若代碼已存在，警告
  if (editingOriginalCode.value === null
      && items.value.some(it => it.code === code)) {
    errorMsg.value = `代碼 "${code}" 已存在`
    return
  }
  try {
    await dispatchProviderUpsert({
      code,
      name,
      sortOrder: Number(form.value.sort_order) || 0,
      printProfile,
    })
    dialogOpen.value = false
    flash(editingOriginalCode.value === null ? '已新增物流商' : '已更新物流商')
    await load()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
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
    flash(`已刪除 ${deleteTarget.value.name}`)
    deleteTarget.value = null
    await load()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  }
}
</script>

<template>
  <div>
    <AppHeader title="指派物流" subtitle="分揀通道可選擇的物流商主檔" icon="tabler-truck-delivery">
      <template #actions>
        <VBtn color="primary" @click="openCreate">
          <VIcon icon="tabler-plus" size="16" class="me-1" />新增物流
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="!isTauriRuntime" type="info" variant="tonal" class="mb-3" icon="tabler-info-circle">
      瀏覽器預覽模式 — 顯示的是示範資料,實機請於桌面 App 內開啟。
    </VAlert>
    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>
    <VAlert v-if="flashMsg" type="success" variant="tonal" class="mb-3">{{ flashMsg }}</VAlert>

    <VCard>
      <VTable hover>
        <thead>
          <tr>
            <th class="text-center" style="width: 80px;">排序</th>
            <th style="min-width: 100px;">代碼</th>
            <th style="min-width: 160px;">名稱</th>
            <th style="min-width: 180px;">列印設定</th>
            <th class="text-end" style="width: 125px;">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!items.length">
            <td colspan="5">
              <div class="py-3 d-flex align-center justify-center text-disabled">
                <VIcon icon="tabler-package-off" size="20" class="me-1" />
                <span class="text-md">尚未設定物流商，按右上「新增物流」開始</span>
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

    <!-- 新增 / 編輯 dialog -->
    <VDialog v-model="dialogOpen" max-width="480">
      <VCard>
        <VCardTitle>{{ editingOriginalCode === null ? '新增物流' : '編輯' }}</VCardTitle>
        <VCardText>
          <div class="search-field mb-3">
            <label>代碼</label>
            <VTextField
              v-model="form.code"
              placeholder="例: SF / BLACK / POST"
              :disabled="editingOriginalCode !== null"
              variant="outlined"
              density="compact"
              hide-details
            />
          </div>
          <div class="search-field mb-3">
            <label>名稱</label>
            <VTextField
              v-model="form.name"
              placeholder="例: 順豐速運"
              variant="outlined"
              density="compact"
              hide-details
            />
          </div>
          <div class="search-field mb-3">
            <label>列印設定</label>
            <VTextField
              v-model="form.print_profile"
              placeholder="例: PAPER-01#100x150"
              variant="outlined"
              density="compact"
              hide-details
            />
          </div>
          <div class="search-field">
            <label>排序</label>
            <VNumberInput
              v-model="form.sort_order"
              :min="0"
              placeholder="0"
              hint="數字越小越前面"
            />
          </div>
        </VCardText>
        <VCardActions>
          <VSpacer />
          <VBtn @click="dialogOpen = false">取消</VBtn>
          <VBtn color="primary" variant="elevated" @click="save">儲存</VBtn>
        </VCardActions>
      </VCard>
    </VDialog>

    <!-- 刪除確認 dialog -->
    <VDialog v-model="deleteOpen" max-width="420">
      <VCard>
        <VCardTitle>確認刪除</VCardTitle>
        <VCardText v-if="deleteTarget">
          確定要刪除「<span class="font-weight-bold">{{ deleteTarget.name }}</span>」({{ deleteTarget.code }}) 嗎?
          <br>
          <span class="text-caption text-disabled">已指派此物流的通道會自動清空 dispatch_code。</span>
        </VCardText>
        <VCardActions>
          <VSpacer />
          <VBtn @click="deleteOpen = false">取消</VBtn>
          <VBtn color="error" variant="elevated" @click="confirmDelete">刪除</VBtn>
        </VCardActions>
      </VCard>
    </VDialog>
  </div>
</template>
