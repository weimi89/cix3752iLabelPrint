//! 「查件異常」頁分頁 + 關鍵字 / 類別篩選的整合測試(直接呼叫查詢本體)。
//! 涵蓋:無條件分頁、關鍵字命中 query_no / shipping_no / message、類別精確比對、
//! 兩者組合、空白關鍵字視為無條件、越界 offset 回空頁但 total 不變。

use cix3752i_label_print_lib::commands::parcel_alert_commands::{list_parcel_alerts, ParcelAlertListReq};
use sqlx::sqlite::SqlitePoolOptions;

async fn setup() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE parcel_alert (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL, code TEXT, query_no TEXT, message TEXT, channel_code TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')), shipping_no TEXT)",
    )
    .execute(&pool).await.unwrap();
    // 30 筆:偶數 store_closed(有 shipping_no)、奇數 not_found(shipping_no NULL)
    for i in 1..=30i64 {
        let (kind, ship, msg) = if i % 2 == 0 {
            ("store_closed", Some(format!("74Z0{i:07}")), "無法列印，訂單門市關轉")
        } else {
            ("not_found", None, "查無訂單，請確認訂單編號是否正確")
        };
        sqlx::query("INSERT INTO parcel_alert (kind, code, query_no, message, channel_code, shipping_no) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(kind).bind(kind.to_uppercase()).bind(format!("Q{i:04}")).bind(msg).bind("L1").bind(ship)
            .execute(&pool).await.unwrap();
    }
    pool
}

fn req(keyword: Option<&str>, kind: Option<&str>, limit: i64, offset: i64) -> ParcelAlertListReq {
    ParcelAlertListReq { keyword: keyword.map(String::from), query_no: None, shipping_no: None, kind: kind.map(String::from), limit, offset }
}

#[tokio::test]
async fn dedicated_query_no_and_shipping_no_fields() {
    let pool = setup().await;
    let mut r1 = req(None, None, 25, 0);
    r1.query_no = Some("Q001".into());
    let r = list_parcel_alerts(&pool, &r1).await.unwrap();
    assert_eq!(r.total, 10, "Q0010..Q0019");
    assert!(r.items.iter().all(|a| a.query_no.as_deref().unwrap().starts_with("Q001")));

    let mut r2 = req(None, None, 25, 0);
    r2.shipping_no = Some("74Z00000012".into());
    let r = list_parcel_alerts(&pool, &r2).await.unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items[0].id, 12);

    // 三個條件疊加:query_no Q001x ∧ shipping_no 有值(store_closed 才有)∧ kind store_closed → 10,12,14,16,18
    let mut r3 = req(Some("門市"), Some("store_closed"), 25, 0);
    r3.query_no = Some("Q001".into());
    r3.shipping_no = Some("74Z".into());
    let r = list_parcel_alerts(&pool, &r3).await.unwrap();
    assert_eq!(r.items.iter().map(|a| a.id).collect::<Vec<_>>(), vec![18, 16, 14, 12, 10]);
    assert_eq!(r.total, 5);

    let mut r4 = req(None, None, 25, 0);
    r4.query_no = Some("  ".into());
    assert_eq!(list_parcel_alerts(&pool, &r4).await.unwrap().total, 30, "空白視為無條件");
}

#[tokio::test]
async fn paging_without_filter_is_newest_first() {
    let pool = setup().await;
    let p1 = list_parcel_alerts(&pool, &req(None, None, 25, 0)).await.unwrap();
    assert_eq!(p1.total, 30);
    assert_eq!(p1.items.len(), 25);
    assert_eq!(p1.items[0].id, 30, "id DESC 最新在前");
    let p2 = list_parcel_alerts(&pool, &req(None, None, 25, 25)).await.unwrap();
    assert_eq!(p2.total, 30);
    assert_eq!(p2.items.len(), 5);
    assert_eq!(p2.items.last().unwrap().id, 1);
    let p3 = list_parcel_alerts(&pool, &req(None, None, 25, 50)).await.unwrap();
    assert!(p3.items.is_empty());
    assert_eq!(p3.total, 30, "越界頁 total 仍為全量");
}

#[tokio::test]
async fn keyword_hits_query_no_shipping_no_and_message() {
    let pool = setup().await;
    let r = list_parcel_alerts(&pool, &req(Some("Q0007"), None, 25, 0)).await.unwrap();
    assert_eq!((r.total, r.items.len()), (1, 1));
    assert_eq!(r.items[0].query_no.as_deref(), Some("Q0007"));

    let r = list_parcel_alerts(&pool, &req(Some("74Z00000010"), None, 25, 0)).await.unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items[0].shipping_no.as_deref(), Some("74Z00000010"));

    let r = list_parcel_alerts(&pool, &req(Some("門市關轉"), None, 10, 0)).await.unwrap();
    assert_eq!(r.total, 15, "訊息命中全部 store_closed");
    assert_eq!(r.items.len(), 10, "limit 生效");

    let r = list_parcel_alerts(&pool, &req(Some("   "), None, 25, 0)).await.unwrap();
    assert_eq!(r.total, 30, "空白關鍵字視為無條件");

    let r = list_parcel_alerts(&pool, &req(Some("ZZZ"), None, 25, 0)).await.unwrap();
    assert_eq!((r.total, r.items.len()), (0, 0));
}

#[tokio::test]
async fn kind_filter_and_combination() {
    let pool = setup().await;
    let r = list_parcel_alerts(&pool, &req(None, Some("not_found"), 25, 0)).await.unwrap();
    assert_eq!(r.total, 15);
    assert!(r.items.iter().all(|a| a.kind == "not_found" && a.shipping_no.is_none()));

    // 關鍵字 + 類別:Q000 命中 1..=9,其中 store_closed 為 2,4,6,8
    let r = list_parcel_alerts(&pool, &req(Some("Q000"), Some("store_closed"), 25, 0)).await.unwrap();
    assert_eq!(r.total, 4);
    assert_eq!(r.items.iter().map(|a| a.id).collect::<Vec<_>>(), vec![8, 6, 4, 2]);

    let r = list_parcel_alerts(&pool, &req(None, Some(""), 25, 0)).await.unwrap();
    assert_eq!(r.total, 30, "空字串類別 = 全部");

    let r = list_parcel_alerts(&pool, &req(None, None, 0, -5)).await.unwrap();
    assert_eq!(r.items.len(), 1, "limit 最小 clamp 到 1、offset 負值歸 0");
}
