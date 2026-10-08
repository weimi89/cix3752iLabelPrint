use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{db::DbPool, AppError, AppResult, SharedState};

/// 每側最多幾格。格口配置對應現場分揀機的實體格口數,超過這個數多半是填錯。
pub const MAX_SIDE_COUNT: i64 = 10;

/// 格口配置:左右各幾格。以 `sort_channels` 現有的列為準、不另存設定 ——
/// 列在就是有這一格,設定頁、手機遙控、看板、分配全從同一份列讀,不會出現「設定說 3 格、列還有 5 格」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortLayout {
    pub left: i64,
    pub right: i64,
}

/// 拆位置代碼:`L3` → ('L', 3)。格式不對回 None,各入口靠它擋掉亂填的位置。
pub fn parse_position(pos: &str) -> Option<(char, i64)> {
    let mut chars = pos.chars();
    let side = chars.next()?;
    if side != 'L' && side != 'R' {
        return None;
    }
    let n: i64 = chars.as_str().parse().ok()?;
    (n >= 1).then_some((side, n))
}

/// 目前的格口配置:各側取最大的位置號(套用配置時位置一定連號,最大號就是格數)。
pub async fn load_layout(db: &DbPool) -> AppResult<SortLayout> {
    let rows = sqlx::query("SELECT position FROM sort_channels").fetch_all(db).await?;
    let mut layout = SortLayout { left: 0, right: 0 };
    for r in rows {
        let pos: String = r.try_get("position").unwrap_or_default();
        match parse_position(&pos) {
            Some(('L', n)) => layout.left = layout.left.max(n),
            Some(('R', n)) => layout.right = layout.right.max(n),
            _ => {}
        }
    }
    Ok(layout)
}

