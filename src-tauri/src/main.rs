#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// macOS dev binary 沒 .app bundle,把最小 Info.plist 內嵌進 __TEXT,__info_plist,
// 讓 Dock 顯示「智配通 LabelPrint」而非 binary 檔名
#[cfg(target_os = "macos")]
embed_plist::embed_info_plist!("../Info.plist");

fn main() {
    cix3752i_label_print_lib::run()
}
