<script setup>
import { cloudLogout } from '@/api/tauri'
import { useStatusStore } from '@/stores/status'
import { useI18n } from 'vue-i18n'

const status = useStatusStore()
const router = useRouter()

const { t } = useI18n()

const userName = computed(() => status.cloud.user_label || t('user.notLoggedIn'))
const userRole = computed(() => t(status.cloud.logged_in ? 'user.role.admin' : 'user.role.guest'))

const logout = async () => {
  await cloudLogout()
  await status.refreshAll()
  router.push('/cloud-settings')
}
</script>

<template>
  <VMenu offset="8" :close-on-content-click="false">
    <template #activator="{ props: actv }">
      <div class="app-navbar__avatar" :class="{ 'is-online': status.cloud.logged_in }" v-bind="actv">
        <VAvatar size="34" class="app-navbar__avatar-img">
          <VIcon icon="tabler-user" color="primary" />
        </VAvatar>
      </div>
    </template>

    <VCard min-width="230" class="user-menu">
      <VCardItem class="px-3 pt-3 pb-2">
        <template #prepend>
          <div class="app-navbar__avatar is-online" style="margin: 0;">
            <VAvatar size="40" class="app-navbar__avatar-img">
              <VIcon icon="tabler-user" color="primary" />
            </VAvatar>
          </div>
        </template>
        <VCardTitle class="text-body-1 font-weight-medium">{{ userName }}</VCardTitle>
        <VCardSubtitle class="text-caption">{{ userRole }}</VCardSubtitle>
      </VCardItem>
      <VDivider />
      <div class="pa-2">
        <VBtn color="error" block @click="logout">
          <span class="me-2">{{ $t('user.logout') }}</span><VIcon icon="tabler-logout" size="18" />
        </VBtn>
      </div>
    </VCard>
  </VMenu>
</template>

<style scoped>
.user-menu :deep(.v-list-item) {
  border-radius: 6px;
  margin: 2px 8px;
  min-block-size: 38px;
}
</style>
