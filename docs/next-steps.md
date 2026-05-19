# 下一個工作清單(v0.1.0 published 後)

> 2026-05-19 v0.1.0 published 後盤點。三平台已交付(macOS arm64 + Windows + Linux 20.04),但仍有改進空間。

## 已完成 ✅

- v0.1.0 published — macOS arm64 + Windows NSIS + Linux Ubuntu 20.04 tarball
- Linux Ubuntu 20.04 docker 實測通過(ldd 全 link、binary 啟動 OK)
- dev codesign cargo runner + .app bundle wrapper(Dock 顯示中文)
- 列印次數浮水印 + label_path 三模式(local / share / http)
- 請求記錄頁 + 雙語 i18n(zh-Hant / vi-VN)
- **Ubuntu 22.04 / 24.04 兼容**(2026-05-19):release-linux matrix 三 distro,
  22.04+ 走系統 webkit2gtk-4.1,本地 docker 驗證 ldd 全 link(待 GHA workflow_dispatch 觸發驗證)
- **README 補 release 實測 table + Known limitations**(2026-05-19)
- **release-desktop 加 60min job timeout**(macos-13 hang 不拖死整個 release)
- **docs/local-http-api.md 對齊 code**(2026-05-19):補 `print_num` / `parcel_query_log.shipping_provider`

---

## 高優先 🔴 — 真實部署驗證

1. **工控機現場真實安裝 v0.1.0**
   - 解壓 tarball → `sudo bash install.sh`
   - 確認 desktop entry「智配通 面單列印」出現在應用選單
   - 開啟後 GUI 正常顯示
2. **完整業務流程實測**(工控機端跑)
   - 掃條碼 → cix3752iLabelPrint → 雲端 API → 取得 label_path → 印表機出單
   - `POST /api/report` 回報結果 → webhook 推送到雲端
3. **列印次數浮水印實機驗證**
   - 同包裹列印 2+ 次,確認右上(順豐右下)有 `(N)` 字樣
   - 順豐 vs 其他物流商位置切換正確

## 中優先 🟡 — Release pipeline 收尾

4. **`/reflect`** 把 source build / GHA 試錯經驗整理進 memory
   - Tauri 2 + Ubuntu 20.04 完整 stack source build 路線
   - `assetNamePattern`(不是 `releaseAssetNamePattern`)正確 input 名
   - `.deb` Package 名違規處理(productName 中文 → workflow rewrite)
   - linker `-L` 順序問題:覆蓋系統 .so 解法
   - cache v4 fail 時不 save → 拆 restore/save + `if: always()`
   - 跨 distro 限制:focal build 對 jammy 不向上(libffi SONAME)

5. **macOS Intel (x64)** 補出貨
   - GHA `macos-13` runner 排隊問題:換 `macos-latest` build universal binary,或設 self-hosted
   - 或接受 cancel,標 known limitation

6. **macOS / Windows 簽章**
   - macOS:Apple Dev cert + notarization(避 Gatekeeper)
   - Windows:EV cert(避 SmartScreen,商業憑證 $200~600/年)

7. ~~**Ubuntu 22.04 / 24.04 兼容**~~ ✅ 2026-05-19 完成 — 改開三條 matrix pipeline(20.04 + 22.04 + 24.04),22.04+ 走系統 webkit2gtk-4.1

## 中優先 🟢 — 文件與後續

8. ~~**README** 加 release 流程實測結果 + Known limitations~~ ✅ 2026-05-19 完成
9. ~~**docs/local-http-api.md** 與現有 code 對齊~~ ✅ 2026-05-19 完成(補 `print_num` 為雲端回應第 6 欄、`parcel_query_log` 表格補 `shipping_provider`;對外契約未變動)
10. ~~**.deb Depends** 補完整 runtime deps~~ ✅ 2026-05-19 完成(per-distro 設定:20.04 `libgtk-3-0` + `appindicator3-1`;22.04+ 加 `libwebkit2gtk-4.1-0`)

## 低優先 🔵 — DX 改善

11. ~~**GHA workflow 加 timeout** 給 macos-13 job~~ ✅ 2026-05-19 完成(release-desktop job timeout-minutes: 60)
12. **Dashboard** 顯示 release 版本資訊
13. **離線部署** 完整 self-contained 策略(目前 docker 容器內 minimal Ubuntu 仍缺 libgtk-3-0 等)

---

## 本次 release iterate 經驗摘錄(供 /reflect 參考)

### Linux source build 痛點與解法

