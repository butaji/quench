//! Development server with hot reload
//!
//! Watches for file changes and provides:
//! - Incremental transpilation
//! - WebSocket-based hot reload
//! - Error overlay

use anyhow::Result;
use axum::{
    extract::{State, Path},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures::StreamExt;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use std::{
    path::{Path as StdPath, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::transpile::Transpiler;

/// Shared state for the dev server
#[derive(Clone)]
pub struct DevServerState {
    /// Transpiler instance
    pub transpiler: Arc<RwLock<Transpiler>>,
    
    /// Broadcast channel for file changes
    pub change_tx: broadcast::Sender<FileChange>,
    
    /// Configuration
    pub config: Arc<Config>,
    
    /// Project root
    pub project_root: PathBuf,
}

/// File change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

/// Start the development server
pub async fn start_dev_server(config: &Config, path: PathBuf) -> Result<()> {
    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("Starting development server on http://{}", addr);
    info!("Project: {:?}", path);

    // Resolve project root
    let project_root = find_project_root(&path)?;
    info!("Project root: {:?}", project_root);

    // Create broadcast channel for file changes
    let (change_tx, _rx) = broadcast::channel::<FileChange>(100);
    
    // Create shared state
    let state = DevServerState {
        transpiler: Arc::new(RwLock::new(Transpiler::new(config))),
        change_tx: change_tx.clone(),
        config: Arc::new(config.clone()),
        project_root: project_root.clone(),
    };

    // Build router
    let app = Router::new()
        // Serve static files
        .nest_service("/static", ServeDir::new(project_root.join("static")))
        // API endpoints for dev
        .route("/_api/transpile", post(transpile_handler))
        // SPA fallback for routes
        .route("/*path", get(route_handler))
        .route("/", get(index_handler))
        .with_state(state.clone());

    // Start file watcher
    let watcher_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = start_file_watcher(watcher_state).await {
            error!("File watcher error: {}", e);
        }
    });

    // Start server
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server listening on http://{}", addr);
    
    // Optionally open browser
    if config.dev.open {
        #[cfg(target_os = "macos")]
        {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let _ = std::process::Command::new("open")
                    .arg(&format!("http://{}", addr))
                    .spawn();
            });
        }
    }
    
    axum::serve(listener, app).await?;

    Ok(())
}

