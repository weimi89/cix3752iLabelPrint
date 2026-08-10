use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::cloud::CloudClient;
use crate::db::DbPool;
use crate::event_log;
use crate::models::ReportPayload;
use crate::{AppError, AppResult};

/// 推送最大重試次數,達到後放棄(status=failed 並寫 last_error,以 error 級事件留痕,不靜默)
const MAX_RETRY: i64 = 10;

/// 直印自補等工控機回報的**上限秒數**(10 分鐘)。
/// 不只是「設太久不合理」的品味問題:SQLite `datetime()` 的位移超出可表達範圍會回 NULL,
/// 而 NULL 對 worker 代表「立即可送」—— 不夾範圍的話,設定值一大反而變成秒推。
pub const MAX_REPORT_DELAY_SECS: u64 = 600;

/// DirectPrint 自補入列的 SQL(見 [`QueueManager::enqueue_direct_print`])。
/// 提為常數是為了讓回歸測試打到**同一份語句**,不必複製一份在測試裡各自長歪。
/// bind 順序:tracking_no / payload_json / response_id / sort_channel / job_sticker / 延後修飾字
/// 衝突時原則上不動既有列(那代表工控機搶先回報過,以它為準),
/// **唯一例外是先前被攔下的件**:同一筆列印記錄後來又印成功了,說明前一次的失敗結論已被推翻,
/// 必須連同攔截旗標一起解除、重新排入等待,否則這筆「確實印出來的」會永遠卡在已攔下、雲端收不到。
pub const SQL_ENQUEUE_DIRECT_PRINT: &str = "INSERT INTO report_queue
     (tracking_no, payload_json, response_id, sort_channel, job_sticker, status,
      source, next_attempt_at, created_at, updated_at)
 VALUES (?, ?, ?, ?, ?, 'pending',
      'direct_print', datetime('now','localtime',?), datetime('now','localtime'), datetime('now','localtime'))
 ON CONFLICT(response_id) DO UPDATE SET
      status = CASE WHEN report_queue.status='cancelled' THEN 'pending' ELSE report_queue.status END,
      -- 旗標的解除範圍比 status 更寬:除了已定案的 cancelled,也涵蓋「攔截已發出、
      -- 但那次推送還卡在 sending 沒收斂」的中繼態 —— 這次列印成功已推翻前次失敗結論,
      -- 此時不能改 status(推送結果未定),但旗標必須放掉,否則那筆收斂後會永遠沒人送。
      cancel_requested = CASE WHEN report_queue.status='cancelled'
                                OR (report_queue.status='sending' AND report_queue.cancel_requested=1)
                              THEN 0 ELSE report_queue.cancel_requested END,
      retry_count = CASE WHEN report_queue.status='cancelled' THEN 0 ELSE report_queue.retry_count END,
      last_error = CASE WHEN report_queue.status='cancelled' THEN NULL ELSE report_queue.last_error END,
      next_attempt_at = CASE WHEN report_queue.status='cancelled'
                             THEN excluded.next_attempt_at ELSE report_queue.next_attempt_at END,
      job_sticker = COALESCE(report_queue.job_sticker, excluded.job_sticker),
      sort_channel = COALESCE(report_queue.sort_channel, excluded.sort_channel),
      updated_at = excluded.updated_at";

