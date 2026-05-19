; Tauri NSIS installer hook — 強制 install dir 為 ASCII
;
; 原因:productName 是「智配通 面單列印」(Tauri 把它當 install dir 名),
; 預設安裝路徑為 C:\Users\<u>\AppData\Local\智配通 面單列印,中文目錄不便
; 於 cmd / log / 第三方工具的存取與顯示。
;
; 為何用 .onGUIInit 而非 NSIS_HOOK_PREINSTALL:
;   NSIS_HOOK_PREINSTALL 在 user 已透過 MUI_PAGE_DIRECTORY 看 / 確認 path
;   後才跑,reset $INSTDIR 會造成 UI 顯示 vs 實際 install 路徑不一致。
;   .onGUIInit 是 NSIS 內建 callback,在所有 MUI page 之前跑,在這 reset
;   $INSTDIR 後 user 在 DIRECTORY page 看到的就是英文路徑,流程一致。
;
; 維持 productName 中文 → macOS Dock / Linux .desktop 顯示中文不變。
; 只覆蓋 Windows install dir 預設值,user 仍可在 DIRECTORY page 自選別處。
Function .onGUIInit
  StrCpy $INSTDIR "$LOCALAPPDATA\cix3752iLabelPrint"
FunctionEnd
