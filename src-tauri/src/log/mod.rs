use tracing_subscriber::EnvFilter;

/// 初始化全域 tracing subscriber
///
/// 預設等級：debug（development）/ info（release），可由 RUST_LOG 覆寫。
pub fn init() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter()));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .compact()
        .init();
}

fn default_filter() -> &'static str {
    if cfg!(debug_assertions) {
        "cix3752i_label_print_lib=debug,axum=info,tower_http=info,sqlx=warn,info"
    } else {
        "info"
    }
}
