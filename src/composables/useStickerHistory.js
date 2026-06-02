import { ref } from 'vue'
import { stickerHistoryAdd, stickerHistoryDelete, stickerHistoryList } from '@/api/tauri'

/**
 * 人員歷史名單(操作人員 / 貼單人員 / 貼標人員三者共用同一份)。
 * 後端對應 sticker_history 表,提供下拉記憶與刪除。
 */
export function useStickerHistory() {
  const history = ref([])

  const reload = async () => {
    history.value = await stickerHistoryList()
  }

  // 把名字加入歷史(去重後置頂),並同步後端
  const add = async name => {
    const n = (name || '').trim()
    if (!n) return
    await stickerHistoryAdd(n)
    history.value = [n, ...history.value.filter(x => x !== n)]
  }

  const remove = async name => {
    await stickerHistoryDelete(name)
    history.value = history.value.filter(x => x !== name)
  }

  return { history, reload, add, remove }
}