/// 直印失敗時攔下該件雲端回報的 SQL(見 [`QueueManager::cancel_report_on_print_failure`])。
///
/// 面單沒印出來,雲端就不該收到「這單完成了」。攔截有三種局面,一句 UPSERT 全包:
/// - **還沒送出**(pending/failed,可能是自補、也可能是工控機搶先回報建立的)→ 轉 `cancelled`,worker 不再撿
/// - **已送出或正在送**(success/sending)→ 保持原狀,收不回來了;呼叫端據回傳值發出高等級告警
/// - **還沒有任何回報**→ 直接建一筆 `cancelled` 墓碑。**這是關鍵的一種**:
///   工控機若稍後才回報,[`SQL_ENQUEUE_IPC_REPORT`] 會撞上這筆墓碑而只記回報時間、
///   不會把它變回待送 —— 少了墓碑,那筆回報就會變成一列全新的 pending 被推去雲端。
///
/// bind 順序:tracking_no / payload_json / response_id / last_error
pub const SQL_CANCEL_REPORT_ON_PRINT_FAILURE: &str = "INSERT INTO report_queue
     (tracking_no, payload_json, response_id, status, source, last_error, cancel_requested, created_at, updated_at)
 VALUES (?, ?, ?, 'cancelled', 'direct_print', ?, 1, datetime('now','localtime'), datetime('now','localtime'))
 ON CONFLICT(response_id) DO UPDATE SET
      -- 正在推送(sending)時**不能**直接改狀態:webhook 結果還沒回來,改了會與 process_row 的寫回打架。
      -- 改用 cancel_requested 記下意圖,由 process_row 拿到結果後自行收斂(見 SQL_FINISH_CANCELLED)。
      status = CASE WHEN report_queue.status IN ('pending','failed')
                    THEN 'cancelled' ELSE report_queue.status END,
      -- 旗標一律設起來:不論此刻是 pending / sending / 甚至已 success,
      -- 「這件的列印失敗了」都是既成事實,後續任何重試都必須看得到它
      cancel_requested = 1,
      last_error = CASE WHEN report_queue.status = 'success'
                        THEN report_queue.last_error ELSE excluded.last_error END,
      next_attempt_at = NULL,
      updated_at = excluded.updated_at
 RETURNING status";

/// 推送**失敗後**收斂被攔截的項目(見 [`process_row`])。
/// 這次 webhook 沒送成,而中途已被要求攔截 → 直接定案 `cancelled`,不進退避重試。
/// 少了這條,重試邏輯會在不知情的狀況下把「列印失敗的件」重送成功。
/// bind 順序:id
pub const SQL_FINISH_CANCELLED: &str = "UPDATE report_queue
    SET status='cancelled', next_attempt_at=NULL, updated_at=datetime('now','localtime')
  WHERE id=? AND cancel_requested=1";

/// worker claim 一筆來推送(見 [`process_row`])。
///
/// **必須是帶條件的 compare-and-set**:`process_once` 選出 id 到這裡真正下手之間,
/// 列印那邊可能剛好判定失敗把它攔下 —— 無條件 `WHERE id=?` 會把 `cancelled` 直接蓋回 `sending`
/// 並照常送出,攔截等於形同虛設。用 `rows_affected` 判斷有沒有真的搶到,搶不到就跳過。
/// bind 順序:id
pub const SQL_CLAIM_FOR_SENDING: &str = "UPDATE report_queue
    SET status='sending', updated_at=datetime('now','localtime')
  WHERE id=?
    AND cancel_requested=0
    AND (status IN ('pending','failed')
         OR (status='sending' AND updated_at < datetime('now','localtime','-60 seconds')))";

/// 工控機回報入列的 SQL(見 [`QueueManager::enqueue_ipc_report`])。
///
/// **刻意寫成單一 UPSERT 而非「先 UPDATE 沒中再 INSERT」**:後者在
/// 「工控機重送」與「直印 worker 自補」同時發生時,兩邊可能都判定沒中而各自 INSERT,
/// 後到的那個撞上 response_id 唯一索引 → 合法回報收到 HTTP 500(工控機只好重試)。
/// 一句 UPSERT 由 SQLite 保證原子,新增與併入走同一條路,沒有中間窗口。
///
/// 衝突(該筆已由直印自補建立)時:
/// - `ipc_reported_at` 只在還沒記過時才寫入 —— 重複回報保留**首次**時間
/// - 還沒送出(pending/failed)就清掉寬限等待,讓 worker 下一輪立即送出;
///   已送出(success/sending)則保持原樣 —— **絕不重推**,該次屬「遲到回報」,
///   由 `ipc_reported_at > sent_at` 判定
/// - `sort_channel` / `job_sticker` 只在既有值為空時補上(自補當下查得到就以它為準,不覆寫)
/// - `source` 保持不變:這筆確實是直印自補建立的,不因工控機回報而改寫身世
///
/// bind 順序:tracking_no / payload_json / response_id / sort_channel / job_sticker
pub const SQL_ENQUEUE_IPC_REPORT: &str = "INSERT INTO report_queue
     (tracking_no, payload_json, response_id, sort_channel, job_sticker, status,
      source, ipc_reported_at, created_at, updated_at)
 VALUES (?, ?, ?, ?, ?, 'pending',
      'ipc', datetime('now','localtime'), datetime('now','localtime'), datetime('now','localtime'))
 ON CONFLICT(response_id) DO UPDATE SET
      ipc_reported_at = COALESCE(report_queue.ipc_reported_at, excluded.ipc_reported_at),
      next_attempt_at = CASE WHEN report_queue.status IN ('pending','failed')
                             THEN NULL ELSE report_queue.next_attempt_at END,
      sort_channel = COALESCE(report_queue.sort_channel, excluded.sort_channel),
      job_sticker  = COALESCE(report_queue.job_sticker,  excluded.job_sticker),
      updated_at = excluded.updated_at";

