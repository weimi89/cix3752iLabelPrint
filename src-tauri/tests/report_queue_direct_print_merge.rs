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
    MAX_REPORT_DELAY_SECS, SQL_CANCEL_REPORT_ON_PRINT_FAILURE, SQL_CLAIM_FOR_SENDING,
    SQL_ENQUEUE_DIRECT_PRINT, SQL_ENQUEUE_IPC_REPORT, SQL_FINISH_CANCELLED,
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
            ipc_reported_at TEXT,
            cancel_requested INTEGER NOT NULL DEFAULT 0
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

/// 走與 `report_direct_print_failed` 相同的攔截路徑,回傳攔截後的最終狀態
async fn cancel_on_print_failure(pool: &sqlx::SqlitePool, response_id: i64) -> String {
    sqlx::query_scalar(SQL_CANCEL_REPORT_ON_PRINT_FAILURE)
        .bind("SF123456789")
        .bind(format!("{{\"response_id\":{response_id}}}"))
        .bind(response_id)
        .bind("直印失敗(print_failed),已攔下雲端回報")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// worker 實際會撿去推送的條件(與 queue/mod.rs 的 process_once 同義):
/// 用來斷言「被攔下的那筆真的不會被送出」,而不只是斷言欄位值長得對
async fn is_sendable(pool: &sqlx::SqlitePool, response_id: i64) -> bool {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM report_queue
          WHERE response_id = ?
            AND retry_count < 10
            AND cancel_requested = 0
            AND (status IN ('pending','failed')
                 OR (status = 'sending' AND updated_at < datetime('now','localtime','-60 seconds')))
            AND (next_attempt_at IS NULL OR next_attempt_at <= datetime('now','localtime'))",
    )
    .bind(response_id)
    .fetch_one(pool)
    .await
    .unwrap();
    n > 0
}

