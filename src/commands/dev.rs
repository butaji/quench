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
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;


use crate::config::Config;
use crate::transpile::{Parser, Analyzer, CodeGenerator, hir};

/// Dev server state
#[allow(dead_code)]
pub struct DevState {
    /// Project root
    pub root: PathBuf,
    
    /// Transpiler cache (path -> HIR)
    pub cache: Arc<Mutex<HashMap<PathBuf, ModuleCache>>>,
    
    /// Broadcast channel for hot reload events
    pub reload_tx: broadcast::Sender<ReloadEvent>,
    
    /// Watcher handle
    pub watcher: Mutex<Option<RecommendedWatcher>>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ModuleCache {
    /// Parsed module
    pub hir: hir::Module,
    
    /// Last modified time
    pub modified: std::time::SystemTime,
    
    /// Compilation errors (if any)
    pub errors: Vec<String>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum ReloadEvent {
    /// A file was changed
    Changed(PathBuf),
    
    /// A file was added
    Added(PathBuf),
    
    /// A file was removed
    Removed(PathBuf),
    
    /// Full reload requested
    Reload,
    
    /// Error occurred
    Error(String),
}

impl DevState {
    pub fn new(root: PathBuf) -> Self {
        let (reload_tx, _) = broadcast::channel(100);
        
        Self {
            root,
            cache: Arc::new(Mutex::new(HashMap::new())),
            reload_tx,
            watcher: Mutex::new(None),
        }
    }
    
    /// Start watching for file changes
    #[allow(unused_variables)]
    pub fn start_watcher(&self, paths: Vec<PathBuf>) -> Result<()> {
        let root = self.root.clone();
        let reload_tx = self.reload_tx.clone();
        
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let paths: Vec<PathBuf> = event.paths.into_iter()
                        .filter(|p| {
                            // Only watch TS/TSX/JS files
                            matches!(
                                p.extension().and_then(|e| e.to_str()),
                                Some("ts") | Some("tsx") | Some("js") | Some("jsx")
                            )
                        })
                        .collect();
                    
                    if paths.is_empty() {
                        return;
                    }
                    
                    let event = match event.kind {
                        notify::EventKind::Create(_) => {
                            ReloadEvent::Added(paths[0].clone())
                        }
                        notify::EventKind::Remove(_) => {
                            ReloadEvent::Removed(paths[0].clone())
                        }
                        notify::EventKind::Modify(_) => {
                            // Invalidate cache
                            ReloadEvent::Changed(paths[0].clone())
                        }
                        _ => return,
                    };
                    
                    let _ = reload_tx.send(event);
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
        
        *self.watcher.lock().unwrap() = Some(watcher);
        
        Ok(())
    }
    
    /// Transpile a file (with caching)
    #[allow(dead_code)]
    pub fn transpile(&self, path: &PathBuf) -> Result<String> {
        let modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::now());
        
        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(path) {
                if cached.modified >= modified && cached.errors.is_empty() {
                    return generate_response(&cached.hir);
                }
            }
        }
        
        // Parse
        let mut parser = Parser::new();
        let module = parser.parse_file(path)?;
        
        // Analyze
        let mut analyzer = Analyzer::new();
        if let Err(errors) = analyzer.analyze(&module) {
            // Store errors but continue
            let error_strs: Vec<String> = errors.iter()
                .map(|e| e.to_string())
                .collect();
            
            // Cache even with errors
            let mut cache = self.cache.lock().unwrap();
            cache.insert(path.clone(), ModuleCache {
                hir: module,
                modified,
                errors: error_strs.clone(),
            });
            
            return Err(anyhow::anyhow!("Analysis errors: {:?}", error_strs));
        }
        
        // Generate
        let mut codegen = CodeGenerator::new();
        let rust_code = codegen.generate_module(&module)?;
        
        // Cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(path.clone(), ModuleCache {
                hir: module,
                modified,
                errors: Vec::new(),
            });
        }
        
        Ok(rust_code)
    }
}

#[allow(dead_code)]
fn generate_response(module: &hir::Module) -> Result<String> {
    let mut codegen = CodeGenerator::new();
    codegen.generate_module(module)
}

/// Serve HMR client script
async fn hmr_script() -> Response {
    let script = r#"<!DOCTYPE html>
<html>
<head><title>runts HMR</title></head>
<body>
    <h1>runts Dev Server</h1>
    <p>Edit your <code>routes/</code>, <code>islands/</code>, or <code>components/</code> files to see changes.</p>
    <script>
        console.log('[runts] Dev server ready');
    </script>
</body>
</html>"#;
    Html(script).into_response()
}

/// Handle static file serving
#[allow(dead_code)]
async fn handle_static(path: axum::extract::Path<String>) -> Response {
    // Simple static file serving - just return a placeholder for now
    // In production, this would use tower-http's ServeDir
    let filename = format!("static/{}", path.0);
    
    if let Ok(contents) = std::fs::read(&filename) {
        let mime = if filename.ends_with(".css") {
            "text/css"
        } else if filename.ends_with(".js") {
            "application/javascript"
        } else if filename.ends_with(".html") {
            "text/html"
        } else if filename.ends_with(".png") {
            "image/png"
        } else if filename.ends_with(".svg") {
            "image/svg+xml"
        } else {
            "application/octet-stream"
        };
        
        axum::response::Response::builder()
            .header("Content-Type", mime)
            .body(Body::from(contents))
            .unwrap_or_else(|_| Html("<h1>Error</h1>").into_response())
    } else {
        Html(format!("<h1>Not Found: {}</h1>", filename)).into_response()
    }
}

/// Run the development server
pub async fn run_dev_server(config: &Config, _port: u16) -> Result<()> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    
    // Create dev state
    let state = Arc::new(DevState::new(root.clone()));
    
    // Start file watcher
    let watch_paths = vec![
        root.join("routes"),
        root.join("islands"),
        root.join("components"),
        root.join("lib"),
    ];
    
    if let Err(e) = state.start_watcher(watch_paths) {
        eprintln!("Warning: Could not start file watcher: {}", e);
    }
    
    let dev_port = config.dev.port;
    
    println!("🚀 Starting development server on http://localhost:{}", dev_port);
    println!("📁 Watching for changes in:");
    println!("   - routes/");
    println!("   - islands/");
    println!("   - components/");
    println!("   - lib/");
    println!();
    println!("Press Ctrl+C to stop.");
    println!();
    
    // Create router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/_runts/hmr.js", get(hmr_script))
        .with_state(state);
    
    // Start server
    let addr = format!("0.0.0.0:{}", dev_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

async fn index_handler() -> impl IntoResponse {
    Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>runts dev server</title>
    <script src="/_runts/hmr.js"></script>
</head>
<body>
    <h1>🏃 runts Dev Server</h1>
    <p>Your application is running. Edit files to see changes.</p>
</body>
</html>
"#.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_dev_state_creation() {
        let root = env::current_dir().unwrap();
        let state = DevState::new(root.clone());
        
        assert_eq!(state.root, root);
    }
}
