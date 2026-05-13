<script setup>
import { usePage } from '@inertiajs/vue3'
import { layoutConfig } from '@layouts'
import { useLayoutConfigStore } from '@layouts/stores/config'
import { injectionKeyIsVerticalNavHovered } from '@layouts/symbols'
import {
  getComputedNavLinkToProp,
  getDynamicI18nProps,
  isNavLinkActive,
} from '@layouts/utils'

const props = defineProps({
  item: {
    type: null,
    required: true,
  },
})

const configStore = useLayoutConfigStore()

// 明確 inject hover ref 並傳入 isVerticalNavMini，與 VerticalNavGroup 行為一致，避免隱式依賴
const isVerticalNavHovered = inject(injectionKeyIsVerticalNavHovered, ref(false))
const hideTitleAndBadge = configStore.isVerticalNavMini(isVerticalNavHovered)

// 透過 usePage().url 建立 reactive dependency；URL 沒變時 active 狀態快取，
// 避免每次 template render 都呼叫 Ziggy `route().current()` 比對
const page = usePage()

const isActive = computed(() => {
  // eslint-disable-next-line no-unused-expressions
  page.url
  return isNavLinkActive(props.item)
})

// 把 href / target / rel 一次算好，item 不變時保持同一份物件 reference，
// 避免每次 render 跑 Ziggy `route()` 與 v-bind 重新 patch
const linkProps = computed(() => getComputedNavLinkToProp.value(props.item))

// i18n props 在未啟用時是空物件，但原本每次 render 都 new {} 會觸發 v-bind diff；
// 改用 computed 在 item.title 不變時保持同 reference
const titleI18nProps = computed(() => getDynamicI18nProps(props.item.title, 'span'))
const badgeI18nProps = computed(() => getDynamicI18nProps(props.item.badgeContent, 'span'))
</script>

<template>
  <li
    class="nav-link"
    :class="{ disabled: item.disable }"
  >
    <Component
      :is="item?.to ? 'Link' : 'a'"
      v-bind="linkProps"
      :class="{ 'router-link-active router-link-exact-active': isActive }"
    >
      <Component
        :is="layoutConfig.app.iconRenderer || 'div'"
        v-bind="item.icon || layoutConfig.verticalNav.defaultNavItemIconProps"
        class="nav-item-icon"
      />
      <!-- 👉 Title -->
      <!-- 改用單一 element 的 Transition：動畫效果一致（v-show 切換 Vue 3 會套 enter/leave class），
           但省掉 TransitionGroup 的 FLIP children 追蹤開銷 -->
      <Transition name="transition-slide-x">
        <Component
          :is="layoutConfig.app.i18n.enable ? 'i18n-t' : 'span'"
          v-show="!hideTitleAndBadge"
          class="nav-item-title"
          v-bind="titleI18nProps"
        >
          {{ item.title }}
        </Component>
      </Transition>

      <!-- 👉 Badge -->
      <Transition
        v-if="item.badgeContent"
        name="transition-slide-x"
      >
        <Component
          :is="layoutConfig.app.i18n.enable ? 'i18n-t' : 'span'"
          v-show="!hideTitleAndBadge"
          class="nav-item-badge"
          :class="item.badgeClass"
          v-bind="badgeI18nProps"
        >
          {{ item.badgeContent }}
        </Component>
      </Transition>
    </Component>
  </li>
</template>

<style lang="scss">
.layout-vertical-nav {
  .nav-link a {
    display: flex;
    align-items: center;
  }
}
</style>
