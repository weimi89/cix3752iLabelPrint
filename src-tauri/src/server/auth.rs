//! 網頁版的存取控制。
//!
//! 這台機器直接對外(路由器 Port Forward),因此這裡是唯一一道門:
//!
//! | 來源 | 待遇 |
//! |------|------|
//! | 內網網段 | 免登入,完整權限(現場電腦、工控機、手機) |
//! | 非內網 | 必須輸入共用密碼;通過後與坐在現場同等權限 |
//! | 非內網打工控機 API | 一律拒絕 —— PLC 不會帶密碼,也不該從外網被呼叫 |
//!
//! **外網來源不得修改存取控制本身**(`web_access`)。少了這條,拿到共用密碼的人可以把
//! `lan_cidrs` 改成 `0.0.0.0/0`,之後全世界都被判定為內網 —— `is_lan` 先行放行,
//! 上面那張表的每一條(含「工控機 API 只認內網」)就全部失效。門鎖不能由門外的人來換。
//!
//! **來源判斷只認 TCP 連線的對端位址。** 這台機器直接對外、前面沒有反向代理,
//! `X-Forwarded-For` 之類的標頭是任何人都能自己填的,一旦拿來判斷內外網,
//! 外部只要送一行標頭就整道門直接失效。日後若真的擺了代理在前面,要先確認
//! 代理會覆寫該標頭,再改這裡 —— 不可為了「順手支援代理」就直接採信。

use std::net::{IpAddr, SocketAddr};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use ipnet::IpNet;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::config::WebAccessConfig;
use crate::db::DbPool;
use crate::{event_log, AppResult, SharedState};

/// 共用密碼的 argon2 雜湊存放位置(app_setting 的 key)
const PASSWORD_KEY: &str = "web_access_password_hash";

const COOKIE_NAME: &str = "cix_web_session";

/// 工控機專用端點:只認內網。
///
/// 外網即使登入過也擋 —— 這幾支是給產線 PLC 打的,從網際網路呼叫沒有任何正當情境,
/// 卻能直接觸發列印與回報。
fn is_machine_api(path: &str) -> bool {
    path == "/healthz"
        || path == "/api/report"
        || path == "/api/device-alert"
        || path.starts_with("/api/parcel")
}

/// 需要登入才能碰的資料端點。
///
/// 靜態資源(HTML/JS/CSS)不在此列:外網要先載得到登入頁才有辦法登入,
/// 而那些檔案只是 UI 程式碼,不含任何營運資料。
fn needs_session(path: &str) -> bool {
    path.starts_with("/rpc/")
        || path.starts_with("/api/")
        || path.starts_with("/images/")
        || path.starts_with("/captures/")
        || path.starts_with("/camera/")
        || path.starts_with("/events/")
        || path.starts_with("/board/stream")
}

/// 是否為瀏覽器發起的跨站請求。
///
/// 內網來源完全靠 IP 放行、沒有帳號密碼,因此現場電腦只要開到一個惡意網頁,
/// 那個網頁就能對中介機送出請求:`fetch` 可以觸發 `cache_clear`、`server_restart`
/// 這類不吃參數的指令;而 `<img src="http://中介機:18080/api/parcel/XXXX">`
/// 更會讓中介機真的去雲端查件,直印模式下還會直接送印一張面單。
///
/// 兩種都要擋,但**它們的特徵不同**:
///
/// - `fetch` 的跨站請求帶 `Origin`。
/// - `<img>` / `<script>` / `<iframe>` 發出的跨站 GET **不帶 `Origin`** ——
///   只靠 Origin 判斷會直接漏掉,這正是先前的缺口。
///
/// 因此優先看 `Sec-Fetch-Site`:它由瀏覽器填、網頁改不掉,而且上述每一種都會帶。
/// `same-origin` 是網頁版自己的請求,`none` 是使用者直接開網址或掃 QR 進來的,兩者放行;
/// 其餘(`cross-site`、`same-site`)一律擋。舊瀏覽器沒有這個標頭時退回比對 `Origin`。
/// 工控機、curl 這類非瀏覽器請求兩個標頭都不帶,完全不受影響。
fn is_cross_site(req: &Request) -> bool {
    if let Some(site) = req
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
    {
        if site == "same-origin" || site == "none" {
            return false;
        }

        // 使用者自己點連結進來的頂層導覽要放行。
        //
        // 主管從 LINE、Email 收到網址點進來,瀏覽器標的就是 cross-site —— 一律擋的話
        // 連登入頁都看不到,而「把網址傳給主管」正是這套網頁版的主要用法。
        //
        // 放行不會開後門:CSRF 怕的是網頁在背景替使用者發請求,而頂層導覽是使用者
        // 主動離開原網站、把整個分頁換成我們的頁面,不會把結果回傳給原網站;
        // session cookie 又是 SameSite=Strict,跨站導覽本來就不會帶上,
        // 進來之後仍然要重新登入。
        let dest = req
            .headers()
            .get("sec-fetch-dest")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let mode = req
            .headers()
            .get("sec-fetch-mode")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if dest == "document" && mode == "navigate" {
            return false;
        }

        return true;
    }

    let Some(origin) = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };

    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    // Origin 形如 `scheme://host[:port]`,去掉 scheme 後應與 Host 完全相同
    origin.split("://").nth(1).unwrap_or("") != host
}