/// 位置是否存在於目前的格口配置。桌面與手機所有「依位置操作」的入口都靠這個擋:
/// 縮減配置後被移除的位置不能再被暫停／指派,否則會憑空寫出一列。
pub async fn position_exists(db: &DbPool, position: &str) -> bool {
    if parse_position(position).is_none() {
        return false;
    }
    sqlx::query("SELECT 1 FROM sort_channels WHERE position = ?")
        .bind(position)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// 套用格口配置:補建缺少的位置列、移除超出的位置(連同它的物流指派)。回傳被移除的位置。
/// 被移除位置的通道代碼、貼標人員、印表機設定跟著消失 —— 呼叫端要先讓使用者確認過。
pub async fn apply_layout(db: &DbPool, layout: SortLayout) -> AppResult<Vec<String>> {
    for (side, n) in [("左", layout.left), ("右", layout.right)] {
        if !(1..=MAX_SIDE_COUNT).contains(&n) {
            return Err(AppError::Server(format!(
                "{side}側格數必須在 1 到 {MAX_SIDE_COUNT} 之間"
            )));
        }
    }
    let mut tx = db.begin().await?;
    let mut removed = Vec::new();
    for (side, count) in [('L', layout.left), ('R', layout.right)] {
        for n in 1..=count {
            // updated_at 明寫 localtime:0003 建表時的預設值是 UTC,沿用會讓新列比其他列少 8 小時
            sqlx::query(
                "INSERT OR IGNORE INTO sort_channels (position, updated_at) VALUES (?, datetime('now','localtime'))",
            )
            .bind(format!("{side}{n}"))
            .execute(&mut *tx)
            .await?;
        }
        let extra = sqlx::query(
            "SELECT position FROM sort_channels
              WHERE substr(position,1,1) = ? AND CAST(substr(position,2) AS INTEGER) > ?
              ORDER BY CAST(substr(position,2) AS INTEGER)",
        )
        .bind(side.to_string())
        .bind(count)
        .fetch_all(&mut *tx)
        .await?;
        for r in extra {
            let pos: String = r.try_get("position").unwrap_or_default();
            sqlx::query("DELETE FROM sort_channel_dispatch WHERE position = ?")
                .bind(&pos)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM sort_channels WHERE position = ?")
                .bind(&pos)
                .execute(&mut *tx)
                .await?;
            removed.push(pos);
        }
    }
    tx.commit().await?;
    Ok(removed)
}

#[tauri::command]
pub async fn sort_layout_get(state: State<'_, SharedState>) -> AppResult<SortLayout> {
    load_layout(&state.db).await
}

/// 改格口配置(桌面與手機 /rpc 共用)。記事件並廣播,看板與設定頁重拉清單。
#[tauri::command]
pub async fn sort_layout_save(
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
    layout: SortLayout,
) -> AppResult<SortLayout> {
    let removed = apply_layout(&state.db, layout).await?;
    let detail = if removed.is_empty() {
        String::new()
    } else {
        format!(",移除 {}", removed.join("、"))
    };
    crate::event_log::log_bg(
        state.db.clone(),
        "info",
        "server",
        "格口配置",
        format!("格口配置改為左 {} 格／右 {} 格{detail}", layout.left, layout.right),
    );
    let _ = crate::event_bridge::emit(
        &app,
        "sort-layout-updated",
        serde_json::json!({ "left": layout.left, "right": layout.right, "removed": removed }),
    );
    load_layout(&state.db).await
}

/// 將人員姓名寫入共用歷史名單(操作 / 貼單 / 貼標人員三者共用同一份),
/// 已存在則只更新 used_at。空字串不寫入。
pub async fn upsert_sticker_history(db: &DbPool, name: &str) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO sticker_history (name, used_at) VALUES (?, datetime('now','localtime'))
         ON CONFLICT(name) DO UPDATE SET used_at = datetime('now','localtime')",
    )
    .bind(name)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortChannel {
    pub position: String,
    pub channel_code: Option<String>,
    /// 指派物流(1 對多):一個通道可指派多個物流商,對應 dispatch_provider.code
    #[serde(default)]
    pub dispatch_codes: Vec<String>,
    pub job_sticker: Option<String>,
    /// direct_print 模式此通道面單送印的本機印表機(取代舊 dispatch_provider.printer_name)
    #[serde(default)]
    pub printer_name: Option<String>,
    /// 是否啟用。false=暫停,暫停的通道不參與路由分配
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn sort_channel_list(state: State<'_, SharedState>) -> AppResult<Vec<SortChannel>> {
    // 用 CASE 排序保證先左後右、號碼由小到大(L10 不會排在 L2 前面)
    let rows = sqlx::query(
        "SELECT position, channel_code, job_sticker, printer_name, enabled
         FROM sort_channels
         ORDER BY
           CASE substr(position,1,1) WHEN 'L' THEN 0 WHEN 'R' THEN 1 ELSE 2 END,
           CAST(substr(position,2) AS INTEGER)",
    )
    .fetch_all(&state.db)
    .await?;

    // 一次撈出全部通道→物流指派,在記憶體分組(避免 N+1 查詢)
    let dispatch_rows = sqlx::query(
        "SELECT position, dispatch_code FROM sort_channel_dispatch
         ORDER BY position, dispatch_code",
    )
    .fetch_all(&state.db)
    .await?;
    let mut dispatch_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in dispatch_rows {
        let pos: String = r.try_get("position").unwrap_or_default();
        let code: String = r.try_get("dispatch_code").unwrap_or_default();
        if !pos.is_empty() && !code.is_empty() {
            dispatch_map.entry(pos).or_default().push(code);
        }
    }

    Ok(rows
        .into_iter()
        .map(|r| {
            let position: String = r.try_get("position").unwrap_or_default();
            let dispatch_codes = dispatch_map.remove(&position).unwrap_or_default();
            SortChannel {
                channel_code: r.try_get("channel_code").ok(),
                job_sticker: r.try_get("job_sticker").ok(),
                printer_name: r.try_get("printer_name").ok().flatten(),
                enabled: r.try_get::<i64, _>("enabled").unwrap_or(1) != 0,
                dispatch_codes,
                position,
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct SortChannelSaveReq {
    pub position: String,
    #[serde(default)]
    pub channel_code: Option<String>,
    /// 指派物流(1 對多):空陣列代表未指派
    #[serde(default)]
    pub dispatch_codes: Vec<String>,
    #[serde(default)]
    pub job_sticker: Option<String>,
    /// direct_print 模式此通道的本機印表機
    #[serde(default)]
    pub printer_name: Option<String>,
}

fn normalize(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[tauri::command]
pub async fn sort_channel_save(
    state: State<'_, SharedState>,
    req: SortChannelSaveReq,
) -> AppResult<()> {
    if !position_exists(&state.db, &req.position).await {
        return Err(AppError::Server(format!("無效的通道位置: {}", req.position)));
    }

    let channel_code = normalize(req.channel_code);
    // 通道代碼是分揀機格口機器碼(工控機讀它路由格口,如 L1/R4/A01),必為 ASCII 機器碼。
    // 限英數與 - _、長度 ≤ 16,擋下把貼標人員名等中文/長字串誤填進通道代碼 ——
    // 誤填會被工控機當格口碼、且污染「依分揀通道」統計(歷史以當時 channel_code 歸戶,事後難清)。
    // 只在「代碼有變更」時驗證:DB 內既有的不合規值放行,讓操作員仍能改該通道
    // 其他欄位(物流指派/貼標),不被這類資料把整列存檔卡死。
    if let Some(code) = channel_code.as_deref() {
        let current: Option<String> = sqlx::query(
            "SELECT channel_code FROM sort_channels WHERE position = ?",
        )
        .bind(&req.position)
        .fetch_optional(&state.db)
        .await?
        .and_then(|r| r.try_get::<Option<String>, _>("channel_code").ok().flatten());
        if current.as_deref() != Some(code) {
            check_channel_code_format(code)?;
        }
    }
    let job_sticker = normalize(req.job_sticker);
    let printer_name = normalize(req.printer_name);
    // 指派物流去重 + 去空白,保持原始順序
    let mut dispatch_codes: Vec<String> = Vec::new();
    for code in req.dispatch_codes {
        let code = code.trim().to_string();
        if !code.is_empty() && !dispatch_codes.contains(&code) {
            dispatch_codes.push(code);
        }
    }

    // channel_code 若有值，檢查是否被其他 position 或特殊件分流佔用
    if let Some(code) = channel_code.as_deref() {
        if let Some(conflict) = position_using_code(&state.db, code, Some(&req.position)).await? {
            return Err(AppError::Other(format!(
                "通道代碼 \"{code}\" 已被 {conflict} 使用"
            )));
        }
        if read_special_routes(&state.db).await?.reserves(code) {
            return Err(AppError::Other(format!(
                "通道代碼 \"{code}\" 已用在特殊件分流,請改用其他代碼"
            )));
        }
    }

    // 通道本身與多對多指派一起寫入,用交易保證原子性(避免刪了舊指派卻沒寫入新指派)
    let mut tx = state.db.begin().await?;

    sqlx::query(
        "UPDATE sort_channels
         SET channel_code = ?, job_sticker = ?, printer_name = ?, updated_at = datetime('now','localtime')
         WHERE position = ?",
    )
    .bind(&channel_code)
    .bind(&job_sticker)
    .bind(&printer_name)
    .bind(&req.position)
    .execute(&mut *tx)
    .await?;

    // 重設此通道的指派物流:先清空再寫入(整列覆蓋語意,與前端「儲存整列」一致)
    sqlx::query("DELETE FROM sort_channel_dispatch WHERE position = ?")
        .bind(&req.position)
        .execute(&mut *tx)
        .await?;
    for code in &dispatch_codes {
        sqlx::query(
            "INSERT OR IGNORE INTO sort_channel_dispatch (position, dispatch_code) VALUES (?, ?)",
        )
        .bind(&req.position)
        .bind(code)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 寫入 sticker 歷史（給 autocomplete）
    if let Some(name) = job_sticker.as_deref() {
        upsert_sticker_history(&state.db, name).await?;
    }

    Ok(())
}

/// 快速暫停 / 啟用某通道(分揀進行中即時生效,不需整列儲存)。
/// 只動 enabled 欄位,不影響使用者尚未儲存的通道代碼 / 指派物流編輯。
#[tauri::command]
pub async fn sort_channel_set_enabled(
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
    position: String,
    enabled: bool,
) -> AppResult<()> {
    if !position_exists(&state.db, &position).await {
        return Err(AppError::Server(format!("無效的通道位置: {position}")));
    }
    sqlx::query(
        "UPDATE sort_channels
         SET enabled = ?, updated_at = datetime('now','localtime')
         WHERE position = ?",
    )
    .bind(if enabled { 1 } else { 0 })
    .bind(&position)
    .execute(&state.db)
    .await?;
    // 廣播給所有視窗(及讓手機輪詢一致):桌面與手機任一端切換,兩邊都同步
    // 記進事件記錄:格口件數突然掛零時要查得到是誰何時關的(手機遙控那條在 server::set_channel_enabled)
    crate::event_log::log_bg(
        state.db.clone(),
        "info",
        "server",
        "分揀通道切換",
        format!("通道 {position} 已{}(桌面)", if enabled { "啟用" } else { "暫停" }),
    );
    let _ = crate::event_bridge::emit(
        &app,
        "sort-channel-updated",
        serde_json::json!({ "position": position, "enabled": enabled }),
    );
    Ok(())
}

/// 舊版的「未指派通道」與「異常通道」單一代碼。只在還沒存過 [`SETTING_SPECIAL_ROUTES`] 時讀來帶入;
/// 存新設定時不刪也不改 —— 退回舊版時舊版還讀得到原本的值,不會整組分流無聲消失。
const SETTING_UNASSIGNED_CHANNEL: &str = "unassigned_channel_code";
const SETTING_EXCEPTION_CHANNEL: &str = "exception_channel_code";
/// 特殊件分流(JSON 物件:情況 → 格口代碼,見 [`SpecialRoutes`])
const SETTING_SPECIAL_ROUTES: &str = "special_routes";

/// 不進一般格口、可以另外指定送哪個格口的特殊件:物流沒指派格口、各種查件異常、每一格都撞號。
/// 讀碼失敗不在其中:那種件由分揀機自己送它的預設格口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecialCase {
    /// 這家物流沒指派任何格口(或指派的格口全部被跳過)
    Unassigned,
    StoreClosed,
    Unconfirmed,
    StatusAbnormal,
    NotFound,
    NotProxyForward,
    LabelFailed,
    CloudError,
    /// 能去的格口上一件後兩碼都跟它撞號
    Collision,
}

impl SpecialCase {
    /// 設定畫面的列表順序
    pub const ALL: [SpecialCase; 9] = [
        Self::Unassigned,
        Self::StoreClosed,
        Self::Unconfirmed,
        Self::StatusAbnormal,
        Self::NotFound,
        Self::NotProxyForward,
        Self::LabelFailed,
        Self::CloudError,
        Self::Collision,
    ];

    /// 雲端查件錯誤碼屬於哪一種情況;不認得的錯誤碼與連不上雲端都歸「雲端連不上或其他錯誤」
    pub fn for_cloud_code(code: &str) -> Self {
        match code {
            "STORE_CLOSED" => Self::StoreClosed,
            "UNCONFIRMED" => Self::Unconfirmed,
            "STATUS_ABNORMAL" | "ABNORMAL" => Self::StatusAbnormal,
            "NOT_FOUND" => Self::NotFound,
            "NOT_PROXY" | "NOT_FORWARD" => Self::NotProxyForward,
            "LABEL_FAILED" => Self::LabelFailed,
            _ => Self::CloudError,
        }
    }

    /// 代碼不可與一般格口相同:查件異常與撞號的件不印面單,送進一般格口會變成那格多一件沒單的包裹。
    /// 物流沒指派格口沿用一直以來的規則,可以指定一般格口(現場可能本來就把它分進某一格)。
    fn must_differ_from_channels(self) -> bool {
        self != Self::Unassigned
    }

    /// 存檔檢查的錯誤訊息用
    fn label(self) -> &'static str {
        match self {
            Self::Unassigned => "物流沒指派格口",
            Self::StoreClosed => "門市關轉",
            Self::Unconfirmed => "訂單未確認",
            Self::StatusAbnormal => "訂單狀態異常",
            Self::NotFound => "查無訂單",
            Self::NotProxyForward => "非代寄／非轉寄包裹",
            Self::LabelFailed => "面單產生失敗",
            Self::CloudError => "雲端連不上或其他錯誤",
            Self::Collision => "每一格都撞號",
        }
    }
}

/// 特殊件分流:每種情況送哪個格口代碼。沒列出的情況不指定
/// (照沒有設定時的方式處理:不回格口由分揀機送它的預設格口,或錯誤面單跟著物流走)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpecialRoutes(BTreeMap<SpecialCase, String>);

impl SpecialRoutes {
    /// 這種情況要送的格口代碼;沒指定回 None
    pub fn route(&self, case: SpecialCase) -> Option<&str> {
        self.0.get(&case).map(String::as_str).filter(|c| !c.is_empty())
    }

    /// 這個代碼有沒有用在「不可與一般格口同代碼」的情況上
    fn reserves(&self, code: &str) -> bool {
        self.0.iter().any(|(case, c)| case.must_differ_from_channels() && c == code)
    }

    /// 去空白、拿掉沒填的情況,並檢查代碼格式與「不可與一般格口同代碼」。`used_by_channel` 回傳用了某代碼的格口位置。
    fn validated(self, used_by_channel: impl Fn(&str) -> Option<String>) -> AppResult<Self> {
        let mut routes = BTreeMap::new();
        for (case, code) in self.0 {
            let code = code.trim();
            if code.is_empty() {
                continue;
            }
            check_channel_code_format(code)
                .map_err(|e| AppError::Other(format!("「{}」{e}", case.label())))?;
            if let Some(pos) = used_by_channel(code).filter(|_| case.must_differ_from_channels()) {
                return Err(AppError::Other(format!(
                    "「{}」填的 \"{code}\" 是格口 {pos} 的代碼,特殊件請填不屬於一般格口的代碼",
                    case.label()
                )));
            }
            routes.insert(case, code.to_string());
        }
        Ok(Self(routes))
    }
}

/// 讀特殊件分流。還沒存過新設定時,沿用舊版的兩個單一代碼(與舊版行為相同):
/// 物流沒指派格口送「未指派通道」代碼,其他情況都送「異常通道」代碼。
pub async fn read_special_routes(db: &DbPool) -> AppResult<SpecialRoutes> {
    let saved: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(SETTING_SPECIAL_ROUTES)
        .fetch_optional(db)
        .await?;
    if let Some(json) = saved {
        return parse_special_routes(&json);
    }
    let unassigned = read_channel_setting(db, SETTING_UNASSIGNED_CHANNEL).await?;
    let exception = read_channel_setting(db, SETTING_EXCEPTION_CHANNEL).await?;
    let routes = SpecialCase::ALL
        .into_iter()
        .filter_map(|case| {
            let code = if case == SpecialCase::Unassigned { &unassigned } else { &exception };
            code.clone().map(|c| (case, c))
        })
        .collect();
    Ok(SpecialRoutes(routes))
}

/// 解析存下來的特殊件分流。不認得的情況(較新版本存的)略過並記錄,其他照常讀 ——
/// 退回舊版時不會因為多一項就整份讀不了。
fn parse_special_routes(json: &str) -> AppResult<SpecialRoutes> {
    let raw: BTreeMap<String, String> =
        serde_json::from_str(json).map_err(|e| AppError::Other(format!("特殊件分流設定內容無法讀取: {e}")))?;
    let mut routes = BTreeMap::new();
    for (key, code) in raw {
        match serde_json::from_value::<SpecialCase>(serde_json::Value::String(key.clone())) {
            Ok(case) => {
                routes.insert(case, code);
            }
            Err(_) => tracing::warn!(%key, "特殊件分流有這一版不認得的情況,略過"),
        }
    }
    Ok(SpecialRoutes(routes))
}

/// 存特殊件分流。舊版的兩個單一代碼保留不動(見 [`SETTING_UNASSIGNED_CHANNEL`])。
async fn write_special_routes(db: &DbPool, routes: &SpecialRoutes) -> AppResult<()> {
    let json = serde_json::to_string(routes).map_err(|e| AppError::Other(e.to_string()))?;
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?, ?, datetime('now','localtime'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(SETTING_SPECIAL_ROUTES)
    .bind(json)
    .execute(db)
    .await?;
    Ok(())
}

/// 讀舊版的單一格口代碼設定(未指派 / 異常),未設定或留空回 None。
async fn read_channel_setting(db: &DbPool, key: &str) -> AppResult<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<String, _>("value").ok())
        .filter(|s| !s.is_empty()))
}

/// 格口代碼是工控機拿來對實體格口的機器碼:只收英數與 - _、長度 ≤ 16。
/// 擋下把人名等中文、長字串誤填進來 —— 工控機會把它當格口碼,統計也會被污染。
fn check_channel_code_format(code: &str) -> AppResult<()> {
    let ok = code.chars().count() <= 16
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "通道代碼 \"{code}\" 格式不符:僅允許英數字與 - _(長度 ≤ 16),請勿填入人名等文字"
        )))
    }
}

