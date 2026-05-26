//! Development server with hot reload
//!
//! In development mode, runts:
//! - Does NOT compile to Rust binaries
//! - Uses in-memory transpilation
//! - Executes HIR with Rust-based interpreter
//! - Provides instant hot reload

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::commands::routes::{HttpMethod, RouteTable};
use crate::commands::ssr::{IslandManifest, SsrRenderer};
use crate::commands::layouts::LayoutManager;
use crate::transpile::{Parser, Analyzer, CodeGenerator};

/// Application state shared across requests
#[derive(Clone)]
pub struct AppState {
    /// Project root
    pub root: PathBuf,
    
    /// Route table
    pub route_table: Arc<RwLock<RouteTable>>,
    
    /// SSR renderer
    pub ssr: Arc<SsrRenderer>,
    
    /// Layout manager
    pub layout_manager: Arc<LayoutManager>,
    
    /// Module cache
    pub module_cache: Arc<RwLock<HashMap<PathBuf, ModuleCacheEntry>>>,
    
    /// Broadcast channel for hot reload events
    pub reload_tx: broadcast::Sender<ReloadEvent>,
}

struct ModuleCacheEntry {
    code: String,
    modified: std::time::SystemTime,
    errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum ReloadEvent {
    Changed(PathBuf),
    Reload,
    Error(String),
}

impl AppState {
    /// Create new application state
    pub fn new(root: PathBuf, port: u16) -> Result<Self> {
        let routes_dir = root.join("routes");
        let islands_dir = root.join("islands");
        
        let route_table = RouteTable::from_routes_dir(&routes_dir)?;
        let ssr = SsrRenderer::new(routes_dir, islands_dir)?;
        let layout_manager = LayoutManager::from_routes_dir(&root.join("routes"))?;
        
        let (reload_tx, _) = broadcast::channel(100);
        
        Ok(Self {
            root,
            route_table: Arc::new(RwLock::new(route_table)),
            ssr: Arc::new(ssr),
            layout_manager: Arc::new(layout_manager),
            module_cache: Arc::new(RwLock::new(HashMap::new())),
            reload_tx,
        })
    }
    
    /// Start file watcher
    pub fn start_watcher(&self, paths: Vec<PathBuf>) -> Result<()> {
        let reload_tx = self.reload_tx.clone();
        let route_table = self.route_table.clone();
        
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let paths: Vec<PathBuf> = event.paths.into_iter()
                        .filter(|p| {
                            matches!(
                                p.extension().and_then(|e| e.to_str()),
                                Some("ts") | Some("tsx") | Some("js") | Some("jsx")
                            )
                        })
                        .collect();
                    
                    if paths.is_empty() {
                        return;
                    }
                    
                    // Reload route table if routes changed
                    if paths.iter().any(|p| {
                        p.to_string_lossy().contains("/routes/")
                    }) {
                        if let Ok(new_table) = RouteTable::from_routes_dir(
                            &std::env::current_dir().unwrap_or_default().join("routes")
                        ) {
                            *route_table.write() = new_table;
                        }
                    }
                    
                    let event = match event.kind {
                        notify::EventKind::Create(_) |
                        notify::EventKind::Modify(_) => {
                            ReloadEvent::Changed(paths[0].clone())
                        }
                        notify::EventKind::Remove(_) => ReloadEvent::Reload,
                        _ => return,
                    };
                    
                    let _ = reload_tx.send(event);
                    println!("[runts] File changed: {:?}", paths[0]);
                }
            },
            notify::Config::default(),
        )
        .context("Failed to create file watcher")?;
        
        for path in &paths {
            if path.exists() {
                let _ = watcher.watch(path, RecursiveMode::Recursive);
            }
        }
        
        Ok(())
    }
    
    /// Transpile a file
    pub fn transpile(&self, path: &PathBuf) -> Result<String> {
        let modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::now());
        
        {
            let cache = self.module_cache.read();
            if let Some(cached) = cache.get(path) {
                if cached.modified >= modified && cached.errors.is_empty() {
                    return Ok(cached.code.clone());
                }
            }
        }
        
        let mut parser = Parser::new();
        let module = parser.parse_file(path)?;
        
        let mut analyzer = Analyzer::new();
        if let Err(errors) = analyzer.analyze(&module) {
            let error_strs: Vec<String> = errors.iter()
                .map(|e| e.to_string())
                .collect();
            
            let mut cache = self.module_cache.write();
            cache.insert(path.clone(), ModuleCacheEntry {
                code: String::new(),
                modified,
                errors: error_strs.clone(),
            });
            
            return Err(anyhow::anyhow!("Analysis errors: {:?}", error_strs));
        }
        
        let mut codegen = CodeGenerator::new();
        let rust_code = codegen.generate_module(&module)?;
        
        {
            let mut cache = self.module_cache.write();
            cache.insert(path.clone(), ModuleCacheEntry {
                code: rust_code.clone(),
                modified,
                errors: Vec::new(),
            });
        }
        
        Ok(rust_code)
    }
}