/// `/rpc` 是否以 JSON 送出。
///
/// 要求 `application/json` 會讓瀏覽器對跨站請求先送預檢,而這台 server 不回任何
/// CORS 標頭,預檢過不了 —— 等於把上面那條 CSRF 路徑再堵一層。
/// 少了這道檢查的話,`Content-Type: text/plain` 的簡單請求可以直接送達。
fn rpc_body_is_json(req: &Request) -> bool {
    req.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim_start().starts_with("application/json"))
}

/// 來源是否落在設定的內網網段
fn is_lan(ip: IpAddr, cidrs: &[String]) -> bool {
    // IPv4-mapped IPv6(::ffff:192.168.1.5)要還原成 IPv4 再比,
    // 否則雙堆疊監聽下內網電腦會被判成外網。
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    };

    // 本機永遠算內網,不受設定影響。桌面 App 自己的相機預覽、看板、手機遙控 QR
    // 都走這條路徑;設定清單被清空或打錯字時若連本機都擋,桌面版的網頁功能會一起壞掉,
    // 而使用者看到的只是「圖載不出來」,很難聯想到是網段設定的問題。
    if ip.is_loopback() {
        return true;
    }

    cidrs.iter().any(|c| {
        match c.parse::<IpNet>() {
            Ok(net) => net.contains(&ip),
            // 設定打錯字時「不當成內網」,寧可多要一次密碼,
            // 也不要因為一個錯字讓整個外網被當成內網放行
            Err(_) => false,
        }
    })
}

fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    format!("{:x}", h.finalize())
}

/// 產生 session token:兩個 v4 UUID 串接(各 122 bit 亂數),足以抵抗猜測
fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn cookie_token(req: &Request) -> Option<String> {
    token_from_headers(req.headers())
}

// ── 密碼 ────────────────────────────────────────────────────────

pub async fn stored_password_hash(db: &DbPool) -> AppResult<Option<String>> {
    let row = sqlx::query("SELECT value FROM app_setting WHERE key = ?")
        .bind(PASSWORD_KEY)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| r.get::<String, _>("value")).filter(|s| !s.is_empty()))
}

