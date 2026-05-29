//! HTTP request handlers

use crate::commands::dev::AppState;
use crate::config::Config;
use crate::runtime::quickjs::QuickJsRuntime;
use anyhow::Result;
use axum::{response::Html, routing::get, Router};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

pub async fn run_server(config: &Config, port: u16) -> Result<()> {
    let state = create_app_state()?;
    let app = create_router(state);
    start_server(app, port).await
}

fn create_app_state() -> Result<AppState> {
    let project_root = PathBuf::from(".");
    let route_table = Arc::new(RwLock::new(crate::commands::dev::routes::RouteTable::new()));
    let js_runtime = Arc::new(RwLock::new(QuickJsRuntime::new()));
    let (reload_tx, _) = broadcast::channel(100);
    let watcher = create_watcher(reload_tx.clone())?;
    Ok(AppState { root: project_root, route_table, js_runtime, reload_tx, watcher })
}

fn create_watcher(reload_tx: broadcast::Sender<crate::commands::dev::ReloadEvent>) -> Result<Arc<std::sync::Mutex<notify::RecommendedWatcher>>> {
    let tx = reload_tx.clone();
    let watcher = notify::recommended_watcher(move |_| {
        let _ = tx.send(crate::commands::dev::ReloadEvent::ModuleChanged(".".to_string()));
    }).map_err(|e| anyhow::anyhow!("Watcher error: {}", e))?;
    Ok(Arc::new(std::sync::Mutex::new(watcher)))
}

fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handler))
        .route("/blog", get(blog_handler))
        .route("/blog/*path", get(blog_handler))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

async fn start_server(app: Router, port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("Starting dev server on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server running! Press Ctrl+C to stop.");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handler() -> Html<String> {
    // QuickJS eval example
    let js = crate::runtime::quickjs::QuickJsRuntime::new();
    let greeting = js.eval("\"Hello from QuickJS!\"").unwrap_or_else(|_| "[error]".to_string());
    
    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Runts Dev</title></head>
<body>
<h1>Welcome to Runts!</h1>
<p>QuickJS: {}</p>
<p>Start building your app by editing files in the <code>routes/</code> directory.</p>
</body>
</html>"#,
        greeting
    ))
}

async fn blog_handler() -> Html<String> {
    Html(
        r#"<!DOCTYPE html>
<html>
<head><title>Blog</title></head>
<body>
<h1>Blog</h1>
<p>Blog posts will appear here.</p>
</body>
</html>"#
            .to_string(),
    )
}
