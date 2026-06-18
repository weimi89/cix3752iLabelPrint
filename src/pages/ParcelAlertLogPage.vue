<script setup>
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'
import AppHeader from '@/components/AppHeader.vue'
import { recentParcelAlerts } from '@/api/tauri'

const { t } = useI18n()
const alerts = ref([])
const loading = ref(false)
const errorMsg = ref('')
let unlisten = null

// 雲端查件異常類別名稱
const KIND_LABEL = {
  store_closed: '門市關轉',
  unconfirmed: '未確認',
  not_found: '查無此件',
  not_proxy: '非代收件',
  not_forward: '非轉寄件',
  unauthorized: '雲端未登入',
  error: '查件異常',
}
const kindLabel = k => KIND_LABEL[k] || t(`parcelAlert.${k}`, k) || k
const kindColor = k =>
  k === 'store_closed' || k === 'unconfirmed' ? 'warning'
    : k === 'not_found' ? 'secondary' : 'error'

const load = async () => {
  loading.value = true
  errorMsg.value = ''
  try {
    alerts.value = await recentParcelAlerts()
  } catch (e) {
    errorMsg.value = String(e?.message || e)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await load()
  // 即時:後端 emit parcel-alert 時刷新清單
  try {
    unlisten = await listen('parcel-alert', () => load())
  } catch (e) {
    console.warn('listen parcel-alert 失敗', e)
  }
})
onBeforeUnmount(() => {
  if (unlisten) { unlisten(); unlisten = null }
})

const empty = computed(() => !loading.value && alerts.value.length === 0)
</script>

<template>
  <div>
    <AppHeader :title="$t('page.alertLog.title')" :subtitle="$t('page.alertLog.subtitle')" icon="tabler-alert-triangle">
      <template #actions>
        <VBtn color="primary" :loading="loading" @click="load">
          <VIcon icon="tabler-refresh" size="16" class="me-1" />{{ $t('common.reload') }}
        </VBtn>
      </template>
    </AppHeader>

    <VAlert v-if="errorMsg" type="error" variant="tonal" class="mb-3">{{ errorMsg }}</VAlert>

    <VCard>
      <VTable v-if="!empty" density="comfortable">
        <thead>
          <tr>
            <th>{{ $t('page.alertLog.col.time') }}</th>
            <th>{{ $t('page.alertLog.col.kind') }}</th>
            <th>{{ $t('page.alertLog.col.queryNo') }}</th>
            <th>{{ $t('page.alertLog.col.channel') }}</th>
            <th>{{ $t('page.alertLog.col.message') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="a in alerts" :key="a.id">
            <td class="text-no-wrap text-medium-emphasis">{{ a.created_at }}</td>
            <td><VChip :color="kindColor(a.kind)" size="small" variant="tonal">{{ kindLabel(a.kind) }}</VChip></td>
            <td class="font-weight-medium">{{ a.query_no || '—' }}</td>
            <td class="text-center">{{ a.channel_code || '—' }}</td>
            <td class="text-medium-emphasis">{{ a.message || '—' }}</td>
          </tr>
        </tbody>
      </VTable>

      <div v-else class="d-flex flex-column align-center justify-center pa-10 text-medium-emphasis">
        <VIcon icon="tabler-circle-check" size="48" color="success" class="mb-2" />
        {{ $t('page.alertLog.empty') }}
      </div>
    </VCard>
  </div>
</template>