/// 讀取特殊件分流
#[tauri::command]
pub async fn sort_channel_special_routes_get(state: State<'_, SharedState>) -> AppResult<SpecialRoutes> {
    read_special_routes(&state.db).await
}

/// 儲存特殊件分流
#[tauri::command]
pub async fn sort_channel_special_routes_save(
    state: State<'_, SharedState>,
    routes: SpecialRoutes,
) -> AppResult<()> {
    let channel_codes = channel_code_positions(&state.db).await?;
    let routes = routes.validated(|code| channel_codes.get(code).cloned())?;
    write_special_routes(&state.db, &routes).await
}

/// 一般格口的「通道代碼 → 位置」
async fn channel_code_positions(db: &DbPool) -> AppResult<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT channel_code, position FROM sort_channels WHERE channel_code IS NOT NULL AND channel_code <> ''",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// 哪個格口(位置)用了這個通道代碼;`except` 指定要略過的位置(存檔自己那一列時用)。
async fn position_using_code(db: &DbPool, code: &str, except: Option<&str>) -> AppResult<Option<String>> {
    let row = sqlx::query(
        "SELECT position FROM sort_channels WHERE channel_code = ? AND position <> COALESCE(?, '')",
    )
    .bind(code)
    .bind(except)
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|r| r.try_get::<String, _>("position").ok()))
}

