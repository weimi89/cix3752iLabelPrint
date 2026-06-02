<script setup>
// Sidebar logo 顯示 30x30 — 用 Tauri Square*Logo 系列(Windows Store tile 規格,
// 各 DPI 倍率「特別設計過」的小尺寸版本,1:1 像素對齊不靠縮放)
// 1x DPI: 30x30(完美對齊)/ 2x: 71x71 → 60px(1.18x 微縮)/ 3x: 89x89 → 90px(0.99x 幾乎 1:1)
import appIcon30 from '@images/app-icon-30.png'
import appIcon71 from '@images/app-icon-71.png'
import appIcon89 from '@images/app-icon-89.png'
</script>

<template>
  <img
    :src="appIcon30"
    :srcset="`${appIcon30} 1x, ${appIcon71} 2x, ${appIcon89} 3x`"
    width="30"
    height="30"
    alt="智配通"
    aria-hidden="true"
    draggable="false"
  >
</template>
