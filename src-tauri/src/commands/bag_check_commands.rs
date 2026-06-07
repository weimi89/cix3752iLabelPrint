//! 分揀袋件核對 commands
//!
//! 清單為後端常駐記憶體狀態(bag_check module 維護),由工控機 GET /api/parcel
//! 事件驅動更新並 emit `bag-check-updated`。這裡只提供「讀快照(切頁保留)」與
//! 「清除」兩支同步 command。

use tauri::State;

use crate::bag_check::BagEntry;
use crate::{AppResult, SharedState};

/// 讀取當前袋件核對清單快照(切頁回來不必重查雲端)
#[tauri::command]
pub fn bag_check_snapshot(state: State<'_, SharedState>) -> AppResult<Vec<BagEntry>> {
    Ok(state.bag_check.snapshot())
}

/// 清空袋件核對清單(前端「清除清單」鈕)
#[tauri::command]
pub fn bag_check_clear(state: State<'_, SharedState>) -> AppResult<()> {
    state.bag_check.clear();
    Ok(())
}
