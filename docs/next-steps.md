# 下一個工作清單(v0.2.0 published 後)

> **接手開發請先看 [`docs/handover.md`](handover.md)**(當前進度、卡住的事、未驗證項)。
> 本檔是 v0.2.0 時期的 roadmap 與跨版本經驗摘錄,不代表目前狀態。

> 2026-05-19 v0.2.0 published 後盤點。發佈策略從「四平台齊發」收斂為「三平台 + Intel 走 Rosetta 2」;印單統計從輪詢升級為 Tauri IPC event 即時推播。

## v0.2.0 已完成 ✅

### 功能 / UX
- 印單統計頁(scan / auto / ipc 三來源 `shipping_no` 去重;每日 / 每小時 / 物流商 / 貼標人員四種拆分)
- 印單統計 **Tauri IPC event 即時推播**(三個寫入點 emit `print-stats-updated`,前端 `DefaultLayout` listen,端到端 < 100ms,取代 5s 輪詢)
- Navbar 全頁印單統計 chip(今日 / 昨日,點擊跳統計頁)
- Sidebar 區塊重排:訂單列印群(含印表機設定)/ 日誌群 / 設定群
- 版本號從 Navbar 移到 Sidebar Logo 右側(白色字)
- VDatePicker zh-Hant i18n + 全綠按鈕 bug 修復
- ScanPrint / AutoPrint 列印類型下拉動態化 + localStorage 記憶操作人員 / 貼單人員 / 列印類型
- 列印前提示缺印表機 + 網路狀態加 `unconfigured`

### 發佈策略
- **release.yml 移除 `macos-13` Intel runner**(排隊 3h+ 是常態,改用 Rosetta 2 策略)
- 發佈說明 Linux 標示改 **Ubuntu Desktop**(避免 Ubuntu Server 誤裝)
- Windows NSIS `.onGUIInit` 重定義衝突修復(MUI_LANGUAGE 宏自動生成衝突)
- Windows NSIS installer authors 中文化

### 真實部署驗證(2026-05-19 完成)
- ✅ 工控機現場真實安裝 v0.2.0(tarball → install.sh → desktop entry「智配通 面單列印」出現 → GUI 正常)
- ✅ 完整業務流程實測:掃條碼 → cix3752iLabelPrint → 雲端 API → label_path → 印表機出單 → `POST /api/report` → webhook 推送雲端
- ✅ 印單統計 IPC event 即時更新驗證(工控機 IPC 寫入後 Navbar chip 立即 +1)
- ✅ 列印次數浮水印實機驗證(`(N)` 位置正確:順豐右下、其他物流商右上)

### 文件 / 開發
- README 更新:主要功能補印單統計、Tauri event 即時推播說明、版本號 v0.2.0、Release runner 表移除 Intel 行
- `package-lock.json` 加入 `.gitignore`(避免不同 host 平台 optional deps 干擾 docker build)
- `docs/local-http-api.md` 對齊 code(`print_num` / `parcel_query_log.shipping_provider`)

---

## 尚未處理

### 中優先 🟡

1. **`/reflect`** 把 source build / GHA 試錯經驗整理進 memory(見下方「經驗摘錄」段落,內容已備齊,只待跑 reflect)
2. **macOS / Windows 簽章**(避 Gatekeeper / SmartScreen)
   - macOS:Apple Dev cert($99/年)+ notarization
   - Windows:EV cert($200~600/年)
   - 商業決定,非技術問題

### 低優先 🔵

3. **離線部署 self-contained 策略**
   - 目前只有 Ubuntu 20.04 tarball 完整 self-contained(含自編 webkit2gtk-4.1 + glib 2.78 + libsoup3 + runtime .so)
   - 22.04 / 24.04 走系統 apt,離線環境無法裝
   - 若有完全離線部署需求,需把 22.04 / 24.04 也做 self-contained

---

## 已決議放棄 / 取消