/// 工控機回報佇列管理 — 寫入即回 200 給工控機,背景 worker 嘗試推送雲端
#[derive(Clone)]
pub struct QueueManager {
    inner: Arc<Inner>,
}

struct Inner {
    db: DbPool,
    cloud: CloudClient,
}

/// 直印失敗時試圖攔下雲端回報的結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// 攔下了 —— 這筆不會被推去雲端(含「先立墓碑,擋掉工控機稍後才來的回報」)
    Blocked,
    /// 推送正在進行中 —— 攔截旗標已設下,最終結果由 worker 收斂:
    /// 這次推送失敗就定案攔下、成功則轉為無法撤回並告警。**此刻不可斷言送達與否。**
    SendInProgress,
    /// 已經推出去了 —— 雲端已記成完成,收不回來,只能告警要現場人工處理
    AlreadySent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub pending: i64,
    pub sending: i64,
    pub success: i64,
    pub failed: i64,
    /// 因列印失敗而被攔下、刻意不推送雲端的件數。
    /// **必須單獨計數**:少了它,直印失敗攔截發生過幾次在儀表板上完全看不出來。
    pub cancelled: i64,
}

impl QueueManager {
    pub fn new(db: DbPool, cloud: CloudClient) -> Self {
        Self {
            inner: Arc::new(Inner { db, cloud }),
        }
    }

    /// 寫入(或併入)一筆工控機回報;status 寫 pending,等 worker 推送雲端。
    /// sort_channel / job_sticker 由 server 端反查後傳入,方便 QueueLogPage 直接顯示。
    ///
    /// 直印模式下該筆可能已由 [`Self::enqueue_direct_print`] 先建立,此時**併入同一列**
    /// 而非新增第二列(否則同一件會被推兩次);併入規則見 [`SQL_ENQUEUE_IPC_REPORT`]。
    pub async fn enqueue_ipc_report(
        &self,
        payload: &ReportPayload,
        tracking_no: &str,
        sort_channel: Option<&str>,
        job_sticker: Option<&str>,
    ) -> AppResult<()> {
        let json = serde_json::to_string(payload)?;
        sqlx::query(SQL_ENQUEUE_IPC_REPORT)
            .bind(tracking_no)
            .bind(&json)
            .bind(payload.response_id)
            .bind(sort_channel)
            .bind(job_sticker)
            .execute(&self.inner.db)
            .await?;
        Ok(())
    }