pub async fn set_password(db: &DbPool, plain: &str) -> AppResult<()> {
    let hash = if plain.is_empty() {
        String::new()
    } else {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(plain.as_bytes(), &salt)
            .map_err(|e| crate::AppError::Config(format!("密碼雜湊失敗: {e}")))?
            .to_string()
    };

    sqlx::query(
        "INSERT INTO app_setting (key, value, updated_at)
         VALUES (?, ?, datetime('now','localtime'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(PASSWORD_KEY)
    .bind(&hash)
    .execute(db)
    .await?;

    // 改密碼等於要把所有人請出去重新驗證,否則舊密碼流出後對方仍能用既有連線
    sqlx::query("DELETE FROM web_session").execute(db).await?;
    Ok(())
}

fn verify_password(plain: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// ── session ────────────────────────────────────────────────────

async fn session_valid(db: &DbPool, token: &str) -> bool {
    let hash = hash_token(token);
    let row = sqlx::query(
        "SELECT token_hash FROM web_session
         WHERE token_hash = ? AND expires_at > datetime('now','localtime')",
    )
    .bind(&hash)
    .fetch_optional(db)
    .await;

    match row {
        Ok(Some(_)) => {
            // 更新活動時間供稽核;失敗不影響本次放行
            let _ = sqlx::query(
                "UPDATE web_session SET last_seen_at = datetime('now','localtime') WHERE token_hash = ?",
            )
            .bind(&hash)
            .execute(db)
            .await;
            true
        }
        _ => false,
    }
}

async fn create_session(db: &DbPool, ip: &str, hours: u32) -> AppResult<String> {
    // 順手清掉過期的,免得資料表無止盡長大
    let _ = sqlx::query("DELETE FROM web_session WHERE expires_at <= datetime('now','localtime')")
        .execute(db)
        .await;

    let token = new_token();
    sqlx::query(
        "INSERT INTO web_session (token_hash, client_ip, expires_at)
         VALUES (?, ?, datetime('now','localtime', ?))",
    )
    .bind(hash_token(&token))
    .bind(ip)
    .bind(format!("+{} hours", hours.max(1)))
    .execute(db)
    .await?;

    Ok(token)
}

// ── 失敗鎖定 ────────────────────────────────────────────────────

/// 登入流程的序列化鎖。
///
/// 「查有沒有被鎖 → 驗密碼 → 記一次失敗」是三個獨立步驟,中間沒有任何鎖。
/// 同一來源只要同時開 N 條連線,全部都會在**任何一次失敗被寫進資料庫之前**
/// 讀到「沒被鎖」,等於一口氣免費猜 N 次,`max_fail_attempts` 形同虛設 ——
/// 而這台機器對外就只有這一組共用密碼。
///
/// 登入是低頻操作(要有人真的去輸入密碼),argon2 驗證本身也要上百毫秒,
/// 序列化不會影響現場使用。
static LOGIN_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));


/// 這個來源還要被鎖多久(秒);0 代表沒被鎖。
///
/// 查詢失敗時回 `Err` 而**不是當作沒被鎖** —— 若吞掉錯誤回 0,資料庫一忙碌
/// (SQLITE_BUSY)暴力破解保護就靜默失效,而且不會留下任何跡象。
async fn lock_remaining_secs(db: &DbPool, ip: &str) -> AppResult<i64> {
    let row = sqlx::query(
        "SELECT CAST((julianday(locked_until) - julianday('now','localtime')) * 86400 AS INTEGER) AS secs
         FROM web_login_attempt
         WHERE client_ip = ? AND locked_until IS NOT NULL AND locked_until > datetime('now','localtime')",
    )
    .bind(ip)
    .fetch_optional(db)
    .await?;

    Ok(match row {
        Some(r) => r.try_get::<i64, _>("secs").unwrap_or(0).max(0),
        None => 0,
    })
}

async fn record_fail(db: &DbPool, ip: &str, cfg: &WebAccessConfig) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO web_login_attempt (client_ip, fail_count, last_fail_at)
         VALUES (?, 1, datetime('now','localtime'))
         ON CONFLICT(client_ip) DO UPDATE SET
           fail_count = web_login_attempt.fail_count + 1,
           last_fail_at = datetime('now','localtime')",
    )
    .bind(ip)
    .execute(db)
    .await?;

    sqlx::query(
        "UPDATE web_login_attempt
         SET locked_until = datetime('now','localtime', ?), fail_count = 0
         WHERE client_ip = ? AND fail_count >= ?",
    )
    .bind(format!("+{} minutes", cfg.lock_minutes.max(1)))
    .bind(ip)
    .bind(cfg.max_fail_attempts.max(1) as i64)
    .execute(db)
    .await?;

    Ok(())
}

async fn clear_fails(db: &DbPool, ip: &str) {
    let _ = sqlx::query("DELETE FROM web_login_attempt WHERE client_ip = ?")
        .bind(ip)
        .execute(db)
        .await;
}

/// 取這條長連線的 session token(沒有就是空字串)
pub(super) fn stream_token(headers: &axum::http::HeaderMap) -> String {
    token_from_headers(headers).unwrap_or_default()
}

/// 這條長連線現在還能不能繼續收事件。
///
/// SSE 一旦建立就不再經過中介層,所以要週期性回頭問這一次。**內外網也要每次重算** ——
/// 只在建立當下判斷的話,管理員事後把某個位址移出內網網段,那條已開著的串流會永遠
/// 繼續推送包裹資料;這與「外網 session 失效但串流還開著」是同一個問題的另一半。
///
/// 服務尚未就緒時回 `true`:寧可多推一輪,也不要因為讀不到設定就把現場的看板全部斷線。
pub(super) async fn stream_still_allowed(
    state: &super::ServerState,
    peer: IpAddr,
    token: &str,
) -> bool {
    let Some(cfg) = web_access_config(&state.app).await else {
        return true;
    };
    if is_lan(peer, &cfg.lan_cidrs) {
        return true;
    }
    !token.is_empty() && session_valid(&state.db, token).await
}

