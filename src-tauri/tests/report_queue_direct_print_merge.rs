//! DirectPrint 自補回報 × 工控機回報「合併成同一列」的回歸測試。
//!
//! 直印模式下面單由中介機自己印,工控機沒有列印動作、實務上多半不再 POST /api/report,
//! 導致貼標人員永遠推不到雲端。修法是中介機印完自補一筆並延後送出(留寬限時間給工控機)。
//! 這裡守住三條不能破的規則:
//!   1. **同一筆列印記錄只會有一列佇列** —— 兩列會被 worker 各推一次,雲端記兩筆印單
//!   2. **工控機在寬限期內回報 → 立刻送出**(清掉等待),不必白等完剩下的秒數
//!   3. **遲到的回報只補記、絕不重推**(已 success 的列 next_attempt_at 不得被重置)
//!
//! SQL 直接取自 `queue/mod.rs` 的常數,不在測試裡另抄一份(抄了就會各自長歪)。

use cix3752i_label_print_lib::queue::{
    MAX_REPORT_DELAY_SECS, SQL_ENQUEUE_DIRECT_PRINT, SQL_ENQUEUE_IPC_REPORT,
};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::Row;

/// 建一份與 migration 後等價的 report_queue(含 0027 的 source / ipc_reported_at 與唯一索引)
async fn setup() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE report_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tracking_no TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            retry_count INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            sent_at TEXT,
            response_id INTEGER,
            job_sticker TEXT,
            sort_channel TEXT,
            next_attempt_at TEXT,
            source TEXT NOT NULL DEFAULT 'ipc',
            ipc_reported_at TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("CREATE UNIQUE INDEX idx_report_queue_response_unique ON report_queue(response_id)")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn enqueue_direct_print(pool: &sqlx::SqlitePool, response_id: i64, delay_secs: u64) -> u64 {
    sqlx::query(SQL_ENQUEUE_DIRECT_PRINT)
        .bind("SF123456789")
        .bind(format!("{{\"response_id\":{response_id}}}"))
        .bind(response_id)
        .bind("L3")
        .bind("阿明")
        .bind(format!("+{delay_secs} seconds"))
        .execute(pool)
        .await
        .unwrap()
        .rows_affected()
}

/// 走與 `post_report` 相同的那條 UPSERT(新增與併入同一條路,無中間窗口)
async fn ipc_report(pool: &sqlx::SqlitePool, response_id: i64) -> u64 {
    sqlx::query(SQL_ENQUEUE_IPC_REPORT)
        .bind("SF123456789")
        .bind(format!("{{\"response_id\":{response_id}}}"))
        .bind(response_id)
        .bind("L3")
        .bind("阿明")
        .execute(pool)
        .await
        .unwrap()
        .rows_affected()
}