    /// DirectPrint 模式:中介機自己印完面單後補記一筆回報,**延後 `delay_secs` 秒才推雲端**。
    ///
    /// 直印模式下工控機沒有列印動作,多半也就不回報,這條是唯一能把貼標人員送到雲端的路。
    /// 但不能立刻送:工控機的回報才代表「它收到了分揀通道、包裹確實被分進格口」,
    /// 先等一段寬限時間讓它有機會回報([`Self::enqueue_ipc_report`] 會把等待清掉改為立即送出),
    /// 等不到才由這筆兜底送出。**推送前只有一列記錄,因此無論哪邊先到都只會送一次。**
    ///
    /// 該筆已存在時不重複建立(工控機搶先回報過,以它為準);
    /// 但若它先前因列印失敗被攔下、這次印成功了,會連同攔截旗標一起解除(見 [`SQL_ENQUEUE_DIRECT_PRINT`])。
    pub async fn enqueue_direct_print(
        &self,
        response_id: i64,
        tracking_no: &str,
        sort_channel: Option<&str>,
        job_sticker: Option<&str>,
        delay_secs: u64,
    ) -> AppResult<()> {
        let json = serde_json::to_string(&ReportPayload { response_id })?;
        // next_attempt_at 沿用既有的「退避閘」(worker 只撿到期項目),不必另做排程。
        // **秒數必須夾在合理範圍**:SQLite 的 datetime() 對超出可表達範圍的位移會回 NULL,
        // 而 NULL 在 worker 眼中等於「立即可送」—— 設一個荒謬大的等待反而變成秒推,語意完全相反。
        let delay_modifier = format!("+{} seconds", delay_secs.min(MAX_REPORT_DELAY_SECS));
        let res = sqlx::query(SQL_ENQUEUE_DIRECT_PRINT)
        .bind(tracking_no)
        .bind(&json)
        .bind(response_id)
        .bind(sort_channel)
        .bind(job_sticker)
        .bind(&delay_modifier)
        .execute(&self.inner.db)
        .await?;
        let _ = res;
        Ok(())
    }

    /// 直印失敗時攔下該件的雲端回報 —— **面單沒印出來,雲端就不該記成完成**。
    ///
    /// 回傳攔截結果供呼叫端決定告警等級,規則見 [`SQL_CANCEL_REPORT_ON_PRINT_FAILURE`]。
    pub async fn cancel_report_on_print_failure(
        &self,
        response_id: i64,
        tracking_no: &str,
        reason: &str,
    ) -> AppResult<CancelOutcome> {
        let json = serde_json::to_string(&ReportPayload { response_id })?;
        let status: String = sqlx::query_scalar(SQL_CANCEL_REPORT_ON_PRINT_FAILURE)
            .bind(tracking_no)
            .bind(&json)
            .bind(response_id)
            .bind(format!("直印失敗({reason}),已攔下雲端回報"))
            .fetch_one(&self.inner.db)
            .await?;
        Ok(match status.as_str() {
            "cancelled" => CancelOutcome::Blocked,
            // 推送中:webhook 結果未定,不能斷言已送達。旗標已設起,
            // process_row 拿到結果後會自行收斂(失敗→定案攔下;成功→發「無法撤回」告警)
            "sending" => CancelOutcome::SendInProgress,
            // 已送達雲端:收不回來,只能靠告警讓現場知道要人工處理
            _ => CancelOutcome::AlreadySent,
        })
    }

    /// 透過 queue_id 反查當初工控機回報時帶的 response_id
    pub async fn lookup_response_id(&self, queue_id: i64) -> AppResult<Option<i64>> {
        let row = sqlx::query("SELECT response_id FROM report_queue WHERE id = ?")
            .bind(queue_id)
            .fetch_optional(&self.inner.db)
            .await?;
        Ok(row.and_then(|r| r.try_get::<Option<i64>, _>("response_id").ok().flatten()))
    }

    /// 取得佇列統計
    pub async fn stats(&self) -> AppResult<QueueStats> {
        let row = sqlx::query(
            "SELECT
                 COALESCE(SUM(CASE WHEN status='pending' THEN 1 ELSE 0 END), 0) AS pending,
                 COALESCE(SUM(CASE WHEN status='sending' THEN 1 ELSE 0 END), 0) AS sending,
                 COALESCE(SUM(CASE WHEN status='success' THEN 1 ELSE 0 END), 0) AS success,
                 COALESCE(SUM(CASE WHEN status='failed'  THEN 1 ELSE 0 END), 0) AS failed,
                 COALESCE(SUM(CASE WHEN status='cancelled' THEN 1 ELSE 0 END), 0) AS cancelled
             FROM report_queue",
        )
        .fetch_one(&self.inner.db)
        .await?;

        Ok(QueueStats {
            pending: row.try_get("pending").unwrap_or(0),
            sending: row.try_get("sending").unwrap_or(0),
            success: row.try_get("success").unwrap_or(0),
            failed: row.try_get("failed").unwrap_or(0),
            cancelled: row.try_get("cancelled").unwrap_or(0),
        })
    }