| 痛點 | 解法 |
|---|---|
| Tauri 2 hard-requires webkit2gtk-4.1,Ubuntu 20.04 無此套件 | docker `ubuntu:20.04` container 內自編 from gnome.org upstream tarball |
| `Debian source rebuild` 路線(`apt source` + `dpkg-buildpackage`)死於 `debhelper-compat 13` / `libmount-dev 2.35` 連環 deps | 改走 `upstream tarball` + `meson` / `cmake` 直接 build,繞 Debian packaging |
| webkit2gtk-4.1 cmake 缺 deps 一個個冒(unifdef、libwpe、libgbm、libxt、libwoff、libxslt 等) | 逐次 iterate 補 + disable 非必要 features(`USE_JPEGXL=OFF`、`ENABLE_WEB_AUDIO=OFF`、`USE_WAYLAND_TARGET=OFF`)|
| Tauri build linker `unable to find library -lcups` | 補 `libcups2-dev` |
| Tauri build linker `undefined symbol: g_uri_error_quark`(focal glib 2.64 沒此 symbol) | 編完後 `cp -fP` 把 /usr/local 的新 .so 覆蓋 /usr/lib;`RUSTFLAGS=-Wl,-rpath,/usr/local/lib/...` 寫 RUNPATH |
| `.deb` Package: 智配通 面單列印 → dpkg 拒絕(unicode 違規) | workflow `dpkg-deb -R/-b` rewrite control 內 `Package:` 為 ASCII |
| .deb `Depends: libwebkit2gtk-4.1-0` → focal 沒此套件 → `apt -f` 把主程式 remove | sed 整行替換 Depends 為 ASCII focal 有的套件 |
| Cache v4 fail 時不 save → 每次 fail 都重編 webkit 1+ 小時 | 拆 `actions/cache/restore` + `actions/cache/save` 用 `if: always()` |
| `install.sh` ld.so.conf 只加 `/usr/local/lib`(不含 multiarch),ldconfig 找不到我們的 .so | conf 加 `/usr/local/lib/x86_64-linux-gnu`,用 `00-` 前綴排在系統 conf 之前 |
| ldd 缺 12+ runtime .so(libxslt、libwoff2、libwayland-server、libstdc++ 等) | workflow 用 `ldconfig -p` 找真實 path,ship 進 stack(處理 symlink + 真檔) |

### GHA release pipeline 痛點

| 痛點 | 解法 |
|---|---|
| Release asset 中文檔名被 sanitize 成 `_0.1.0_aarch64.dmg` | tauri-action `assetNamePattern: 'cix3752iLabelPrint_[version]_[arch][_setup][ext]'`(注意:不是 `releaseAssetNamePattern`) |
| 多次 trigger 撞 release upload race(`already_exists`) | `concurrency: group=release-{tag} + cancel-in-progress: true` |
| `workflow_dispatch` 觸發時 `github.ref_name = "main"`(branch),不是 tag | 用 `${{ github.event.inputs.tag || github.ref_name }}` |
| `setup-node` `cache: npm` 報 lock file not found(yarn.lock untracked、無 package-lock.json) | 移除 `cache: npm` 設定 |
| Windows MSI `light.exe` fail on 中文 path | `--bundles nsis` 只出 NSIS,跳過 MSI |
| `beforeBuildCommand: yarn build` 在 container 內無 yarn | 改 `npm run build`,並 tauri.conf.json `beforeDevCommand` 一起改 |
| `concurrency cancel-in-progress` 把跑中的舊 run mac arm64 + Linux 全 cancel | 接受 trade-off,或 trigger 前先確認舊 run 狀態 |

---

## 22.04 / 24.04 兼容 iterate 經驗摘錄(2026-05-19)

### Release workflow 經驗

| 痛點 | 解法 |
|---|---|
| Tauri 2 22.04 / 24.04 build linker `unable to find library -lcups` | 22.04+ Bootstrap apt 也要裝 `libcups2-dev`(原以為只 20.04 需要;webkit 間接 link `-lcups`) |
| 重 trigger `workflow_dispatch` 跑 v0.1.0 會 desktop assets `already_exists` 衝突 | `workflow_dispatch.inputs.only`(all / linux / desktop)+ `if: github.event.inputs.only != 'linux'` 在 release-desktop job 上,推 tag 觸發時 input 空字串,只跑該跑的 |
| `release-linux` 三 distro `.deb` Depends 寫死同一行 sed 會錯 | matrix 加 `deb_depends:` 屬性 per-distro:20.04 列 `libgtk-3-0, libayatana-appindicator3-1`(focal 無 libwebkit2gtk-4.1-0);22.04+ 加 `libwebkit2gtk-4.1-0` |
| Ubuntu 跨 distro tarball 不能互相借用 | 20.04 自編 glib 2.78 + webkit 2.42 跟系統 2.36 ABI 不兼容,反之亦然 — 三條獨立 tarball,客戶端認對 distro |

### 本地 docker 驗證(OrbStack)經驗

| 痛點 | 解法 |
|---|---|
| host(macOS arm64)mount 進 linux container 後 `npm run tauri:build` 報 `Cannot find native binding @tauri-apps/cli-linux-x64-gnu` | host 跑過 `npm install` 後 `package-lock.json` 內 optionalDependencies 鎖死 `darwin-arm64` 平台,其他 platform 的 binding entry 被 npm 從 lock 移掉;container 內 `rm -rf node_modules package-lock.json` 後 `npm install` 才會解所有 platform optional deps(GHA 因為 `package-lock.json` 沒入 git 不會撞此問題) |
| host 專案直接 `-v $PWD:/workspace:rw` 會把 host build 產物(`node_modules` / `src-tauri/target`)污染 container build | `-v $PWD:/source:ro` + container 內 `cp -a /source/. /workspace/` 後 `rm -rf node_modules src-tauri/target package-lock.json` 隔離 |
| 22.04 / 24.04 install 測試:用 `ldd $(which cix3752i-label-print) \| grep 'not found'` 直接判定缺什麼 .so | `apt install ./*.deb` 讓 apt 自動解 Depends 拉系統 `libwebkit2gtk-4.1-0` 等,不需 install.sh 多步 |
