#!/bin/bash
# dev-codesign-run.sh — cargo runner wrapper for stable dev codesign
#
# 由 .cargo/config.toml 的 [target.macos] runner 觸發
# 用法:scripts/dev-codesign-run.sh <binary> [args...]
#
# 環境變數:
#   CIX3752I_DEV_SIGN_IDENTITY  簽章 identity 名稱(預設 "-",代表 ad-hoc)
#                               推薦設為 "Apple Development: <你的名字> (<TeamID>)"
#                               或自建 self-signed 的名稱
#
# 設定一次後,dev binary 永遠用同一個 identity 簽章,
# macOS keychain 「永遠允許」就會持續有效。

set -e

BINARY="$1"
shift

IDENTITY="${CIX3752I_DEV_SIGN_IDENTITY:--}"
# Dev .app bundle 名稱:macOS 從 executable path 往上找 .app/Contents/Info.plist,
# 找到就用 CFBundleDisplayName 顯示 Dock tooltip,所以包成 .app 結構即可
APP_NAME="${CIX3752I_DEV_APP_NAME:-智配通 面單列印}"

if [ -z "$BINARY" ] || [ ! -f "$BINARY" ]; then
  echo "[dev-codesign-run] 找不到 binary: $BINARY" >&2
  exit 1
fi

# 非 macOS 直接 exec
if [ "$(uname)" != "Darwin" ]; then
  exec "$BINARY" "$@"
fi

# === macOS:把 binary 包進 .app bundle 結構 ===
DEBUG_DIR="$(dirname "$BINARY")"
BIN_NAME="$(basename "$BINARY")"
APP_DIR="$DEBUG_DIR/$APP_NAME.app"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RES_DIR="$APP_DIR/Contents/Resources"
PLIST="$APP_DIR/Contents/Info.plist"
DEST_BIN="$MACOS_DIR/$BIN_NAME"

mkdir -p "$MACOS_DIR" "$RES_DIR"

# Info.plist(每次都寫,確保 APP_NAME 變更會生效)
cat > "$PLIST" <<PLIST_EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleExecutable</key><string>$BIN_NAME</string>
    <key>CFBundleIdentifier</key><string>com.weimi.cix3752i.labelprint.dev</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0-dev</string>
    <key>CFBundleVersion</key><string>0.1.0-dev</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleIconFile</key><string>icon</string>
</dict>
</plist>
PLIST_EOF

# Icon(從 src-tauri/icons/icon.icns 複製;script 在 <repo>/scripts/)
ICON_SRC="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/icons/icon.icns"
if [ -f "$ICON_SRC" ]; then
  cp -f "$ICON_SRC" "$RES_DIR/icon.icns"
fi

# 同步 binary 進 .app(cargo 每次 rebuild 都覆寫原 BINARY,要 cp 過來)
cp -f "$BINARY" "$DEST_BIN"
chmod +x "$DEST_BIN"

# Codesign 整個 bundle(--deep 確保 inner binary 也被簽)
if ! codesign --force --deep --sign "$IDENTITY" "$APP_DIR" 2>/tmp/cix3752i_codesign.err; then
  echo "[dev-codesign-run] codesign 失敗(identity=$IDENTITY):" >&2
  cat /tmp/cix3752i_codesign.err >&2
  echo "[dev-codesign-run] 退回 ad-hoc 簽章繼續啟動" >&2
  codesign --force --deep --sign - "$APP_DIR" 2>/dev/null || true
fi

# Exec .app 內的 binary(macOS 自動往上找 .app/Contents/Info.plist 用作 process bundle)
exec "$DEST_BIN" "$@"