fn token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.trim() == COOKIE_NAME)
        .map(|(_, v)| v.trim().to_string())
}

// ── 中介層 ──────────────────────────────────────────────────────

fn json_error(code: StatusCode, msg: &str) -> Response {
    (code, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// 取當前的網頁存取設定。
///
/// 回 `None` 代表 `AppState` 還沒掛上 —— HTTP server 在 bootstrap 早於 `manage(state)` 啟動,
/// 這段空窗期 socket 已經在收連線(工控機重連、瀏覽器 SSE 退避重連都可能打進來)。
/// 這裡若直接 `state()` 會 panic 掉那條連線的 task,所以改用 `try_state` 讓呼叫端回 503。
async fn web_access_config(app: &tauri::AppHandle) -> Option<WebAccessConfig> {
    use tauri::Manager;
    let shared = app.try_state::<SharedState>()?;
    let cfg = shared.config.read().await.web_access.clone();
    Some(cfg)
}

pub(super) async fn guard(
    State(state): State<super::ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    // 直接讀當前設定,改設定即時生效,不必重啟 server
    let Some(cfg) = web_access_config(&state.app).await else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "服務尚未就緒,請稍候再試",
        );
    };

    let path = req.uri().path().to_string();
    let ip = peer.ip();

    // 這兩道要擋在「內網放行」之前 —— CSRF 針對的正是內網使用者的瀏覽器,
    // 先放行內網再檢查等於沒檢查。
    if is_cross_site(&req) {
        tracing::warn!(%ip, %path, "擋下跨站請求");
        return json_error(StatusCode::FORBIDDEN, "不接受跨站請求");
    }
    if path.starts_with("/rpc/") && !rpc_body_is_json(&req) {
        return json_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "指令必須以 application/json 送出",
        );
    }

    if is_lan(ip, &cfg.lan_cidrs) {
        return next.run(req).await;
    }

    // ── 以下都是外網來源 ──

    if !cfg.enabled {
        // 對外沒開放時完全不透露這裡有什麼,連登入頁都不給
        return json_error(StatusCode::FORBIDDEN, "此服務未對外開放");
    }

    if is_machine_api(&path) {
        tracing::warn!(%ip, %path, "外網嘗試呼叫工控機 API,已拒絕");
        event_log::log_bg(
            state.db.clone(),
            "warn",
            "security",
            "外網存取工控機 API 被拒",
            format!("來源 {ip} 嘗試呼叫 {path}"),
        );
        return json_error(StatusCode::FORBIDDEN, "此端點僅限內網");
    }

    if !needs_session(&path) {
        // 靜態資源:讓外網載得到登入頁
        return next.run(req).await;
    }

    let ok = match cookie_token(&req) {
        Some(t) => session_valid(&state.db, &t).await,
        None => false,
    };

    if ok {
        next.run(req).await
    } else {
        json_error(StatusCode::UNAUTHORIZED, "尚未登入或登入已逾期")
    }
}

/// 只准在中介機本機或現場網路內做的事。
///
/// 目前用在「更換共用密碼」:換密碼等同換門鎖,而且**不需要輸入舊密碼**。
/// 加上這一期刻意沒有 TLS,對外那段路徑的 session cookie 是明文 —— 攔到 cookie 的人
/// 若還能換密碼,就不只是「偷看到資料」,而是直接把真正的操作員永久鎖在門外
/// (單純偷 session 會隨到期失效,換掉密碼不會)。
pub(super) async fn guard_lan_only(
    state: &super::ServerState,
    peer: IpAddr,
    what: &str,
) -> Result<(), String> {
    let Some(cfg) = web_access_config(&state.app).await else {
        return Err("服務尚未就緒,請稍候再試".into());
    };
    if is_lan(peer, &cfg.lan_cidrs) {
        return Ok(());
    }

    tracing::warn!(%peer, %what, "外網來源嘗試進行僅限內網的操作,已拒絕");
    event_log::log_bg(
        state.db.clone(),
        "warn",
        "security",
        "外網嘗試變更存取憑證被拒",
        format!("來源 {peer}｜操作 {what}"),
    );
    Err("這項設定只能在中介機本機或現場網路內變更".into())
}