    /// 啟動背景 worker:每 5 秒掃一次到期(next_attempt_at <= now)的 pending / failed 推送雲端
    pub fn start_worker(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            // 啟動回收:上次程序結束前殘留在 sending 的項目(App 中途關閉/崩潰)重設為 pending,
            // 否則它們因不在 worker 查詢條件內會永久卡死(殭屍佇列項)
            if let Err(e) = recover_stale_sending(&inner.db).await {
                tracing::warn!(?e, "啟動回收殘留 sending 失敗");
            }
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Err(e) = process_once(&inner).await {
                    tracing::warn!(?e, "Queue worker 推送發生錯誤");
                }
            }
        });
    }
}

/// 回收殘留 sending:把卡在 sending 的項目重設回 pending(清退避立即可送)。
/// 啟動時呼叫一次,救回上次崩潰/關閉遺留的殭屍項。
async fn recover_stale_sending(db: &DbPool) -> AppResult<()> {
    // 被攔截的殘留 sending 不回收成 pending(那會讓它重新被推送),直接定案 cancelled
    sqlx::query(
        "UPDATE report_queue
         SET status='cancelled', next_attempt_at=NULL, updated_at=datetime('now','localtime')
         WHERE status='sending' AND cancel_requested=1",
    )
    .execute(db)
    .await?;

    let res = sqlx::query(
        "UPDATE report_queue
         SET status='pending', next_attempt_at=NULL, updated_at=datetime('now','localtime')
         WHERE status='sending' AND cancel_requested=0",
    )
    .execute(db)
    .await?;
    if res.rows_affected() > 0 {
        event_log::log_bg(db.clone(), "warn", "queue", "回收殘留",
            format!("啟動回收 {} 筆殘留 sending 為 pending", res.rows_affected()));
    }
    Ok(())
}

async fn process_once(inner: &Inner) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT id, response_id, job_sticker, retry_count
         FROM report_queue
         WHERE retry_count < ?
           -- 被攔截的一律不撿(直印失敗 = 面單沒印出來,雲端不該收到完成)
           AND cancel_requested = 0
           AND (
                status IN ('pending', 'failed')
                -- in-session 安全網:卡在 sending 超過 60 秒(中途 DB 錯誤未即時回收)也重撿,
                -- 與啟動回收互補,確保任何路徑下的 sending 都不會永久殭屍
                OR (status = 'sending' AND updated_at < datetime('now','localtime','-60 seconds'))
           )
           -- 退避閘:只撿到期項目(NULL=立即可送)
           AND (next_attempt_at IS NULL OR next_attempt_at <= datetime('now','localtime'))
         ORDER BY id ASC
         LIMIT 20",
    )
    .bind(MAX_RETRY)
    .fetch_all(&inner.db)
    .await?;

    // 逐筆隔離:單筆 DB 錯誤只記 warn 並跳下一筆,不讓整批 early-return
    // (避免「已標 sending 卻因後續錯誤中斷」造成殭屍;殘留也有 60s 安全網兜底)
    for row in rows {
        let id = row.try_get::<i64, _>("id").unwrap_or(-1);
        if let Err(e) = process_row(inner, row).await {
            tracing::warn!(queue_id = id, ?e, "處理佇列項目失敗,跳過續處理下一筆");
        }
    }
    Ok(())
}