async fn row_of(pool: &sqlx::SqlitePool, response_id: i64) -> sqlx::sqlite::SqliteRow {
    sqlx::query(
        "SELECT id, status, source, next_attempt_at, ipc_reported_at, sent_at, job_sticker, last_error
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

    // 之後直印完成才要自補 → 撞到既有列,不得新增、也不得改寫既有狀態
    enqueue_direct_print(&pool, 1004, 10).await;
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

// ── 直印失敗時攔下雲端回報(面單沒印出來,雲端不該記成完成)────────────────────

/// 工控機搶先回報、但列印最後失敗:那筆待送的回報必須被攔下,不可送去雲端
#[tokio::test]
async fn print_failure_blocks_pending_report_from_ipc() {
    let pool = setup().await;
    ipc_report(&pool, 2001).await; // 工控機先回報(還沒印完)
    assert!(is_sendable(&pool, 2001).await, "前提:這筆本來會被 worker 送出");

    assert_eq!(cancel_on_print_failure(&pool, 2001).await, "cancelled");
    assert!(!is_sendable(&pool, 2001).await, "攔下後 worker 不可再撿去推送");
    assert_eq!(count_of(&pool, 2001).await, 1);
}

/// 直印失敗當下還沒有任何回報:必須先立墓碑,否則工控機稍後才回報會變成一筆全新的待送
#[tokio::test]
async fn print_failure_tombstone_blocks_later_ipc_report() {
    let pool = setup().await;
    assert_eq!(cancel_on_print_failure(&pool, 2002).await, "cancelled", "無既有列時應立墓碑");

    // 工控機稍後才回報 —— 撞上墓碑,只留下回報時間,不得復活成待送
    ipc_report(&pool, 2002).await;
    assert_eq!(count_of(&pool, 2002).await, 1);
    let row = row_of(&pool, 2002).await;
    assert_eq!(row.get::<String, _>("status"), "cancelled", "遲來的回報不可讓被攔下的件復活");
    assert!(row.get::<Option<String>, _>("ipc_reported_at").is_some(), "仍應記下工控機有回報過");
    assert!(!is_sendable(&pool, 2002).await, "攔下的件永遠不該被送出");
}

/// 直印自補後才失敗(例如佇列積壓時印表機出事):自補那筆同樣要被攔下
#[tokio::test]
async fn print_failure_blocks_self_reported_row() {
    let pool = setup().await;
    enqueue_direct_print(&pool, 2003, 10).await;
    assert_eq!(cancel_on_print_failure(&pool, 2003).await, "cancelled");
    assert!(!is_sendable(&pool, 2003).await);
    let row = row_of(&pool, 2003).await;
    assert!(
        row.get::<Option<String>, _>("last_error").unwrap_or_default().contains("直印失敗"),
        "應留下攔截原因供佇列歷史頁查證"
    );
}

/// 已經推送成功才發現列印失敗:**不可竄改既有狀態**,回報收不回來,
/// 呼叫端據此發高等級告警要現場人工處理
#[tokio::test]
async fn print_failure_does_not_rewrite_already_sent_report() {
    let pool = setup().await;
    ipc_report(&pool, 2004).await;
    sqlx::query(
        "UPDATE report_queue SET status='success', sent_at=datetime('now','localtime') WHERE response_id = ?",
    )
    .bind(2004i64)
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(
        cancel_on_print_failure(&pool, 2004).await,
        "success",
        "已送出的必須回報 success(呼叫端據此判定收不回來)"
    );
    let row = row_of(&pool, 2004).await;
    assert_eq!(row.get::<String, _>("status"), "success", "不可把已完成的改成已攔下");
    assert!(row.get::<Option<String>, _>("sent_at").is_some(), "推送時間不可被抹掉");
}

/// 重試「失敗」項目時不可把被攔下的件一起復活(retry 只針對 failed / sending)
#[tokio::test]
async fn retry_failed_does_not_revive_cancelled_rows() {
    let pool = setup().await;
    cancel_on_print_failure(&pool, 2005).await;

    // 與 queue_commands::queue_retry_failed 同一條件
    sqlx::query(
        "UPDATE report_queue
            SET status='pending', retry_count=0, next_attempt_at=NULL, updated_at=datetime('now','localtime')
          WHERE status IN ('failed', 'sending')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(row_of(&pool, 2005).await.get::<String, _>("status"), "cancelled");
    assert!(!is_sendable(&pool, 2005).await, "手動重試不該讓刻意攔下的件被送出");
}

// ── 攔截與推送的時序競態(覆檢指出這三條原本零覆蓋)────────────────────────────

/// 走與 worker 相同的 claim 語句,回傳有沒有真的搶到這筆
async fn claim_for_sending(pool: &sqlx::SqlitePool, response_id: i64) -> bool {
    let id: i64 = sqlx::query_scalar("SELECT id FROM report_queue WHERE response_id = ?")
        .bind(response_id)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(SQL_CLAIM_FOR_SENDING)
        .bind(id)
        .execute(pool)
        .await
        .unwrap()
        .rows_affected()
        > 0
}

/// 走與 worker 推送失敗時相同的收斂語句
async fn finish_cancelled(pool: &sqlx::SqlitePool, response_id: i64) -> bool {
    let id: i64 = sqlx::query_scalar("SELECT id FROM report_queue WHERE response_id = ?")
        .bind(response_id)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(SQL_FINISH_CANCELLED)
        .bind(id)
        .execute(pool)
        .await
        .unwrap()
        .rows_affected()
        > 0
}

/// **claim 競態**:worker 選出待送清單後、真正下手前,列印那邊剛好判定失敗。
/// claim 必須是帶條件的 compare-and-set —— 無條件 UPDATE 會把 cancelled 蓋回 sending 照樣送出。
#[tokio::test]
async fn claim_must_not_resurrect_a_row_cancelled_after_selection() {
    let pool = setup().await;
    ipc_report(&pool, 3001).await;
    assert!(is_sendable(&pool, 3001).await, "前提:worker 這一輪會選中它");

    // 選中之後、claim 之前 —— 列印判定失敗
    cancel_on_print_failure(&pool, 3001).await;

    assert!(!claim_for_sending(&pool, 3001).await, "已被攔截的不可被 claim 去推送");
    assert_eq!(row_of(&pool, 3001).await.get::<String, _>("status"), "cancelled",
        "claim 不可把 cancelled 蓋回 sending");
}

/// **推送中被攔截 + 這次推送失敗**:不可走一般退避重試(重試會在不知情下把它送出去),
/// 必須直接定案 cancelled。
#[tokio::test]
async fn cancel_during_sending_then_failure_settles_as_cancelled() {
    let pool = setup().await;
    ipc_report(&pool, 3002).await;
    assert!(claim_for_sending(&pool, 3002).await);
    assert_eq!(row_of(&pool, 3002).await.get::<String, _>("status"), "sending");

    // webhook 進行中,列印判定失敗 —— 此刻不可斷言送達,只記下攔截意圖
    assert_eq!(cancel_on_print_failure(&pool, 3002).await, "sending",
        "推送中不可被改狀態(結果未定),呼叫端據此回報 SendInProgress");

    // 這次推送最終失敗 → 收斂為 cancelled,不再重試
    assert!(finish_cancelled(&pool, 3002).await);
    assert_eq!(row_of(&pool, 3002).await.get::<String, _>("status"), "cancelled");
    assert!(!is_sendable(&pool, 3002).await, "定案後不可再被撿去重試");
}

/// 推送中被攔截、但這次推送**成功**了:狀態誠實維持 success(收不回來),
/// 攔截旗標保留供呼叫端發「無法撤回」告警;不可竄改成 cancelled 假裝沒送出去。
#[tokio::test]
async fn cancel_during_sending_then_success_stays_success() {
    let pool = setup().await;
    ipc_report(&pool, 3003).await;
    claim_for_sending(&pool, 3003).await;
    cancel_on_print_failure(&pool, 3003).await;

    // worker 拿到成功結果
    sqlx::query("UPDATE report_queue SET status='success', sent_at=datetime('now','localtime') WHERE response_id=?")
        .bind(3003i64)
        .execute(&pool)
        .await
        .unwrap();

    let flagged: i64 = sqlx::query_scalar("SELECT cancel_requested FROM report_queue WHERE response_id=?")
        .bind(3003i64)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(flagged, 1, "旗標須保留,呼叫端據此發出「已送出無法撤回」告警");
    assert_eq!(row_of(&pool, 3003).await.get::<String, _>("status"), "success",
        "已送達就是已送達,不可竄改成已攔下");
}

/// **重印成功要能解除攔截**:同一筆列印記錄先失敗被攔下,之後又印成功,
/// 那筆「確實印出來的」必須能被送到雲端,不可永遠卡在已攔下。
#[tokio::test]
async fn successful_reprint_revives_a_cancelled_row() {
    let pool = setup().await;
    cancel_on_print_failure(&pool, 3004).await;
    assert!(!is_sendable(&pool, 3004).await, "前提:已被攔下");

    // 同一 response_id 重印成功 → 自補路徑必須解除攔截並重新排入
    enqueue_direct_print(&pool, 3004, 0).await;

    let row = row_of(&pool, 3004).await;
    assert_eq!(row.get::<String, _>("status"), "pending", "重印成功應解除已攔下");
    let flagged: i64 = sqlx::query_scalar("SELECT cancel_requested FROM report_queue WHERE response_id=?")
        .bind(3004i64)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(flagged, 0, "攔截旗標必須一併清掉,否則 worker 仍不會撿");
    assert!(is_sendable(&pool, 3004).await, "解除後必須真的能被送出");
    assert_eq!(count_of(&pool, 3004).await, 1);
}

/// 啟動回收殘留 sending 時,被攔截的不可被回收成 pending(那等於讓它重新排隊送出)
#[tokio::test]
async fn startup_recovery_does_not_requeue_cancelled_rows() {
    let pool = setup().await;
    ipc_report(&pool, 3005).await;
    claim_for_sending(&pool, 3005).await;
    cancel_on_print_failure(&pool, 3005).await; // 推送中被攔截,狀態仍是 sending

    // 模擬 App 崩潰重啟後的回收(與 recover_stale_sending 同一組語句)
    sqlx::query("UPDATE report_queue SET status='cancelled', next_attempt_at=NULL WHERE status='sending' AND cancel_requested=1")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE report_queue SET status='pending', next_attempt_at=NULL WHERE status='sending' AND cancel_requested=0")
        .execute(&pool).await.unwrap();

    assert_eq!(row_of(&pool, 3005).await.get::<String, _>("status"), "cancelled");
    assert!(!is_sendable(&pool, 3005).await, "重啟後不可讓被攔截的件重新排隊");
}

// ── 覆檢第二輪挖出的兩個窄縫(原本零覆蓋)──────────────────────────────────

/// **不可留下「失敗卻帶著攔截旗標」的幽靈列**:worker 不撿它(對),但操作員按「重試失敗」
/// 會讓它看起來重新排隊、實際永遠送不出去 —— 比卡在失敗更難察覺。
/// 推送失敗的寫回撞上期間才發出的攔截請求時,必須直接定案 cancelled。
#[tokio::test]
async fn failed_writeback_racing_a_cancel_settles_instead_of_becoming_a_ghost() {
    let pool = setup().await;
    ipc_report(&pool, 4001).await;
    claim_for_sending(&pool, 4001).await;
    cancel_on_print_failure(&pool, 4001).await; // 推送中被攔截(狀態仍 sending)

    // mark_failed 的寫回(帶 cancel_requested=0 條件)—— 應該寫不進去
    let affected = sqlx::query(
        "UPDATE report_queue
            SET status='failed', retry_count=1, last_error='x',
                next_attempt_at=datetime('now','localtime','+5 seconds'),
                updated_at=datetime('now','localtime')
          WHERE id=(SELECT id FROM report_queue WHERE response_id=?) AND cancel_requested=0",
    )
    .bind(4001i64)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(affected, 0, "被攔截的件不可被寫成 failed(那會變成幽靈列)");

    // 寫不進去 → 收斂成 cancelled
    assert!(finish_cancelled(&pool, 4001).await);
    assert_eq!(row_of(&pool, 4001).await.get::<String, _>("status"), "cancelled");
}

/// 「重試失敗」按鈕不可把被攔截的件轉成待送 —— 轉了會顯示成已重新排隊卻永遠送不出
#[tokio::test]
async fn retry_failed_button_skips_flagged_rows_entirely() {
    let pool = setup().await;
    // 造一筆「失敗且帶攔截旗標」的列(舊版可能殘留的幽靈列)
    sqlx::query(
        "INSERT INTO report_queue (tracking_no,payload_json,response_id,status,source,cancel_requested,created_at,updated_at)
         VALUES ('SF1','{}',?,'failed','direct_print',1,datetime('now','localtime'),datetime('now','localtime'))",
    )
    .bind(4002i64)
    .execute(&pool)
    .await
    .unwrap();

    // 與 queue_retry_failed 同一條件(已加 cancel_requested=0)
    sqlx::query(
        "UPDATE report_queue
            SET status='pending', retry_count=0, next_attempt_at=NULL, updated_at=datetime('now','localtime')
          WHERE status IN ('failed','sending') AND cancel_requested=0",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(row_of(&pool, 4002).await.get::<String, _>("status"), "failed",
        "被攔截的件不該被重試按鈕改成待送(改了會看起來在跑卻永遠不動)");
    assert!(!is_sendable(&pool, 4002).await);
}

/// 重印成功時,若前次的攔截還卡在 `sending` 尚未收斂(中繼態),旗標也必須放掉,
/// 否則那筆收斂之後會永遠沒有人送。
#[tokio::test]
async fn reprint_clears_cancel_flag_even_while_previous_send_unsettled() {
    let pool = setup().await;
    ipc_report(&pool, 4003).await;
    claim_for_sending(&pool, 4003).await;
    cancel_on_print_failure(&pool, 4003).await;
    assert_eq!(row_of(&pool, 4003).await.get::<String, _>("status"), "sending", "前提:停在中繼態");

    // 同一 response_id 重印成功 → 自補路徑
    enqueue_direct_print(&pool, 4003, 0).await;

    let flagged: i64 = sqlx::query_scalar("SELECT cancel_requested FROM report_queue WHERE response_id=?")
        .bind(4003i64)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(flagged, 0, "重印成功已推翻前次失敗結論,旗標必須放掉");
    assert_eq!(row_of(&pool, 4003).await.get::<String, _>("status"), "sending",
        "推送結果未定,狀態不可被搶改");
    assert_eq!(count_of(&pool, 4003).await, 1);
}
