//! HTTP request handlers

use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use notify::RecommendedWatcher;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use tokio::sync::broadcast;
use walkdir::WalkDir;
use tracing::info;

use crate::config::Config;
use crate::runtime::interpreter::{Interpreter, RenderResult, RequestInfo};
use super::{AppState, ReloadEvent, routes::RouteTable};

/// Extract headers from request
fn extract_headers(headers: &http::HeaderMap) -> HashMap<String, String> {
    headers.iter()
        .filter_map(|(k, v)| {
            v.to_str().ok().map(|s| (k.to_string(), s.to_string()))
        })
        .collect()
}

/// Path to title
fn path_to_title(path: &str) -> String {
    if path == "/" {
        "Home".to_string()
    } else {
        path.trim_start_matches('/')
            .replace('/', " - ")
            .split('-')
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Build nav links
fn build_nav_links() -> String {
    r#"
        <nav class="runts-nav">
            <a href="/">Home</a>
            <a href="/blog">Blog</a>
        </nav>
    "#.to_string()
}

/// Run dev server
pub async fn run_server(config: &Config, port: u16) -> Result<()> {
    let project_root = PathBuf::from(".");
    let routes_dir = project_root.join("routes");

    // Initialize interpreter
    let interpreter = Arc::new(RwLock::new(Interpreter::new()));

    // Build route table
    let route_table = RouteTable::from_routes_dir(&routes_dir)
        .unwrap_or_default();

    // Create broadcast channel
    let (reload_tx, _reload_rx) = broadcast::channel::<ReloadEvent>(100);

    // Create app state
    let state = AppState {
        root: project_root,
        route_table: Arc::new(RwLock::new(route_table)),
        interpreter,
        reload_tx,
        watcher: Arc::new(std::sync::Mutex::new(
            notify::recommended_watcher(|_| {}).unwrap()
        )),
    };

    // Build router
    let app = Router::new()
        .route("/", get(handle_ssr))
        .route("/api/*path", get(handle_api))
        .route("/*path", get(handle_ssr))
        .with_state(state);

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Dev server running at http://{}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

/// Handle SSR routes
async fn handle_ssr(
    State(state): State<AppState>,
    path: Option<Path<String>>,
    headers: http::HeaderMap,
) -> impl IntoResponse {
    let path = path.map(|p| p.0).unwrap_or_default();
    let path = if path.is_empty() { "/".to_string() } else { format!("/{}", path) };
    let path = path.as_str();

    let request = RequestInfo {
        method: "GET".to_string(),
        path: path.to_string(),
        url: format!("http://localhost{}", path),
        headers: extract_headers(&headers),
        body: None,
        name: None,
        id: None,
    };

    match state.interpreter.read().execute_route(path, HashMap::new(), request) {
        Ok(result) => Html(result.body).into_response(),
        Err(e) => Html(format!("<h1>Error</h1><p>{}</p>", e)).into_response(),
    }
}

/// Handle API routes
async fn handle_api(
    State(state): State<AppState>,
    path: Option<Path<String>>,
) -> Response {
    let path = path.map(|p| p.0).unwrap_or_default();
    let api_path = format!("/api/{}", path);

    let request = RequestInfo {
        method: "GET".to_string(),
        path: api_path.clone(),
        url: format!("http://localhost{}", api_path),
        headers: HashMap::new(),
        body: None,
        name: None,
        id: None,
    };

    match state.interpreter.read().execute_route(&api_path, HashMap::new(), request) {
        Ok(result) => {
            let response = serde_json::json!({
                "path": api_path,
                "body": result.body,
            });
            axum::Json(response).into_response()
        }
        Err(e) => {
            let response = serde_json::json!({ "error": e.to_string() });
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(response)).into_response()
        }
    }
}
