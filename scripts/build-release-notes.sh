#!/usr/bin/env bash
# 產生 GitHub Release 說明 / updater notes = CHANGELOG.md 對應 tag 段落(本次更新)。
# release.yml 的 desktop(tauri-action)與 linux(softprops)兩個 job 都呼叫此腳本產生「同一份」body,
# 避免多 job 搶建同一個 release 時、由沒帶 body 的那個搶先建立而導致網頁 body 空白。
# 只輸出變更內容:更新通知是給已安裝的舊版用戶看的,安裝包表格 / 首次啟動注意對他們多餘。
# 用法:build-release-notes.sh <tag>   例:build-release-notes.sh v0.4.2
set -euo pipefail

TAG="${1:?usage: build-release-notes.sh <tag>}"

# 從 CHANGELOG.md 抽出 `## <tag>` 到下一個 `## ` 之間的內容
CHANGES=$(awk -v tag="$TAG" '
  $0 ~ "^## " tag "([^0-9]|$)" { found=1; next }
  found && /^## / { exit }
  found { print }
' CHANGELOG.md)

if [ -n "$(printf '%s' "$CHANGES" | tr -d '[:space:]')" ]; then
  printf '## 本次更新\n\n%s\n' "$CHANGES"
else
  # CHANGELOG 漏寫對應段落時的保底,避免說明整段空白
  printf '## 更新到 %s\n' "$TAG"
fi