/// 限制外網來源能叫伺服器去讀哪張圖來印。
///
/// `print_image` 的 `image_path` 若不是 http(s) 就直接 `std::fs::read`。對本機 IPC 呼叫端
/// 沒有新增風險(操作員本來就對這台機器有完整檔案權限),但這支現在也經由 `/rpc` 對外開放 ——
/// 拿到共用密碼的人可以叫伺服器讀**它讀得到的任何檔案**(設定檔、資料庫、其他文件)餵進印表機,
/// 而且讀檔失敗的訊息會原樣回傳,等於附送一個路徑存在性的探測管道。
///
/// 因此外網來源只放行「指向這台 server 自己的圖片網址」——網頁版列印面單時傳的正是這種,
/// 功能不受影響。順帶也擋掉「叫伺服器去打任意外部網址」。
pub(super) async fn guard_print_image_path(
    state: &super::ServerState,
    peer: IpAddr,
    args: &serde_json::Value,
) -> Result<(), String> {
    use tauri::Manager;

    let Some(cfg) = web_access_config(&state.app).await else {
        return Err("服務尚未就緒,請稍候再試".into());
    };
    if is_lan(peer, &cfg.lan_cidrs) {
        return Ok(());
    }

    let Some(path) = args
        .get("req")
        .and_then(|r| r.get("image_path"))
        .and_then(|v| v.as_str())
        .filter(|p| !p.is_empty())
    else {
        // 沒帶路徑代表走 base64,影像內容由呼叫端提供,不涉及伺服器端讀檔
        return Ok(());
    };

    let port = state
        .app
        .try_state::<SharedState>()
        .map(|st| st.inner().clone())
        .map(|st| async move { st.config.read().await.server.port })
        .ok_or_else(|| "服務尚未就緒,請稍候再試".to_string())?
        .await;

    let ok = ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|h| path.starts_with(&format!("http://{h}:{port}/")));

    if ok {
        return Ok(());
    }

    tracing::warn!(%peer, %path, "外網來源指定了非本機圖片來源,已拒絕");
    event_log::log_bg(
        state.db.clone(),
        "warn",
        "security",
        "外網指定非本機圖片來源被拒",
        format!("來源 {peer}｜路徑 {path}"),
    );
    Err("只能列印這台機器上的面單影像".into())
}

/// 擋下「外網來源改動機器層級設定」的嘗試。
///
/// 涵蓋兩類欄位,兩類的共同點是**它們影響的是這台機器本身,而不是業務資料**:
///
/// 1. `web_access` —— 內外網判定、對外開關、密碼有效期。不擋的話,外網登入者送一次
///    `update_config` 把 `lan_cidrs` 改成 `0.0.0.0/0`,自己就變成「內網」,
///    連工控機 API 都會對整個網際網路敞開。
/// 2. **快取目錄與存證目錄** —— 快取清理會**遞迴刪除**設定的目錄,`/images` 又直接服務它。
///    目錄驗證只擋磁碟根與幾個受保護資料夾,沒有限制在 app_data 內,
///    所以指到別的目錄就能讓清理器把裡面刪光。改這兩個路徑是維運動作,本來就該在機器前做。
///
/// 桌面(走 IPC,不經過這裡)與內網來源不受限制 —— 坐在機器前或在現場網路內的人,
/// 本來就有完整控制權。其餘設定(雲端、印表機、分揀規則等)外網照樣能改。
pub(super) async fn guard_web_access_change(
    state: &super::ServerState,
    peer: IpAddr,
    args: &serde_json::Value,
) -> Result<(), String> {
    let Some(cfg) = web_access_config(&state.app).await else {
        return Err("服務尚未就緒,請稍候再試".into());
    };

    if is_lan(peer, &cfg.lan_cidrs) {
        return Ok(());
    }

    // Tauri 前端傳 camelCase,直接打 API 的可能用 snake_case,兩種都要看
    let Some(incoming) = args.get("newConfig").or_else(|| args.get("new_config")) else {
        return Ok(());
    };

    // 1) 存取控制本身
    let current = serde_json::to_value(&cfg).unwrap_or(serde_json::Value::Null);
    if incoming.get("web_access").is_some_and(|v| v != &current) {
        return reject(state, peer, "網頁存取設定").await;
    }

    // 2) 會被快取清理遞迴刪除、又被 /images 服務的目錄
    use tauri::Manager;
    let Some(shared) = state.app.try_state::<SharedState>() else {
        return Err("服務尚未就緒,請稍候再試".into());
    };
    let full = shared.config.read().await.clone();

    let dir_changed = |ptr: &[&str], now: &str| -> bool {
        let mut v = incoming;
        for k in ptr {
            match v.get(*k) {
                Some(next) => v = next,
                None => return false, // 沒帶這個欄位就不算變更
            }
        }
        v.as_str().is_some_and(|s| s != now)
    };

    if dir_changed(&["cache", "dir"], &full.cache.dir) {
        return reject(state, peer, "快取目錄").await;
    }
    if dir_changed(&["camera", "captures_dir"], &full.camera.captures_dir) {
        return reject(state, peer, "讀碼站存證目錄").await;
    }

    Ok(())
}