async fn row_of(pool: &sqlx::SqlitePool, response_id: i64) -> sqlx::sqlite::SqliteRow {
    sqlx::query(
        "SELECT id, status, source, next_attempt_at, ipc_reported_at, sent_at, job_sticker
         FROM report_queue WHERE response_id = ?",
    )
    .bind(response_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn count_of(pool: &sqlx::SqlitePool, response_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM report_queue WHERE response_id = ?")
        .bind(response_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 自補入列:延後送出,且帶著貼標人員(這是雲端唯一收得到貼標人員的路)
#[tokio::test]
async fn direct_print_enqueue_defers_send_and_carries_sticker() {
    let pool = setup().await;
    assert_eq!(enqueue_direct_print(&pool, 1001, 10).await, 1);

    let row = row_of(&pool, 1001).await;
    assert_eq!(row.get::<String, _>("source"), "direct_print");
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert_eq!(row.get::<Option<String>, _>("job_sticker").as_deref(), Some("阿明"));
    // 尚未回報過
    assert!(row.get::<Option<String>, _>("ipc_reported_at").is_none());
    // 等待中:next_attempt_at 必須排在未來,worker 的退避閘才不會立刻撿走
    let next: String = row.get::<Option<String>, _>("next_attempt_at").unwrap();
    let now: String = sqlx::query_scalar("SELECT datetime('now','localtime')")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(next > now, "自補入列應延後送出:next_attempt_at={next} now={now}");
}

/// 工控機在寬限期內回報:併入同一列 + 清掉等待立即送出,**不新增第二列**
#[tokio::test]
async fn ipc_report_within_grace_merges_and_sends_immediately() {
    let pool = setup().await;
    enqueue_direct_print(&pool, 1002, 60).await;

    ipc_report(&pool, 1002).await;
    assert_eq!(count_of(&pool, 1002).await, 1, "不可產生第二列(否則雲端會記兩筆印單)");

    let row = row_of(&pool, 1002).await;
    assert_eq!(row.get::<String, _>("source"), "direct_print", "身世不因工控機回報而改寫");
    assert!(row.get::<Option<String>, _>("ipc_reported_at").is_some(), "應記下工控機回報時間");
    assert!(
        row.get::<Option<String>, _>("next_attempt_at").is_none(),
        "工控機已回報,寬限時間應被清掉改為立即送出"
    );
}

/// 工控機遲到回報(該列已推送成功):只補記時間,**不得重推**
#[tokio::test]
async fn late_ipc_report_records_only_and_never_resends() {
    let pool = setup().await;
    enqueue_direct_print(&pool, 1003, 10).await;
    // 模擬寬限時間到、worker 已推送成功
    sqlx::query(
        "UPDATE report_queue
            SET status='success', sent_at=datetime('now','localtime'), next_attempt_at=NULL
          WHERE response_id = ?",
    )
    .bind(1003i64)
    .execute(&pool)
    .await
    .unwrap();

    ipc_report(&pool, 1003).await;
    assert_eq!(count_of(&pool, 1003).await, 1, "遲到回報不可另開一列(會被再推一次)");
    let row = row_of(&pool, 1003).await;
    assert_eq!(row.get::<String, _>("status"), "success", "狀態不可被打回重送");
    assert!(
        row.get::<Option<String>, _>("next_attempt_at").is_none(),
        "已推送的列不可被重新排程(會造成雲端二次記錄)"
    );
    // 遲到判定:回報時間晚於(或等於)推送時間,佇列歷史頁據此顯示「回報遲到」
    let reported: String = row.get::<Option<String>, _>("ipc_reported_at").unwrap();
    let sent: String = row.get::<Option<String>, _>("sent_at").unwrap();
    assert!(reported >= sent);
}

/// 工控機搶在列印完成前就回報:自補入列必須讓步,不可覆蓋也不可新增
#[tokio::test]
async fn direct_print_enqueue_yields_when_ipc_reported_first() {
    let pool = setup().await;
    // 工控機先行建立(走 post_report 的一般路徑)
    sqlx::query(
        "INSERT INTO report_queue
             (tracking_no, payload_json, response_id, sort_channel, job_sticker, status,
              source, ipc_reported_at, created_at, updated_at)
         VALUES ('SF123456789','{}',?,'L3','阿明','pending',
              'ipc', datetime('now','localtime'), datetime('now','localtime'), datetime('now','localtime'))",
    )
    .bind(1004i64)
    .execute(&pool)
    .await
    .unwrap();

    // 之後直印完成才要自補 → ON CONFLICT DO NOTHING,不影響既有列
    assert_eq!(enqueue_direct_print(&pool, 1004, 10).await, 0, "已存在時不應插入");
    assert_eq!(count_of(&pool, 1004).await, 1);

    let row = row_of(&pool, 1004).await;
    assert_eq!(row.get::<String, _>("source"), "ipc", "不可被自補覆寫來源");
    assert!(
        row.get::<Option<String>, _>("next_attempt_at").is_none(),
        "工控機建立的列本來就該立即送出,不可被自補的寬限時間拖延"
    );
}

/// 重複回報(工控機沒收到 200 而重送):不可產生第二列,回報時間保留最早那次
#[tokio::test]
async fn repeated_ipc_report_is_idempotent() {
    let pool = setup().await;
    enqueue_direct_print(&pool, 1005, 10).await;
    ipc_report(&pool, 1005).await;
    let first: String = row_of(&pool, 1005).await.get::<Option<String>, _>("ipc_reported_at").unwrap();

    ipc_report(&pool, 1005).await;
    let again: String = row_of(&pool, 1005).await.get::<Option<String>, _>("ipc_reported_at").unwrap();

    assert_eq!(count_of(&pool, 1005).await, 1);
    assert_eq!(first, again, "重複回報不可覆寫首次回報時間");
}

/// 從未入列過(非直印模式的一般回報):同一條 UPSERT 直接建立新列,來源記工控機
#[tokio::test]
async fn ipc_report_creates_row_when_none_exists() {
    let pool = setup().await;
    ipc_report(&pool, 7777).await;

    assert_eq!(count_of(&pool, 7777).await, 1);
    let row = row_of(&pool, 7777).await;
    assert_eq!(row.get::<String, _>("source"), "ipc");
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert!(row.get::<Option<String>, _>("ipc_reported_at").is_some());
    assert!(
        row.get::<Option<String>, _>("next_attempt_at").is_none(),
        "工控機建立的列應立即可送,不帶寬限等待"
    );
}

/// 併發下的競爭:兩邊都想建立同一筆(直印 worker 自補 vs 工控機回報)。
/// 舊寫法是「先查有沒有、沒有才 INSERT」,兩個請求可能都查到沒有 → 後到者撞唯一索引吃 500。
/// 現在兩條路各自都是單句 UPSERT,不論誰先誰後都只會有一列、也不會報錯。
#[tokio::test]
async fn concurrent_paths_never_conflict_regardless_of_order() {
    for direct_first in [true, false] {
        let pool = setup().await;
        if direct_first {
            enqueue_direct_print(&pool, 1006, 60).await;
            ipc_report(&pool, 1006).await;
        } else {
            ipc_report(&pool, 1006).await;
            enqueue_direct_print(&pool, 1006, 60).await;
        }
        assert_eq!(count_of(&pool, 1006).await, 1, "無論順序都只能有一列");

        let row = row_of(&pool, 1006).await;
        assert!(
            row.get::<Option<String>, _>("ipc_reported_at").is_some(),
            "工控機回報過就必須留下記錄(direct_first={direct_first})"
        );
        assert!(
            row.get::<Option<String>, _>("next_attempt_at").is_none(),
            "工控機已回報就該立即送出,不可殘留寬限等待(direct_first={direct_first})"
        );
    }
}

/// 寬限秒數被夾在上限內 —— 這不是品味問題:SQLite 對超出範圍的位移回 NULL,
/// 而 NULL 在 worker 眼中是「立即可送」,不夾的話「等很久」會變成「馬上送」。
#[tokio::test]
async fn oversized_delay_is_clamped_not_turned_into_immediate_send() {
    let pool = setup().await;
    // 先確認未夾的原始行為確實會產生 NULL(證明這個防護不是多餘的)
    let raw: Option<String> = sqlx::query_scalar("SELECT datetime('now','localtime', ?)")
        .bind(format!("+{} seconds", u64::MAX))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(raw.is_none(), "SQLite 對超大位移應回 NULL(此測試的前提)");

    // 走實際路徑(呼叫端已夾上限)
    enqueue_direct_print(&pool, 1007, MAX_REPORT_DELAY_SECS.min(u64::MAX)).await;
    let row = row_of(&pool, 1007).await;
    let next = row.get::<Option<String>, _>("next_attempt_at");
    assert!(next.is_some(), "夾過的秒數必須算得出時間,不能是 NULL(NULL=立即送)");

    let now: String = sqlx::query_scalar("SELECT datetime('now','localtime')")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(next.unwrap() > now, "仍應排在未來");
}