| 項目 | 原計畫 | 決議 | 日期 |
|---|---|---|---|
| **macOS Intel (x64) 原生 binary** | GHA `macos-13` runner build Intel `.dmg` | 移除 Intel runner,改用 Rosetta 2 跑 ARM64 dmg。需要原生 Intel 請本地 `npm run tauri:build -- --target x86_64-apple-darwin` 自編 | 2026-05-19 |
| **Dashboard 顯示 release 版本資訊 card** | 在儀表板加 System info card | 版本號已在 Sidebar Logo 右側顯示、Middleware bind addr 也已有獨立 card,System info card 顯多餘 | 2026-05-19 |

---

## 經驗摘錄(供 `/reflect` 參考)

### Linux source build 痛點與解法

| 痛點 | 解法 |
|---|---|
| Tauri 2 hard-requires webkit2gtk-4.1,Ubuntu 20.04 無此套件 | docker `ubuntu:20.04` container 內自編 from gnome.org upstream tarball |
| `Debian source rebuild` 路線(`apt source` + `dpkg-buildpackage`)死於 `debhelper-compat 13` / `libmount-dev 2.35` 連環 deps | 改走 `upstream tarball` + `meson` / `cmake` 直接 build,繞 Debian packaging |
| webkit2gtk-4.1 cmake 缺 deps 一個個冒(unifdef、libwpe、libgbm、libxt、libwoff、libxslt 等) | 逐次 iterate 補 + disable 非必要 features(`USE_JPEGXL=OFF`、`ENABLE_WEB_AUDIO=OFF`、`USE_WAYLAND_TARGET=OFF`)|
| Tauri build linker `unable to find library -lcups` | 補 `libcups2-dev`(20.04 / 22.04 / 24.04 都要,webkit 間接 link `-lcups`) |
| Tauri build linker `undefined symbol: g_uri_error_quark`(focal glib 2.64 沒此 symbol) | 編完後 `cp -fP` 把 /usr/local 的新 .so 覆蓋 /usr/lib;`RUSTFLAGS=-Wl,-rpath,/usr/local/lib/...` 寫 RUNPATH |
| `.deb` Package: 智配通 面單列印 → dpkg 拒絕(unicode 違規) | workflow `dpkg-deb -R/-b` rewrite control 內 `Package:` 為 ASCII |
| .deb `Depends: libwebkit2gtk-4.1-0` → focal 沒此套件 → `apt -f` 把主程式 remove | matrix per-distro `deb_depends`:20.04 列 `libgtk-3-0, libayatana-appindicator3-1`;22.04+ 加 `libwebkit2gtk-4.1-0` |
| Cache v4 fail 時不 save → 每次 fail 都重編 webkit 1+ 小時 | 拆 `actions/cache/restore` + `actions/cache/save` 用 `if: always()` |
| `install.sh` ld.so.conf 只加 `/usr/local/lib`(不含 multiarch),ldconfig 找不到我們的 .so | conf 加 `/usr/local/lib/x86_64-linux-gnu`,用 `00-` 前綴排在系統 conf 之前 |
| ldd 缺 12+ runtime .so(libxslt、libwoff2、libwayland-server、libstdc++ 等) | workflow 用 `ldconfig -p` 找真實 path,ship 進 stack(處理 symlink + 真檔) |

### GHA release pipeline 痛點