async fn reject(state: &super::ServerState, peer: IpAddr, what: &str) -> Result<(), String> {
    tracing::warn!(%peer, %what, "外網來源嘗試修改機器層級設定,已拒絕");
    event_log::log_bg(
        state.db.clone(),
        "warn",
        "security",
        "外網嘗試修改機器設定被拒",
        format!("來源 {peer}｜項目 {what}"),
    );
    Err(format!("{what}只能在中介機本機或現場網路內修改"))
}

// ── 登入 / 登出 / 狀態 ──────────────────────────────────────────

#[derive(Deserialize)]
pub(super) struct LoginBody {
    password: String,
}

#[derive(Serialize)]
pub(super) struct AuthStatus {
    /// 這條連線目前能不能存取資料端點
    authenticated: bool,
    /// 來源是否被視為內網(內網免登入)
    lan: bool,
    /// 是否已設定共用密碼 —— 沒設定的話外網永遠進不來
    password_set: bool,
    /// 對外存取總開關
    enabled: bool,
}

pub(super) async fn status(
    State(state): State<super::ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let Some(cfg) = web_access_config(&state.app).await else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "服務尚未就緒,請稍候再試",
        );
    };

    let lan = is_lan(peer.ip(), &cfg.lan_cidrs);
    let password_set = stored_password_hash(&state.db)
        .await
        .ok()
        .flatten()
        .is_some();

    let authenticated = if lan {
        true
    } else {
        match cookie_token(&req) {
            Some(t) => session_valid(&state.db, &t).await,
            None => false,
        }
    };

    Json(AuthStatus {
        authenticated,
        lan,
        password_set,
        enabled: cfg.enabled,
    })
    .into_response()
}

pub(super) async fn login(
    State(state): State<super::ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginBody>,
) -> Response {
    let Some(cfg) = web_access_config(&state.app).await else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "服務尚未就緒,請稍候再試",
        );
    };

    let ip = peer.ip().to_string();

    if !cfg.enabled && !is_lan(peer.ip(), &cfg.lan_cidrs) {
        return json_error(StatusCode::FORBIDDEN, "此服務未對外開放");
    }

    // 從這裡到「記錄失敗」為止必須不可分割,理由見 LOGIN_LOCK
    let _serialized = LOGIN_LOCK.lock().await;

    let wait = match lock_remaining_secs(&state.db, &ip).await {
        Ok(w) => w,
        Err(e) => {
            // 查不出鎖定狀態時寧可擋下來:放行等於在資料庫出問題的期間開放無限次嘗試
            tracing::warn!(?e, %ip, "查詢登入鎖定狀態失敗,本次登入一併拒絕");
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "目前無法驗證登入,請稍候再試",
            );
        }
    };
    if wait > 0 {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("嘗試次數過多,請於 {} 分鐘後再試", wait.div_euclid(60) + 1),
        );
    }

    let stored = match stored_password_hash(&state.db).await {
        Ok(Some(h)) => h,
        // 沒設密碼時外網一律進不來 —— 「沒設密碼就放行」會讓剛裝好、
        // 還沒設定的機器在對外開關打開的瞬間門戶大開
        _ => {
            return json_error(
                StatusCode::FORBIDDEN,
                "尚未設定網頁存取密碼,請先在桌面版設定",
            )
        }
    };

    if !verify_password(&body.password, &stored) {
        if let Err(e) = record_fail(&state.db, &ip, &cfg).await {
            // 記不起來就等於這次失敗沒被計數,鎖定會比預期晚生效 —— 至少要留下痕跡
            tracing::warn!(?e, %ip, "記錄登入失敗次數失敗,鎖定計數可能不準");
        }
        tracing::warn!(%ip, "網頁登入密碼錯誤");
        event_log::log_bg(
            state.db.clone(),
            "warn",
            "security",
            "網頁登入失敗",
            format!("來源 {ip} 密碼錯誤"),
        );
        return json_error(StatusCode::UNAUTHORIZED, "密碼錯誤");
    }

    clear_fails(&state.db, &ip).await;

    let token = match create_session(&state.db, &ip, cfg.session_hours).await {
        Ok(t) => t,
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    event_log::log_bg(
        state.db.clone(),
        "info",
        "security",
        "網頁登入成功",
        format!("來源 {ip}"),
    );

    // 尚未啟用 TLS,因此不加 Secure —— 加了的話 http 連線會直接收不到這個 cookie,
    // 整個登入功能等於不能用。**改用 https 之後這裡要補上 Secure。**
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        cfg.session_hours.max(1) as u64 * 3600
    );

    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