/// 處理單一佇列項目:標 sending → 推送 → 依結果改 success / pending(未登入)/ failed(退避)
async fn process_row(inner: &Inner, row: SqliteRow) -> AppResult<()> {
    let id: i64 = row.try_get("id")?;
    let response_id: Option<i64> = row.try_get("response_id").ok().flatten();
    let job_sticker: Option<String> = row.try_get("job_sticker").ok().flatten();
    let retry_count: i64 = row.try_get("retry_count")?;

    // claim:搶不到就代表這筆在選出後已被攔截(或被別的路徑處理掉),直接跳過不推送
    let claimed = sqlx::query(SQL_CLAIM_FOR_SENDING)
        .bind(id)
        .execute(&inner.db)
        .await?;
    if claimed.rows_affected() == 0 {
        tracing::debug!(queue_id = id, "佇列項已被攔截或已被處理,略過推送");
        return Ok(());
    }

    // 構造 logistic-cat webhook payload (job_user 由 cloud client 內部從設定注入)
    let mut payload = serde_json::json!({
        "job_id": response_id,
        "job_sticker": job_sticker,
    });

    match inner.cloud.notify_logistic_cat(&mut payload).await {
        Ok(()) => {
            sqlx::query(
                "UPDATE report_queue
                 SET status='success', sent_at=datetime('now','localtime'), updated_at=datetime('now','localtime')
                 WHERE id=?",
            )
            .bind(id)
            .execute(&inner.db)
            .await?;
            event_log::log_bg(inner.db.clone(), "info", "queue", "推送成功",
                format!("回報推送成功 queue_id={id}"));

            // 推送途中才被要求攔截:雲端已經收下了,收不回來 —— 狀態誠實維持 success,
            // 但必須讓現場知道「這件雲端記成完成、實體卻沒印出」,需人工補印或更正。
            let too_late: i64 = sqlx::query_scalar(
                "SELECT cancel_requested FROM report_queue WHERE id=?",
            )
            .bind(id)
            .fetch_optional(&inner.db)
            .await?
            .unwrap_or(0);
            if too_late == 1 {
                event_log::log_bg(inner.db.clone(), "error", "queue", "回報已送出無法撤回",
                    format!("該件列印失敗但回報已於推送途中送達雲端(queue_id={id});雲端已記為完成,此件需人工補印/更正"));
            }
        }
        Err(AppError::Unauthorized) => {
            // 未登入:不累加 retry,清退避下一輪立即繼續。
            // 但若期間已被要求攔截,就不該再回到待送佇列(下一輪登入後會照送出去)。
            let cancelled = sqlx::query(SQL_FINISH_CANCELLED)
                .bind(id)
                .execute(&inner.db)
                .await?;
            if cancelled.rows_affected() == 0 {
                sqlx::query(
                    "UPDATE report_queue
                     SET status='pending', next_attempt_at=NULL, updated_at=datetime('now','localtime')
                     WHERE id=?",
                )
                .bind(id)
                .execute(&inner.db)
                .await?;
            }
        }
        Err(e) => {
            tracing::warn!(queue_id = id, ?e, "webhook 推送失敗");
            // 推送期間被要求攔截(列印在這段時間內判定失敗)→ 這次沒送成正好,直接定案不再重試。
            // 若走一般的 mark_failed,重試邏輯並不知道有攔截請求,下一輪會把它重新送出去。
            let cancelled = sqlx::query(SQL_FINISH_CANCELLED)
                .bind(id)
                .execute(&inner.db)
                .await?;
            if cancelled.rows_affected() > 0 {
                event_log::log_bg(inner.db.clone(), "info", "queue", "回報已攔下",
                    format!("推送期間該件列印判定失敗,本次推送未成功,已攔下不再重試 queue_id={id}"));
            } else {
                mark_failed(&inner.db, id, retry_count, &e.to_string()).await?;
            }
        }
    }
    Ok(())
}

/// capped 指數退避秒數:5 * 2^(n-1),上限 300 秒。
/// n=1→5, 2→10, 3→20, 4→40, 5→80, 6→160, 7→300(本應 320,封頂), 8+→300。
/// `(n-1).min(6)` 鎖位移上限避免溢位,封頂用 .min(300)。
fn backoff_seconds(new_count: i64) -> i64 {
    (5_i64 * (1_i64 << (new_count - 1).clamp(0, 6))).min(300)
}