| 痛點 | 解法 |
|---|---|
| Release asset 中文檔名被 sanitize 成 `_0.1.0_aarch64.dmg` | tauri-action `assetNamePattern: 'cix3752iLabelPrint_[version]_[arch][_setup][ext]'`(注意:不是 `releaseAssetNamePattern`) |
| 多次 trigger 撞 release upload race(`already_exists`) | `concurrency: group=release-{tag} + cancel-in-progress: true` |
| `workflow_dispatch` 觸發時 `github.ref_name = "main"`(branch),不是 tag | 用 `${{ github.event.inputs.tag \|\| github.ref_name }}` |
| `setup-node` `cache: npm` 報 lock file not found | 移除 `cache: npm` 設定(`package-lock.json` 不入版控) |
| Windows MSI `light.exe` fail on 中文 path | `--bundles nsis` 只出 NSIS,跳過 MSI |
| Windows NSIS `.onGUIInit` 重定義(`MUI_LANGUAGE` 巨集自動生成) | 自訂 installer hook 移除手動 `.onGUIInit` 定義 |
| `beforeBuildCommand: yarn build` 在 container 內無 yarn | 改 `npm run build`,並 tauri.conf.json `beforeDevCommand` 一起改 |
| macos-13 排隊 3h+ 拖死整個 release | v0.2.0 移除 Intel runner,接受「Rosetta 2 跑 ARM64」trade-off |

### 22.04 / 24.04 兼容 iterate 經驗

| 痛點 | 解法 |
|---|---|
| 重 trigger `workflow_dispatch` 跑同一 tag 會 desktop assets `already_exists` 衝突 | `workflow_dispatch.inputs.only`(all / linux / desktop)+ `if` 條件控制 job 是否跑 |
| Ubuntu 跨 distro tarball 不能互相借用 | 20.04 自編 glib 2.78 + webkit 2.42 跟系統 2.36 ABI 不兼容,反之亦然 — 三條獨立 tarball,客戶端認對 distro |

### 本地 docker 驗證(OrbStack)經驗

> ⚠️ **下表為 v0.2.0 當時的 npm 情境記錄**。專案自 v0.15.0 起改用 **yarn**(`yarn.lock` 入版控):
> `yarn.lock` 含全部平台的 optional binding,不會有「host 平台鎖死、其他平台 binding 缺席」的問題,
> 故 `tests/docker-ubuntu-build.sh` 現在**只刪 `package-lock.json`、保留 `yarn.lock`**(留著才驗證得到
> 版控的 lock 在 Linux 可重現建置)。詳見 CLAUDE.md「規範」一節。

| 痛點 | 解法 |
|---|---|
| host(macOS arm64)mount 進 linux container 後 `npm run tauri:build` 報 `Cannot find native binding @tauri-apps/cli-linux-x64-gnu` | host 跑過 `npm install` 後 `package-lock.json` 內 optionalDependencies 鎖死 `darwin-arm64` 平台,其他 platform binding entry 被 npm 從 lock 移掉;container 內 `rm -rf node_modules package-lock.json` 後重 `npm install` 才解所有 platform optional deps。**根治**:`package-lock.json` 列入 `.gitignore`(v0.2.0 完成) |
| host 專案直接 `-v $PWD:/workspace:rw` 會把 host build 產物污染 container | `-v $PWD:/source:ro` + container 內 `cp -a /source/. /workspace/` 後 `rm -rf node_modules src-tauri/target package-lock.json` 隔離 |
| 22.04 / 24.04 install 測試:用 `ldd $(which cix3752i-label-print) \| grep 'not found'` 直接判定缺什麼 .so | `apt install ./*.deb` 讓 apt 自動解 Depends 拉系統 `libwebkit2gtk-4.1-0` 等,不需 install.sh 多步 |

### Tauri 即時通訊架構決策

| 選項 | 為何選 / 不選 |
|---|---|
| ❌ WebSocket | 桌面 App 後端與前端在同一進程,WebSocket 反而要開 port、做認證、處理重連,繞遠路 |
| ✅ Tauri IPC event(`emit` / `listen`) | 內建、進程內通道、毫秒級、不用認證、不用 socket server。寫入點 `app.emit("print-stats-updated", payload)` → 前端 `listen("print-stats-updated", cb)`,端到端 < 100ms |
| ❌ 輪詢縮短間隔 | 5s → 1s 仍不夠即時且浪費資源(99% 時間無變動);印單統計改 event-driven,系統狀態(server/queue/cache)維持 5s |
