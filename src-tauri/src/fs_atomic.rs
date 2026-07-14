//! 面單快取檔的原子寫入:寫同目錄隱藏臨時檔 → `rename` 覆蓋 target。
//!
//! 用於「同時會被 `/images` 服務 / 工控機 / 浮水印 / 列印讀取」的面單快取檔。
//! 直接 `fs::write(target)` 會先 truncate 再寫,在空窗中被讀 → 讀到截斷檔
//!(printer `image::load_from_memory` 報 "unexpected end of file")。rename 同檔案系統為原子,
//! 讀取端永遠看到完整檔(舊檔或新檔)。
//!
//! 細節:
//! - 臨時檔名由 target 的 **`file_name()`** 衍生(不受 target 檔名內容/上游代碼影響),`.` 前綴隱藏,
//!   PID + 程序內遞增序號確保併發寫同一 target 不撞名。
//! - **寫入失敗**:清掉半寫入的臨時檔再回錯(不留孤兒,`keep_days=0` 關閉過期清理時尤重要)。
//! - **rename 失敗重試**:Windows 讀取端(SMB / 工控機)短暫持檔可能造成瞬時 sharing violation,
//!   短暫重試數次;最終仍失敗清臨時檔並回錯(由呼叫端 fallback / 告警,不靜默)。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static SEQ: AtomicU64 = AtomicU64::new(0);
/// rename 總嘗試次數(1 次 + 3 次重試);延遲 25ms × attempt,最壞約 150ms。
const RENAME_ATTEMPTS: u32 = 4;

/// target 同目錄的唯一隱藏臨時檔路徑(保留完整原檔名,不受 target 檔名內容影響)。
pub fn sibling_tmp(target: &Path) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("cache");
    target.with_file_name(format!(".{name}.part.{}.{}", std::process::id(), seq))
}

/// rename(非同步)含瞬時失敗重試。
pub async fn rename_with_retry_async(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 1u32;
    loop {
        match tokio::fs::rename(from, to).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt >= RENAME_ATTEMPTS {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(25 * attempt as u64)).await;
                attempt += 1;
            }
        }
    }
}

/// rename(同步)含瞬時失敗重試(供 watermark 等同步路徑)。
pub fn rename_with_retry_sync(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 1u32;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt >= RENAME_ATTEMPTS {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_millis(25 * attempt as u64));
                attempt += 1;
            }
        }
    }
}

/// 非同步原子寫入 bytes(cache 下載 / 錯誤面單用)。
pub async fn write_async(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = sibling_tmp(target);
    if let Err(e) = tokio::fs::write(&tmp, bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await; // 寫失敗:清臨時檔,不留孤兒
        return Err(e);
    }
    if let Err(e) = rename_with_retry_async(&tmp, target).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("cix_fsatomic_{}_{}", std::process::id(), seq));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 臨時檔為 target 的隱藏同目錄手足,且多次呼叫不撞名。
    #[test]
    fn sibling_tmp_is_hidden_unique_sibling() {
        let target = Path::new("/x/y/@error/A_B.png");
        let a = sibling_tmp(target);
        let b = sibling_tmp(target);
        assert_eq!(a.parent(), target.parent(), "臨時檔須與 target 同目錄(rename 才原子)");
        assert_ne!(a, b, "併發不可撞名");
        assert!(a.file_name().unwrap().to_str().unwrap().starts_with('.'), "隱藏檔");
    }

    /// 成功寫入 → target 內容正確,且不留任何 .part 孤兒;覆寫也正確。
    #[tokio::test]
    async fn write_async_atomic_and_no_orphan() {
        let dir = temp_dir();
        let target = dir.join("label.png");
        write_async(&target, b"first").await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        write_async(&target, b"second-longer").await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second-longer");
        // 目錄內只該有 target,無 .part 殘留
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_str().unwrap().contains(".part."))
            .collect();
        assert!(leftovers.is_empty(), "不可殘留 .part 臨時檔: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