/// Handle SSR request
async fn handle_ssr(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let path = if path.is_empty() { "/" } else { &path };
    let method = HttpMethod::GET;
    
    match state.ssr.render(path, method).await {
        Ok(result) => {
            let html = inject_island_manifest(result.html, &result.island_manifest);
            Html(html).into_response()
        }
        Err(e) => {
            let html = format!(
                r#"<!DOCTYPE html>
<html>
<head><title>404 - Not Found</title></head>
<body>
    <h1>404 Not Found</h1>
    <p>Route: {}</p>
    <pre>Error: {}</pre>
</body>
</html>"#,
                path, e
            );
            Html(html).into_response()
        }
    }
}

/// Handle static files
async fn handle_static(path: Path<String>) -> Response {
    let root = std::env::current_dir().unwrap_or_default();
    let static_dir = root.join("static");
    let file_path = static_dir.join(&*path);
    
    if !file_path.exists() {
        return Html("<h1>404 - File not found</h1>").into_response();
    }
    
    match tokio::fs::read(&file_path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string();
            
            Response::builder()
                .header("Content-Type", mime)
                .body(Body::from(contents))
                .unwrap_or_else(|_| Html("<h1>Error</h1>").into_response())
        }
        Err(_) => Html("<h1>Error reading file</h1>").into_response(),
    }
}

/// Serve island manifest
async fn handle_island_manifest(State(_state): State<AppState>) -> Response {
    let manifest = IslandManifest::new();
    let json = serde_json::to_string(&manifest).unwrap_or_default();
    
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(json))
        .unwrap()
}

/// Serve island JS bundle
async fn handle_island_bundle(Path(name): Path<String>) -> Response {
    let bundle = format!(
        r#"// Island: {}
export const hydrate = (id, props) => {{
    console.log('[runts] Hydrating island:', id, props);
}};
"#,
        name
    );
    
    Response::builder()
        .header("Content-Type", "application/javascript")
        .body(Body::from(bundle))
        .unwrap()
}

/// SSE endpoint for HMR
async fn handle_hmr_sse(State(_state): State<AppState>) -> Response {
    // For now, return a simple response indicating HMR is ready
    // Full SSE implementation would require a different approach with tokio::sync::broadcast
    let body = "data: {\"type\": \"connected\"}\n\n";
    
    Response::builder()
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from(body))
        .unwrap()
}

/// HMR client script
async fn hmr_client_script() -> Response {
    let script = r#"
(function() {
    const source = new EventSource('/_runts/hmr');
    
    source.onmessage = (event) => {
        const data = JSON.parse(event.data);
        
        if (data.type === 'reload') {
            console.log('[runts HMR] Full reload requested');
            window.location.reload();
        } else if (data.type === 'change') {
            console.log('[runts HMR] File changed:', data.path);
            window.location.reload();
        } else if (data.type === 'error') {
            console.error('[runts HMR] Error:', data.message);
        }
    };
    
    source.onerror = () => {
        console.error('[runts HMR] Connection lost, retrying...');
    };
    
    console.log('[runts HMR] Connected');
})();
"#;
    
    Response::builder()
        .header("Content-Type", "application/javascript")
        .body(Body::from(script))
        .unwrap()
}

fn inject_island_manifest(html: String, manifest: &IslandManifest) -> String {
    let manifest_json = serde_json::to_string(manifest).unwrap_or_default();
    
    format!(
        r#"{}<script id="__runts_manifest" type="application/json">{}</script>"#,
        html, manifest_json
    )
}

/// Run the development server
pub async fn run_dev_server(config: &Config, _port: u16) -> Result<()> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dev_port = config.dev.port;
    
    let state = AppState::new(root.clone(), dev_port)?;
    
    let watch_paths = vec![
        root.join("routes"),
        root.join("islands"),
        root.join("components"),
        root.join("lib"),
    ];
    
    if let Err(e) = state.start_watcher(watch_paths) {
        eprintln!("Warning: Could not start file watcher: {}", e);
    }
    
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    runts Dev Server                         ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  URL:        http://localhost:{}                           ║", dev_port);
    println!("║  Root:       {}  ", root.to_string_lossy());
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Watching:                                                    ║");
    println!("║    • routes/     (route handlers & pages)                     ║");
    println!("║    • islands/   (interactive components)                     ║");
    println!("║    • components/(static components)                         ║");
    println!("║    • lib/       (shared code)                               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Press Ctrl+C to stop.\n");
    
    let app = Router::new()
        .nest_service("/static", ServeDir::new(root.join("static")))
        .route("/_runts/manifest.json", get(handle_island_manifest))
        .route("/_runts/islands/:name", get(handle_island_bundle))
        .route("/_runts/hmr", get(handle_hmr_sse))
        .route("/_runts/hmr.js", get(hmr_client_script))
        .route("/*path", get(handle_ssr))
        .route("/", get(handle_ssr))
        .layer(CorsLayer::permissive())
        .with_state(state);
    
    let addr = format!("0.0.0.0:{}", dev_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    println!("[runts] Server listening on http://localhost:{}\n", dev_port);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_app_state_creation() {
        let root = env::current_dir().unwrap();
        let state = AppState::new(root.clone(), 8080);
        assert!(state.is_ok());
    }
}
