//! 「請求記錄」頁關鍵字搜尋的 SQL 綁定回歸測試。
//!
//! 教訓:sqlx 對 SQLite 是**依 bind 呼叫順序做位置綁定**,與 SQLite 的
//! `?N` 編號參數規則對不上 — `?1` 重複出現後再接匿名 `?`(LIMIT/OFFSET),
//! LIKE 字串會被綁進 LIMIT 造成 `datatype mismatch`(SQLite code 20)。
//! 結論:sqlx 查詢一律用匿名 `?`,同值重複出現就重複 bind。

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::Row;

async fn setup() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE parcel_query_log (
            response_id   INTEGER PRIMARY KEY,
            query_no      TEXT NOT NULL,
            tracking_no   TEXT NOT NULL,
            shipping_provider TEXT,
            sort_channel  TEXT,
            print_profile TEXT,
            should_print  INTEGER NOT NULL DEFAULT 0,
            label_key     TEXT,
            created_at    TEXT NOT NULL DEFAULT (datetime('now')),
            cloud_ms INTEGER, label_ms INTEGER, total_ms INTEGER
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    for i in 1..=5i64 {
        sqlx::query(
            "INSERT INTO parcel_query_log (response_id, query_no, tracking_no, label_key)
             VALUES (?, ?, ?, ?)",
        )
        .bind(i)
        .bind(format!("SF00{i}"))
        .bind(format!("T00{i}"))
        .bind(format!("labels/SF00{i}.png"))
        .execute(&pool)
        .await
        .unwrap();
    }
    pool
}

/// 修正後的關鍵字查詢:全匿名 `?`,like 重複 bind 三次 → 正常命中
#[tokio::test]
async fn keyword_search_anonymous_params_works() {
    let pool = setup().await;
    let like = "%SF003%".to_string();
    let rows = sqlx::query(
        "SELECT response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile,
                should_print, label_key, created_at, cloud_ms, label_ms, total_ms
         FROM parcel_query_log
         WHERE (query_no LIKE ? OR tracking_no LIKE ? OR label_key LIKE ?)
         ORDER BY created_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(&like)
    .bind(&like)
    .bind(&like)
    .bind(25i64)
    .bind(0i64)
    .fetch_all(&pool)
    .await
    .expect("關鍵字查詢執行失敗");
    assert_eq!(rows.len(), 1, "應只命中 SF003 一筆");
    let qn: String = rows[0].try_get("query_no").unwrap();
    assert_eq!(qn, "SF003");
}

/// COUNT 分支(全匿名 `?`)
#[tokio::test]
async fn keyword_count_anonymous_params_works() {
    let pool = setup().await;
    let like = "%T00%".to_string();
    let total: i64 = sqlx::query(
        "SELECT COUNT(*) AS n
         FROM parcel_query_log
         WHERE (query_no LIKE ? OR tracking_no LIKE ? OR label_key LIKE ?)",
    )
    .bind(&like)
    .bind(&like)
    .bind(&like)
    .fetch_one(&pool)
    .await
    .expect("COUNT 查詢執行失敗")
    .try_get("n")
    .unwrap();
    assert_eq!(total, 5);
}

/// 回歸記錄:舊寫法(`?1` 混匿名 `?`)在 sqlx 下必炸 datatype mismatch。
/// 若未來 sqlx 行為改變使此測試失敗,代表混用已安全,但仍不建議。
#[tokio::test]
async fn mixed_numbered_and_anonymous_params_is_broken() {
    let pool = setup().await;
    let like = "%SF003%".to_string();
    let result = sqlx::query(
        "SELECT response_id FROM parcel_query_log
         WHERE query_no LIKE ?1 OR tracking_no LIKE ?1 OR label_key LIKE ?1
         ORDER BY created_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(&like)
    .bind(25i64)
    .bind(0i64)
    .fetch_all(&pool)
    .await;
    assert!(result.is_err(), "?1 混匿名 ? 預期失敗;若通過代表 sqlx 行為已變");
}