/// Find project root
fn find_project_root(path: &StdPath) -> Result<PathBuf> {
    let mut current = path.to_path_buf();
    
    loop {
        if current.join("Cargo.toml").exists() 
            || current.join("runts.config.json").exists() 
            || current.join("runts.config.ts").exists()
        {
            return Ok(current);
        }
        
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    
    // Default to current directory
    Ok(path.to_path_buf())
}

/// Start file watcher
async fn start_file_watcher(state: DevServerState) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    
    let tx_clone = tx.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                let _ = tx_clone.blocking_send(event);
            }
        },
        NotifyConfig::default(),
    )?;

    // Watch source directories
    let dirs = ["routes", "islands", "components"];
    for dir in &dirs {
        let watch_path = state.project_root.join(dir);
        if watch_path.exists() {
            if let Err(e) = watcher.watch(&watch_path, RecursiveMode::Recursive) {
                warn!("Failed to watch {}: {}", watch_path.display(), e);
            }
        }
    }

    info!("File watcher started");
    
    // Process file events
    while let Some(event) = rx.recv().await {
        for event_path in event.paths {
            // Only process TS/TSX files
            let ext = event_path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            
            if ext != "ts" && ext != "tsx" {
                continue;
            }

            // Ignore generated files
            if event_path.to_string_lossy().contains("/gen/") || event_path.to_string_lossy().contains("\\gen\\") {
                continue;
            }

            let change_kind = match event.kind {
                notify::EventKind::Create(_) => ChangeKind::Created,
                notify::EventKind::Modify(_) => ChangeKind::Modified,
                notify::EventKind::Remove(_) => ChangeKind::Deleted,
                _ => continue,
            };

            let change = FileChange {
                path: event_path.to_string_lossy().to_string(),
                kind: change_kind,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            };

            debug!("File change: {:?}", change);
            
            // Broadcast change
            let _ = state.change_tx.send(change);
            
            // Re-transpile the file
            if matches!(change_kind, ChangeKind::Created | ChangeKind::Modified) {
                match state.transpiler.write().transpile_file(&event_path) {
                    Ok(_code) => {
                        info!("Transpiled: {:?}", event_path);
                    }
                    Err(e) => {
                        warn!("Transpile error for {:?}: {}", event_path, e);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Index handler - renders the app
async fn index_handler(State(state): State<DevServerState>) -> Response {
    let index_path = state.project_root.join("routes/index.tsx");
    
    if index_path.exists() {
        match state.transpiler.write().transpile_file(&index_path) {
            Ok(_code) => {
                // In production, this would render the component
                Html(get_dev_page("Index page loaded")).into_response()
            }
            Err(e) => {
                Html(get_error_page("Transpile error", &e.to_string())).into_response()
            }
        }
    } else {
        Html(get_dev_page(r#"<p>Welcome to runts dev server</p>
<p>Create <code>routes/index.tsx</code> to get started.</p>"#)).into_response()
    }
}

/// Route handler for dynamic routes
async fn route_handler(
    State(state): State<DevServerState>,
    Path(path): Path<String>,
) -> Response {
    let route_path = find_route_file(&state.project_root, &path);
    
    if let Some(file_path) = route_path {
        match state.transpiler.write().transpile_file(&file_path) {
            Ok(code) => {
                debug!("Transpiled route /{} -> {:?}", path, file_path);
                Html(get_dev_page(&format!("Route: /{}\n\n```rust\n{}\n```", path, &code[..code.len().min(500)]))).into_response()
            }
            Err(e) => {
                Html(get_error_page(&format!("Error in /{}", path), &e.to_string())).into_response()
            }
        }
    } else {
        Html(get_error_page("404", &format!("Route /{} not found", path))).into_response()
    }
}

/// Find route file for a path
fn find_route_file(project_root: &StdPath, url_path: &str) -> Option<PathBuf> {
    let routes_dir = project_root.join("routes");
    let normalized = url_path.trim_start_matches('/');
    
    // Try direct match
    let direct = routes_dir.join(format!("{}.tsx", normalized.replace('/', "_")));
    if direct.exists() {
        return Some(direct);
    }
    
    // Try as directory index
    let index = routes_dir.join(normalized).join("index.tsx");
    if index.exists() {
        return Some(index);
    }
    
    // Try dynamic route match
    for entry in walkdir::WalkDir::new(&routes_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();
        if entry_path.extension().and_then(|e| e.to_str()) == Some("tsx") {
            let relative = entry_path.strip_prefix(&routes_dir)
                .unwrap_or(entry_path)
                .to_string_lossy()
                .replace('\\', "/");
            
            let pattern = relative
                .replace("[", ":")
                .replace("]", "")
                .replace(".tsx", "");
            
            if pattern.trim_start_matches('/') == normalized 
                || pattern == format!("/{}", normalized)
            {
                return Some(entry_path.to_path_buf());
            }
        }
    }
    
    None
}

/// Transpile API endpoint
async fn transpile_handler(
    State(state): State<DevServerState>,
    body: String,
) -> Response {
    #[derive(Deserialize)]
    struct TranspileRequest {
        path: String,
    }
    
    match serde_json::from_str::<TranspileRequest>(&body) {
        Ok(req) => {
            let event_path = PathBuf::from(&req.path);
            match state.transpiler.write().transpile_file(&event_path) {
                Ok(code) => {
                    Html(format!("<pre><code>{}</code></pre>", code)).into_response()
                }
                Err(e) => {
                    Html(get_error_page("Transpile error", &e.to_string())).into_response()
                }
            }
        }
        Err(e) => {
            Html(get_error_page("Invalid request", &e.to_string())).into_response()
        }
    }
}

/// Generate development page
fn get_dev_page(content: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>runts dev</title>
    <style>
        body {{
            font-family: system-ui, -apple-system, sans-serif;
            max-width: 800px;
            margin: 0 auto;
            padding: 2rem;
            background: #1a1a2e;
            color: #eee;
        }}
        pre {{
            background: #16213e;
            padding: 1rem;
            border-radius: 8px;
            overflow-x: auto;
        }}
        code {{
            color: #00d9ff;
        }}
        p {{ color: #aaa; }}
        h1 {{ color: #00d9ff; }}
    </style>
</head>
<body>
    <h1>runts Development Server</h1>
    {}
</body>
</html>"#, content)
}

/// Generate error page
fn get_error_page(title: &str, message: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Error - runts</title>
    <style>
        body {{
            font-family: system-ui, -apple-system, sans-serif;
            max-width: 800px;
            margin: 0 auto;
            padding: 2rem;
            background: #1a1a2e;
            color: #eee;
        }}
        .error {{
            background: #3d1f1f;
            border: 1px solid #ff4444;
            border-radius: 8px;
            padding: 1rem;
        }}
        h1 {{ color: #ff4444; }}
        pre {{
            background: #2a1a1a;
            padding: 1rem;
            border-radius: 4px;
            overflow-x: auto;
            color: #ff8888;
        }}
    </style>
</head>
<body>
    <h1>{}</h1>
    <div class="error">
        <pre>{}</pre>
    </div>
</body>
</html>"#, title, message.replace('<', "&lt;").replace('>', "&gt;"))
}