pub(super) async fn logout(State(state): State<super::ServerState>, req: Request) -> Response {
    if let Some(t) = cookie_token(&req) {
        let _ = sqlx::query("DELETE FROM web_session WHERE token_hash = ?")
            .bind(hash_token(&t))
            .execute(&state.db)
            .await;
    }

    let cookie = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cidrs() -> Vec<String> {
        crate::config::WebAccessConfig::default().lan_cidrs
    }

    #[test]
    fn 內網網段判定() {
        for ip in ["192.168.1.10", "10.0.0.5", "172.16.3.9", "127.0.0.1"] {
            assert!(is_lan(ip.parse().unwrap(), &cidrs()), "{ip} 應視為內網");
        }
    }

    #[test]
    fn 外網位址不得被當成內網() {
        // 172.32 落在 172.16/12 之外,是常見的邊界誤判
        for ip in ["8.8.8.8", "1.1.1.1", "172.32.0.1", "11.0.0.1"] {
            assert!(!is_lan(ip.parse().unwrap(), &cidrs()), "{ip} 不該視為內網");
        }
    }

    #[test]
    fn ipv4_mapped_的內網位址要還原後再判() {
        // 雙堆疊監聽時內網電腦的來源位址會長這樣,不還原就會被誤判成外網
        let ip: IpAddr = "::ffff:192.168.1.20".parse().unwrap();
        assert!(is_lan(ip, &cidrs()));
    }

    #[test]
    fn 本機永遠算內網_即使網段清單是空的() {
        // 使用者可能在設定頁把網段清單整個清空
        assert!(is_lan("127.0.0.1".parse().unwrap(), &[]));
        assert!(is_lan("::1".parse().unwrap(), &[]));
    }

    #[test]
    fn 網段設定打錯字時不放行() {
        let bad = vec!["19.2.168.0/999".to_string(), "not-a-cidr".to_string()];
        assert!(!is_lan("192.168.1.1".parse().unwrap(), &bad));
    }

    fn req_with(headers: &[(&str, &str)]) -> Request {
        let mut b = Request::builder().uri("/rpc/ping");
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.body(axum::body::Body::empty()).unwrap()
    }

    #[test]
    fn 同源請求不算跨站() {
        // 網頁版自己的 fetch:Origin 與 Host 一致
        assert!(!is_cross_site(&req_with(&[
            ("origin", "http://192.168.1.50:18080"),
            ("host", "192.168.1.50:18080"),
        ])));
        // 開發時走 Vite 代理,兩者同為 localhost:11420
        assert!(!is_cross_site(&req_with(&[
            ("origin", "http://localhost:11420"),
            ("host", "localhost:11420"),
        ])));
    }

    #[test]
    fn 惡意網站的跨站請求要擋掉() {
        // 現場電腦開到惡意網頁,那個網頁對中介機送指令
        assert!(is_cross_site(&req_with(&[
            ("origin", "https://evil.example.com"),
            ("host", "192.168.1.50:18080"),
        ])));
        // 同主機不同埠也算跨站
        assert!(is_cross_site(&req_with(&[
            ("origin", "http://192.168.1.50:9999"),
            ("host", "192.168.1.50:18080"),
        ])));
    }

    #[test]
    fn 圖片標籤發出的跨站_get_要擋掉() {
        // <img src="http://中介機:18080/api/parcel/XXXX"> 不帶 Origin，
        // 但會帶 Sec-Fetch-Site: cross-site
        assert!(is_cross_site(&req_with(&[
            ("sec-fetch-site", "cross-site"),
            ("sec-fetch-dest", "image"),
            ("host", "192.168.1.50:18080"),
        ])));
        assert!(is_cross_site(&req_with(&[("sec-fetch-site", "same-site")])));
    }

    #[test]
    fn 從聊天軟體點連結進來要開得起來() {
        // 主管從 LINE / Email 點網址:cross-site，但那是使用者主動的頂層導覽
        assert!(!is_cross_site(&req_with(&[
            ("sec-fetch-site", "cross-site"),
            ("sec-fetch-dest", "document"),
            ("sec-fetch-mode", "navigate"),
        ])));
    }

    #[test]
    fn 頂層導覽的放行不可被子資源冒用() {
        // 只有「document + navigate」這組才放行，惡意頁面塞的圖片／iframe 仍要擋
        assert!(is_cross_site(&req_with(&[
            ("sec-fetch-site", "cross-site"),
            ("sec-fetch-dest", "image"),
            ("sec-fetch-mode", "no-cors"),
        ])));
        assert!(is_cross_site(&req_with(&[
            ("sec-fetch-site", "cross-site"),
            ("sec-fetch-dest", "iframe"),
            ("sec-fetch-mode", "navigate"),
        ])));
        assert!(is_cross_site(&req_with(&[
            ("sec-fetch-site", "cross-site"),
            ("sec-fetch-dest", "empty"),
            ("sec-fetch-mode", "cors"),
        ])));
    }

    #[test]
    fn 自己的網頁與直接開網址都要放行() {
        // 網頁版自己發的請求
        assert!(!is_cross_site(&req_with(&[("sec-fetch-site", "same-origin")])));
        // 使用者直接輸入網址、點書籤、掃 QR 進來
        assert!(!is_cross_site(&req_with(&[("sec-fetch-site", "none")])));
    }

    #[test]
    fn 非瀏覽器請求不帶_origin_不受影響() {
        // 工控機 PLC、curl 都不會帶 Origin
        assert!(!is_cross_site(&req_with(&[("host", "192.168.1.50:18080")])));
    }

    #[test]
    fn rpc_只收_json() {
        assert!(rpc_body_is_json(&req_with(&[("content-type", "application/json")])));
        assert!(rpc_body_is_json(&req_with(&[(
            "content-type",
            "application/json; charset=utf-8"
        )])));
        // 這兩種是「簡單請求」,不觸發預檢,正是 CSRF 會用的送法
        assert!(!rpc_body_is_json(&req_with(&[("content-type", "text/plain")])));
        assert!(!rpc_body_is_json(&req_with(&[(
            "content-type",
            "application/x-www-form-urlencoded"
        )])));
        assert!(!rpc_body_is_json(&req_with(&[])));
    }

    #[test]
    fn 工控機端點辨識() {
        for p in ["/healthz", "/api/report", "/api/device-alert", "/api/parcel/ABC123"] {
            assert!(is_machine_api(p), "{p} 應為工控機端點");
        }
        for p in ["/rpc/ping", "/api/channels", "/images/a.jpg"] {
            assert!(!is_machine_api(p), "{p} 不是工控機端點");
        }
    }

    #[test]
    fn 需要登入的路徑涵蓋所有資料出口() {
        for p in [
            "/rpc/print_stats_summary",
            "/api/channels",
            "/images/x.jpg",
            "/captures/y.jpg",
            "/camera/preview/stream",
            "/events/stream",
            "/board/stream",
        ] {
            assert!(needs_session(p), "{p} 必須要求登入");
        }
        // 靜態殼要放行,否則外網載不到登入頁
        for p in ["/", "/index.html", "/assets/index-abc.js", "/print-stats"] {
            assert!(!needs_session(p), "{p} 是靜態資源,不該擋");
        }
    }

    #[test]
    fn 密碼雜湊可驗證且不同鹽產生不同結果() {
        let salt = SaltString::generate(&mut OsRng);
        let h = Argon2::default()
            .hash_password(b"pa55word", &salt)
            .unwrap()
            .to_string();
        assert!(verify_password("pa55word", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("pa55word", "not-a-hash"));
    }

    #[test]
    fn token_雜湊不可逆且穩定() {
        let t = new_token();
        assert_eq!(hash_token(&t), hash_token(&t));
        assert_ne!(hash_token(&t), t);
        assert_eq!(hash_token(&t).len(), 64);
    }

    #[test]
    fn token_每次都不同() {
        assert_ne!(new_token(), new_token());
    }
}
