//! 網頁版前端的靜態出口。
//!
//! `dist/` 於編譯期嵌入 binary(見 `build.rs`),安裝後不必另外部署靜態檔,
//! 也不會因為使用者搬動安裝目錄就整個網頁版壞掉。

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../dist"]
struct Assets;

/// 注入給前端的環境旗標。
///
/// 前端據此判斷「我是由本機 server 提供的網頁版」,而不是靠「試打後端失敗就退 mock」
/// —— 後者會把真正的連線錯誤吞成假資料,讓壞掉的頁面看起來像正常的。
const RUNTIME_FLAG: &str = "<script>window.__CIX_WEB__=true</script>";

fn build_index_html() -> String {
    let raw = Assets::get("index.html")
        .map(|f| String::from_utf8_lossy(f.data.as_ref()).into_owned())
        .unwrap_or_else(|| {
            "<!doctype html><meta charset=\"utf-8\"><p>網頁版前端尚未建置。".to_string()
        });

    // 放在 <head> 開頭:必須早於任何前端程式碼執行,否則模組載入時讀不到旗標,
    // 會誤判成純瀏覽器預覽而顯示 mock 資料。
    match raw.find("<head>") {
        Some(i) => {
            let at = i + "<head>".len();
            format!("{}{}{}", &raw[..at], RUNTIME_FLAG, &raw[at..])
        }
        None => format!("{RUNTIME_FLAG}{raw}"),
    }
}

/// 正式版的 dist 是編譯期嵌入的,內容不會變,處理一次就夠。
static INDEX_HTML: Lazy<String> = Lazy::new(build_index_html);

/// 取當前的 index.html。
///
/// 開發模式**每次重讀**:dist 由 Vite 即時產出,而 Vite 的檔名帶內容雜湊,
/// 前端一重新建置檔名就換一組。若沿用啟動時快取的那份,它會繼續指著上一版的
/// JS/CSS —— 那些檔案已經不存在,請求落到 SPA fallback 拿回 index.html,
/// 瀏覽器把 HTML 當成 JS 執行,結果是整個網頁版白畫面,而且看起來像沒有錯誤。
fn index_html() -> String {
    if cfg!(debug_assertions) {
        build_index_html()
    } else {
        INDEX_HTML.clone()
    }
}

/// 這個路徑的快取策略,分三級。
///
/// - **`assets/`**:Vite 產物,檔名帶內容雜湊,改版必換檔名 → 放心鎖一年。
/// - **字型**:檔名帶版本號(`...-v15-latin-700.woff2`),實質不會原地更動,
///   而且每頁都要用。設成每次重新驗證的話,低速網路下光是問這幾個檔就多花好幾個來回。
/// - **其餘**(`sounds/`、其他 `public/` 檔案):檔名固定、內容可能被換掉
///   (例如用 edge-tts 重錄設備異常語音)。給一小時 —— 換過之後最多一小時生效,
///   日常也不必每次回頭問。**不可以給 immutable**,否則換了音檔卻有人繼續聽到舊的,
///   而且沒有任何錯誤跡象,只會有人回報「怎麼還是舊的聲音」。
fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else if path.ends_with(".woff2") || path.ends_with(".woff") || path.ends_with(".ttf") {
        "public, max-age=2592000"
    } else {
        "public, max-age=3600"
    }
}

fn index_response() -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            // 版面本身不快取,否則改版後使用者會一直開到舊的殼
            (header::CACHE_CONTROL, "no-cache"),
        ],
        index_html(),
    )
        .into_response()
}

/// 靜態資源與 SPA fallback。
///
/// 找不到檔案時回 index.html —— 前端路由(`/print-stats` 這類)在使用者按重新整理時
/// 會直接打到後端,沒有 fallback 就會變成 404。
pub(super) async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == "index.html" {
        return index_response();
    }

    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache_control_for(path)),
                ],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => index_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_一定帶著網頁版旗標() {
        assert!(
            index_html().contains("__CIX_WEB__"),
            "少了旗標的話網頁版會被前端當成純瀏覽器預覽,整站顯示假資料"
        );
    }

    /// index.html 引用的每個檔案都要真的取得到。
    ///
    /// Vite 的檔名帶內容雜湊,前端一重新建置就換一組。若 index.html 與資產不同步
    /// (例如把處理後的 HTML 快取住而 dist 已經更新),請求會落到 SPA fallback 拿回
    /// index.html,瀏覽器把 HTML 當 JS 執行 —— 畫面全白,而且主控台不見得看得出原因。
    #[test]
    fn index_指向的資產都取得到() {
        let html = index_html();
        let mut checked = 0;
        for prefix in ["src=\"/assets/", "href=\"/assets/"] {
            let mut rest = html.as_str();
            while let Some(i) = rest.find(prefix) {
                rest = &rest[i + prefix.len()..];
                let Some(end) = rest.find('"') else { break };
                let file = &rest[..end];
                assert!(
                    Assets::get(&format!("assets/{file}")).is_some(),
                    "index.html 指向 assets/{file},但檔案不存在 —— 網頁版會白畫面"
                );
                checked += 1;
                rest = &rest[end..];
            }
        }
        // dist 只有 build.rs 產的佔位時沒有資產可查,不算失敗
        if checked == 0 {
            assert!(
                !html.contains("/assets/"),
                "有引用 /assets/ 卻一個都沒檢查到,解析邏輯壞了"
            );
        }
    }

    #[test]
    fn 只有帶雜湊的產物可以長快取() {
        assert!(cache_control_for("assets/index-abc123.js").contains("immutable"));
        // 這些檔名固定，換內容不換名字，長快取會讓人一直拿到舊的
        for p in ["sounds/alert/parcel_jam-zh.mp3", "static/logo.png", "favicon.ico"] {
            assert!(
                !cache_control_for(p).contains("immutable"),
                "{p} 檔名不帶雜湊，不可標成 immutable"
            );
        }
    }

    #[test]
    fn 字型不必每次回頭驗證() {
        // 字型每頁都要用，又帶版本號不會原地換內容；
        // 設成每次重新驗證會在低速網路下白白多花好幾個來回
        let cc = cache_control_for("static/assets/fonts/red-hat-mono-v15-latin-700.woff2");
        assert!(!cc.contains("max-age=0"), "字型不該每次重新驗證：{cc}");
        assert!(!cc.contains("immutable"), "但也不必鎖死一年：{cc}");
    }

    #[test]
    fn 音檔換得掉但不必每次問() {
        let cc = cache_control_for("sounds/alert/parcel_jam-zh.mp3");
        assert!(cc.contains("max-age=3600"), "重錄後應在一小時內生效：{cc}");
    }

    #[test]
    fn 旗標插在前端程式碼之前() {
        let html = index_html();
        if let Some(head) = html.find("<head>") {
            let flag = html.find("__CIX_WEB__").expect("應有旗標");
            let first_script = html[head..]
                .find("<script type=\"module\"")
                .map(|i| i + head);
            if let Some(s) = first_script {
                assert!(flag < s, "旗標必須早於前端模組載入");
            }
        }
    }
}