/// 前端主動把人員姓名加入歷史名單(掃描/自動列印頁送出時呼叫)。
#[tauri::command]
pub async fn sticker_history_add(state: State<'_, SharedState>, name: String) -> AppResult<()> {
    upsert_sticker_history(&state.db, &name).await
}

#[tauri::command]
pub async fn sticker_history_list(state: State<'_, SharedState>) -> AppResult<Vec<String>> {
    let rows = sqlx::query("SELECT name FROM sticker_history ORDER BY used_at DESC LIMIT 200")
        .fetch_all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| r.try_get("name").unwrap_or_default())
        .collect())
}

#[tauri::command]
pub async fn sticker_history_delete(
    state: State<'_, SharedState>,
    name: String,
) -> AppResult<u64> {
    let result = sqlx::query("DELETE FROM sticker_history WHERE name = ?")
        .bind(&name)
        .execute(&state.db)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod special_routes_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    fn routes(pairs: &[(SpecialCase, &str)]) -> SpecialRoutes {
        SpecialRoutes(pairs.iter().map(|(c, v)| (*c, v.to_string())).collect())
    }

    fn no_channel(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn cloud_error_codes_map_to_cases() {
        use SpecialCase::*;
        for (code, case) in [
            ("STORE_CLOSED", StoreClosed),
            ("UNCONFIRMED", Unconfirmed),
            ("STATUS_ABNORMAL", StatusAbnormal),
            ("ABNORMAL", StatusAbnormal),
            ("NOT_FOUND", NotFound),
            ("NOT_PROXY", NotProxyForward),
            ("NOT_FORWARD", NotProxyForward),
            ("LABEL_FAILED", LabelFailed),
            ("ERROR", CloudError),
            ("SOMETHING_NEW", CloudError),
        ] {
            assert_eq!(SpecialCase::for_cloud_code(code), case, "{code}");
        }
    }

    #[test]
    fn validation_trims_and_drops_blank_cases() {
        let got = routes(&[(SpecialCase::StoreClosed, " LS "), (SpecialCase::Collision, "  ")])
            .validated(no_channel)
            .unwrap();
        assert_eq!(got, routes(&[(SpecialCase::StoreClosed, "LS")]));
        assert_eq!(got.route(SpecialCase::Collision), None, "沒填就是不指定");
    }

    #[test]
    fn validation_rejects_bad_codes() {
        let err = |r: SpecialRoutes, used: fn(&str) -> Option<String>| r.validated(used).unwrap_err().to_string();
        let msg = err(routes(&[(SpecialCase::NotFound, "左邊")]), no_channel);
        assert!(msg.contains("查無訂單") && msg.contains("格式不符"), "{msg}");
        let used_by_l1 = |c: &str| (c == "L1").then(|| "L1".to_string());
        let msg = err(routes(&[(SpecialCase::Collision, "L1")]), used_by_l1);
        assert!(msg.contains("每一格都撞號") && msg.contains("格口 L1"), "{msg}");
    }

    #[test]
    fn unassigned_may_point_at_a_regular_chute() {
        let used_by_l1 = |c: &str| (c == "L1").then(|| "L1".to_string());
        let got = routes(&[(SpecialCase::Unassigned, "L1")]).validated(used_by_l1).unwrap();
        assert_eq!(got.route(SpecialCase::Unassigned), Some("L1"), "物流沒指派格口照舊可以指定一般格口");
        assert!(!got.reserves("L1"), "一般格口仍可使用這個代碼");
        assert!(routes(&[(SpecialCase::NotFound, "RS")]).reserves("RS"));
    }

    #[test]
    fn unknown_cases_from_a_newer_version_are_skipped() {
        let got = parse_special_routes(r#"{"store_closed":"LS","no_read":"RS"}"#).unwrap();
        assert_eq!(got, routes(&[(SpecialCase::StoreClosed, "LS")]));
        assert!(parse_special_routes("not json").is_err());
    }

    #[test]
    fn stored_as_a_plain_case_to_code_object() {
        let json = serde_json::to_string(&routes(&[(SpecialCase::Unassigned, "LS"), (SpecialCase::StoreClosed, "RS")])).unwrap();
        assert_eq!(json, r#"{"unassigned":"LS","store_closed":"RS"}"#);
    }

    async fn db() -> DbPool {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn set_old(db: &DbPool, key: &str, code: &str) {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)").bind(key).bind(code).execute(db).await.unwrap();
    }

    #[tokio::test]
    async fn old_codes_are_carried_over() {
        let db = db().await;
        assert_eq!(read_special_routes(&db).await.unwrap(), SpecialRoutes::default());
        set_old(&db, SETTING_UNASSIGNED_CHANNEL, "LS").await;
        set_old(&db, SETTING_EXCEPTION_CHANNEL, "RS").await;
        let got = read_special_routes(&db).await.unwrap();
        assert_eq!(got.route(SpecialCase::Unassigned), Some("LS"), "物流沒指派格口照舊送未指派通道");
        assert!(
            SpecialCase::ALL.iter().filter(|c| **c != SpecialCase::Unassigned).all(|c| got.route(*c) == Some("RS")),
            "其他情況照舊送原本的異常通道代碼"
        );
    }

    #[tokio::test]
    async fn only_unassigned_code_set_leaves_other_cases_unassigned() {
        let db = db().await;
        set_old(&db, SETTING_UNASSIGNED_CHANNEL, "X00").await;
        let got = read_special_routes(&db).await.unwrap();
        assert_eq!(got, routes(&[(SpecialCase::Unassigned, "X00")]));
    }

    #[tokio::test]
    async fn saved_routes_take_over_but_old_codes_stay_for_rollback() {
        let db = db().await;
        set_old(&db, SETTING_UNASSIGNED_CHANNEL, "LS").await;
        set_old(&db, SETTING_EXCEPTION_CHANNEL, "RS").await;
        let saved = routes(&[(SpecialCase::StoreClosed, "LS"), (SpecialCase::NotFound, "RS")]);
        write_special_routes(&db, &saved).await.unwrap();
        assert_eq!(read_special_routes(&db).await.unwrap(), saved, "存過新設定後就以新設定為準");
        assert_eq!(read_channel_setting(&db, SETTING_EXCEPTION_CHANNEL).await.unwrap().as_deref(), Some("RS"), "舊代碼保留,退回舊版仍讀得到");
        assert_eq!(read_channel_setting(&db, SETTING_UNASSIGNED_CHANNEL).await.unwrap().as_deref(), Some("LS"));
    }
}
