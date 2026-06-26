use tauri::{AppHandle, State};

use crate::{AppResult, SharedState};

/// **即時**調整數位變焦(不重啟相機擷取、不寫設定檔)。供「相機預覽對話框」拖滑桿時呼叫 ——
/// 後端下一幀就套用,MJPEG 預覽串流立刻反映新框景。要永久保存仍需把 `camera.zoom`
/// 寫進設定(走 `update_config`),否則 App 重啟會回到設定檔裡的值。
#[tauri::command]
pub async fn camera_set_zoom(state: State<'_, SharedState>, zoom: f32) -> AppResult<()> {
    state.camera.set_zoom(zoom);
    Ok(())
}

/// 手動拍一張:抓相機當下最新一幀(含已套用 zoom)存進存證目錄,回傳相對 key(`MANUAL_時間.jpg`)。
/// 相機未啟用 / 尚無幀時回 `None`。供對位時測試拍照、確認存檔管線通暢。存到的目錄與工控機查件存證同一處。
#[tauri::command]
pub async fn camera_capture_now(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> AppResult<Option<String>> {
    let jpeg = match state.camera.latest_jpeg() {
        Some(j) => j,
        None => return Ok(None), // 相機未啟用 / 尚未取到幀
    };
    let captures_dir = state.config.read().await.resolved_captures_dir(&app)?;
    let key = tauri::async_runtime::spawn_blocking(move || {
        crate::camera::save_snapshot(&captures_dir, "MANUAL", &jpeg)
    })
    .await
    .ok()
    .flatten();
    Ok(key)
}