/// 標記推送失敗:寫 last_error、累加 retry_count;
/// 未達上限 → capped 指數退避設下次可送時間;達上限 → 放棄並以 error 級事件留痕(不靜默)。
///
/// 兩條寫回都帶 `cancel_requested=0`:呼叫端雖已先試過 [`SQL_FINISH_CANCELLED`],
/// 但兩次 DB 往返之間仍可能插進一個攔截請求。少了這個條件會落地成
/// `status='failed'` 卻帶著攔截旗標的**幽靈列** —— worker 不會撿它(對),
/// 但操作員按「重試失敗」時它會變回待送的樣子卻永遠送不出去,比卡在失敗更難察覺。
/// 寫不進去(代表期間被攔截)就改為收斂成 cancelled,不留中間態。
async fn mark_failed(db: &DbPool, id: i64, retry_count: i64, last_error: &str) -> AppResult<()> {
    let new_count = retry_count + 1;

    if new_count >= MAX_RETRY {
        let res = sqlx::query(
            "UPDATE report_queue
             SET status='failed', retry_count=?, last_error=?, updated_at=datetime('now','localtime')
             WHERE id=? AND cancel_requested=0",
        )
        .bind(new_count)
        .bind(last_error)
        .bind(id)
        .execute(db)
        .await?;
        if res.rows_affected() == 0 {
            return settle_cancelled(db, id).await;
        }
        // 達上限放棄改為可觀測:error 級事件 + last_error 落庫,不再靜默丟件
        event_log::log_bg(db.clone(), "error", "queue", "推送放棄",
            format!("回報達重試上限 {MAX_RETRY} 次放棄 queue_id={id}:{last_error}"));
        return Ok(());
    }

    // capped 指數退避:下次可送 = now + backoff_seconds(n) 秒
    let backoff = backoff_seconds(new_count);
    let res = sqlx::query(
        "UPDATE report_queue
         SET status='failed', retry_count=?, last_error=?,
             next_attempt_at=datetime('now','localtime', ?),
             updated_at=datetime('now','localtime')
         WHERE id=? AND cancel_requested=0",
    )
    .bind(new_count)
    .bind(last_error)
    .bind(format!("+{backoff} seconds"))
    .bind(id)
    .execute(db)
    .await?;
    if res.rows_affected() == 0 {
        return settle_cancelled(db, id).await;
    }
    event_log::log_bg(db.clone(), "warn", "queue", "推送失敗",
        format!("回報推送失敗 queue_id={id} retry={new_count},{backoff}s 後重試"));
    Ok(())
}

/// 推送失敗的寫回撞上「期間才發出的攔截請求」→ 直接定案 cancelled,不留幽靈中間態
async fn settle_cancelled(db: &DbPool, id: i64) -> AppResult<()> {
    sqlx::query(SQL_FINISH_CANCELLED).bind(id).execute(db).await?;
    event_log::log_bg(db.clone(), "info", "queue", "回報已攔下",
        format!("推送失敗且該件列印已判定失敗,直接攔下不再重試 queue_id={id}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{backoff_seconds, MAX_RETRY};

    #[test]
    fn backoff_is_capped_exponential() {
        // 5 * 2^(n-1),上限 300
        assert_eq!(backoff_seconds(1), 5);
        assert_eq!(backoff_seconds(2), 10);
        assert_eq!(backoff_seconds(3), 20);
        assert_eq!(backoff_seconds(4), 40);
        assert_eq!(backoff_seconds(5), 80);
        assert_eq!(backoff_seconds(6), 160);
        // n=7 本應 320,封頂 300
        assert_eq!(backoff_seconds(7), 300);
    }

    #[test]
    fn backoff_never_exceeds_cap_until_give_up() {
        // 放棄前最後一次重試(MAX_RETRY-1=9)仍封頂 300,不溢位
        for n in 1..MAX_RETRY {
            let b = backoff_seconds(n);
            assert!((5..=300).contains(&b), "n={n} backoff={b} 超出 [5,300]");
        }
    }
}
