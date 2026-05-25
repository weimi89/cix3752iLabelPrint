/**
 * 自動更新檢查(Tauri Updater Plugin)
 *
 * - 啟動後 5 秒(避免擋首屏)check GitHub Release 的 latest.json
 * - 有新版 → updateAvailable 變 true、updateInfo 帶 version / notes
 * - downloadAndInstall() 下載 + 安裝 + relaunch
 * - 失敗只 console.warn,不打擾使用者(無網路 / 非 Tauri runtime 都會 fail)
 */
import { ref } from 'vue'

const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

const updateAvailable = ref(false)
const updateInfo = ref(null)  // { version, currentVersion, notes, date }
const isChecking = ref(false)
const isDownloading = ref(false)
const downloadProgress = ref(0)  // 0-100
const lastError = ref(null)

let updateHandle = null  // 暫存 Update instance 供下載階段使用

export const checkForUpdates = async () => {
  if (!isTauri) return
  if (isChecking.value) return
  isChecking.value = true
  lastError.value = null
  try {
    const { check } = await import('@tauri-apps/plugin-updater')
    const update = await check()
    if (update?.available) {
      updateHandle = update
      updateInfo.value = {
        version: update.version,
        currentVersion: update.currentVersion,
        notes: update.body || '',
        date: update.date || '',
      }
      updateAvailable.value = true
    } else {
      updateAvailable.value = false
      updateInfo.value = null
    }
  } catch (e) {
    console.warn('[updater] check failed:', e)
    lastError.value = String(e)
  } finally {
    isChecking.value = false
  }
}

export const downloadAndInstall = async () => {
  if (!isTauri || !updateHandle) return
  if (isDownloading.value) return
  isDownloading.value = true
  downloadProgress.value = 0
  lastError.value = null
  try {
    let downloaded = 0
    let total = 0
    await updateHandle.downloadAndInstall(event => {
      switch (event.event) {
        case 'Started':
          total = event.data.contentLength || 0
          break
        case 'Progress':
          downloaded += event.data.chunkLength || 0
          if (total > 0) downloadProgress.value = Math.round((downloaded / total) * 100)
          break
        case 'Finished':
          downloadProgress.value = 100
          break
      }
    })
    // 安裝完成後 relaunch
    const { relaunch } = await import('@tauri-apps/plugin-process')
    await relaunch()
  } catch (e) {
    console.warn('[updater] install failed:', e)
    lastError.value = String(e)
    isDownloading.value = false
  }
}

export const dismissUpdate = () => {
  updateAvailable.value = false
}

export const useUpdater = () => ({
  updateAvailable,
  updateInfo,
  isChecking,
  isDownloading,
  downloadProgress,
  lastError,
  checkForUpdates,
  downloadAndInstall,
  dismissUpdate,
})

// 啟動 5 秒後跑首次檢查(避開首屏 IPC 高峰)
if (isTauri) {
  setTimeout(() => { checkForUpdates() }, 5000)
}
