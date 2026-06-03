#!/usr/bin/env bash
# 產生 GitHub Release 說明 = CHANGELOG.md 對應 tag 段落(本次更新)+ 安裝包/首次啟動樣板。
# release.yml 的 desktop(tauri-action)與 linux(softprops)兩個 job 都呼叫此腳本產生「同一份」body,
# 避免多 job 搶建同一個 release 時、由沒帶 body 的那個搶先建立而導致網頁 body 空白。
# 用法:build-release-notes.sh <tag>   例:build-release-notes.sh v0.4.2
set -euo pipefail

TAG="${1:?usage: build-release-notes.sh <tag>}"

# 從 CHANGELOG.md 抽出 `## <tag>` 到下一個 `## ` 之間的內容
CHANGES=$(awk -v tag="$TAG" '
  $0 ~ "^## " tag "([^0-9]|$)" { found=1; next }
  found && /^## / { exit }
  found { print }
' CHANGELOG.md)

# 有對應段落才放「本次更新」;沒有就只出安裝包樣板(不致整段空白)
if [ -n "$(printf '%s' "$CHANGES" | tr -d '[:space:]')" ]; then
  printf '## 本次更新\n\n%s\n\n' "$CHANGES"
fi

cat <<'STATIC_EOF'
## 安裝包

| 平台 | 檔案 |
|---|---|
| macOS Apple Silicon (M1/M2/M3+) | `*_aarch64.dmg` |
| macOS Intel | 沒專屬版,請裝 Rosetta 2 後跑 `*_aarch64.dmg` |
| Windows x64 | `*_x64-setup.exe` (NSIS) 或 `*_x64_en-US.msi` |
| Ubuntu Desktop 20.04 工控機(離線) | `*-ubuntu-20.04.tar.gz`(含 webkit2gtk-4.1 + 主程式 + install.sh) |
| Ubuntu Desktop 22.04 | `*-ubuntu-22.04.tar.gz`(走系統 webkit2gtk-4.1,install.sh 跑 apt) |
| Ubuntu Desktop 24.04 | `*-ubuntu-24.04.tar.gz`(走系統 webkit2gtk-4.1,install.sh 跑 apt) |

## 首次啟動注意

- **macOS**:本版未經 Apple Developer ID 簽章,首次開啟會被 Gatekeeper 攔下。
  請至「系統設定 → 隱私權與安全性 → 仍要開啟」放行。
- **Windows**:首次開啟會看到 SmartScreen 警告,按「更多資訊 → 仍要執行」即可。
- **Linux**:解壓對應版本的 tarball 後 `sudo bash install.sh` 一次。
  20.04 走離線 stack(無網路也能跑);22.04 / 24.04 走 apt 抓系統 webkit2gtk-4.1。
STATIC_EOF
