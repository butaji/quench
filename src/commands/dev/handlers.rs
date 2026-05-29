//! HTTP request handlers

use crate::config::Config;
use crate::commands::dev::{AppState, ReloadEvent};
use crate::runtime::interpreter::Interpreter;
use anyhow::Result;
use axum::{response::Html, routing::get, Router};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

pub async fn run_server(config: &Config, port: u16) -> Result<()> {
    let project_root = PathBuf::from(".");
    let route_table = Arc::new(RwLock::new(crate::commands::dev::routes::RouteTable::new()));
    let interpreter = Arc::new(RwLock::new(Interpreter::new()));
    let (reload_tx, _) = broadcast::channel(100);

    let state = AppState {
        root: project_root,
        route_table,
        interpreter,
        reload_tx: reload_tx.clone(),
        watcher: Arc::new(std::sync::Mutex::new(notify::recommended_watcher(move |_| {
            let _ = reload_tx.send(ReloadEvent::ModuleChanged(".".to_string()));
        })?)),
    };

    let app = Router::new()
        .route("/", get(handler))
        .route("/blog", get(blog_handler))
        .route("/blog/*path", get(blog_handler))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("Starting dev server on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handler() -> Html<String> {
    Html(r#"<!DOCTYPE html>
<html>
<head><title>Runts Dev</title></head>
<body>
<h1>Welcome to Runts!</h1>
<p>Start building your app by editing files in the <code>routes/</code> directory.</p>
</body>
</html>"#.to_string())
}

async fn blog_handler() -> Html<String> {
    Html(r#"<!DOCTYPE html>
<html>
<head><title>Blog</title></head>
<body>
<h1>Blog</h1>
<p>Blog posts will appear here.</p>
</body>
</html>"#.to_string())
}
