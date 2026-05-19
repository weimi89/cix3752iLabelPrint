; Tauri NSIS installer hook — 強制 install dir 為 ASCII
;
; 原因:productName 是「智配通 面單列印」(Tauri 把它當 install dir 名),
; 預設安裝路徑為 C:\Users\<u>\AppData\Local\智配通 面單列印,中文目錄不便
; 於 cmd / log / 第三方工具的存取與顯示。
;
; 為何用 MUI_CUSTOMFUNCTION_GUIINIT 而非自定義 Function .onGUIInit:
;   NSIS MUI 內部會在 MUI_LANGUAGE macro 展開時自動生成 .onGUIInit Function
;   (透過 MUI_FUNCTION_GUIINIT),自定義第二個 Function .onGUIInit 會 build
;   報「Function named ".onGUIInit" already exists」。MUI 提供
;   MUI_CUSTOMFUNCTION_GUIINIT 介面讓我們的 callback 被 inject 進去而不覆寫。
;
; 為何用 GUIInit 階段而非 NSIS_HOOK_PREINSTALL:
;   NSIS_HOOK_PREINSTALL 在 user 已透過 MUI_PAGE_DIRECTORY 看 / 確認 path
;   後才跑,reset $INSTDIR 會造成 UI 顯示 vs 實際 install 路徑不一致。
;   .onGUIInit 在所有 MUI page 之前跑,reset $INSTDIR 後 user 在 DIRECTORY
;   page 看到的就是英文路徑,流程一致。
;
; 維持 productName 中文 → macOS Dock / Linux .desktop 顯示中文不變。
; 只覆蓋 Windows install dir 預設值,user 仍可在 DIRECTORY page 自選別處。
!define MUI_CUSTOMFUNCTION_GUIINIT cixForceInstallDir

Function cixForceInstallDir
  StrCpy $INSTDIR "$LOCALAPPDATA\cix3752iLabelPrint"
FunctionEnd
