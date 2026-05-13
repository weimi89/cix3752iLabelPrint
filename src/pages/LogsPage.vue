<script setup>
import AppHeader from '@/components/AppHeader.vue'
</script>

<template>
  <div>
    <AppHeader title="Log 查詢" subtitle="事件記錄、Queue 歷史、雲端推送結果" icon="tabler-file-text" />
    <VCard>
      <VCardText>
        <div class="text-center py-12 text-medium-emphasis">
          <VIcon icon="tabler-file-search" size="80" class="opacity-50 mb-3" />
          <div>待實作：列出 event_log 與 report_queue 歷史</div>
        </div>
      </VCardText>
    </VCard>
  </div>
</template>
