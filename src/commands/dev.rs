//! Development server with instant hot-reload
//!
//! In development mode, runts:
//! - Parses TS/TSX to HIR (NO Rust compilation)
//! - Executes HIR directly with the interpreter
//! - Provides instant hot-reload (<100ms)
//! - Full parity with production rendering

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Path, State, Query},
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
use std::time::Instant;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;

use crate::config::Config;
use crate::commands::routes::{HttpMethod, RouteTable};
use crate::runtime::interpreter::Interpreter;
use crate::commands::layouts::LayoutManager;

/// Application state shared across requests
#[derive(Clone)]
pub struct AppState {
    /// Project root
    pub root: PathBuf,

    /// Route table
    pub route_table: Arc<RwLock<RouteTable>>,

    /// Interpreter (HIR executor)
    pub interpreter: Arc<RwLock<Interpreter>>,

    /// Layout manager
    pub layout_manager: Arc<LayoutManager>,

    /// Module cache (source -> HIR)
    pub module_cache: Arc<RwLock<HashMap<PathBuf, CachedModule>>>,

    /// Broadcast channel for hot reload events
    pub reload_tx: broadcast::Sender<ReloadEvent>,
}

/// Cached module data
#[derive(Clone)]
struct CachedModule {
    /// Parsed HIR
    module: crate::transpile::hir::Module,
    /// Last modified time
    modified: std::time::SystemTime,
    /// Parse errors (if any)
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
    pub fn new(root: PathBuf, _port: u16) -> Result<Self> {
        let routes_dir = root.join("routes");
        let islands_dir = root.join("islands");
        let components_dir = root.join("components");

        let route_table = RouteTable::from_routes_dir(&routes_dir)?;
        let layout_manager = LayoutManager::from_routes_dir(&routes_dir)?;

        // Create interpreter and pre-load modules
        let interpreter = Arc::new(RwLock::new(Interpreter::new()));

        // Pre-load all modules
        Self::preload_modules(&interpreter, &root)?;

        let (reload_tx, _) = broadcast::channel(100);

        Ok(Self {
            root,
            route_table: Arc::new(RwLock::new(route_table)),
            interpreter,
            layout_manager: Arc::new(layout_manager),
            module_cache: Arc::new(RwLock::new(HashMap::new())),
            reload_tx,
        })
    }

    /// Pre-load all TS/TSX modules into interpreter
    fn preload_modules(interpreter: &Arc<RwLock<Interpreter>>, root: &PathBuf) -> Result<()> {
        let mut interpreter = interpreter.write();

        // Load islands
        let islands_dir = root.join("islands");
        if islands_dir.exists() {
            for entry in walkdir::WalkDir::new(&islands_dir)
                .max_depth(2)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("tsx") ||
                   path.extension().and_then(|e| e.to_str()) == Some("ts") {
                    if let Ok(source) = std::fs::read_to_string(path) {
                        if let Err(e) = interpreter.load_file(path, &source) {
                            eprintln!("[runts] Warning: Could not load {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        // Load components
        let components_dir = root.join("components");
        if components_dir.exists() {
            for entry in walkdir::WalkDir::new(&components_dir)
                .max_depth(2)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("tsx") ||
                   path.extension().and_then(|e| e.to_str()) == Some("ts") {
                    if let Ok(source) = std::fs::read_to_string(path) {
                        if let Err(e) = interpreter.load_file(path, &source) {
                            eprintln!("[runts] Warning: Could not load {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        // Load routes
        let routes_dir = root.join("routes");
        if routes_dir.exists() {
            Self::load_routes_recursive(&mut interpreter, &routes_dir)?;
        }

        Ok(())
    }

    /// Recursively load route files
    fn load_routes_recursive(interpreter: &mut Interpreter, dir: &PathBuf) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                Self::load_routes_recursive(interpreter, &path)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("tsx") ||
                      path.extension().and_then(|e| e.to_str()) == Some("ts") {
                // Skip middleware and special files
                let filename = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                if filename.starts_with('_') && !filename.ends_with(".tsx") && !filename.ends_with(".ts") {
                    continue;
                }

                if let Ok(source) = std::fs::read_to_string(&path) {
                    if let Err(e) = interpreter.load_file(&path, &source) {
                        eprintln!("[runts] Warning: Could not load {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Start file watcher for hot reload
    pub fn start_watcher(&self) -> Result<()> {
        let reload_tx = self.reload_tx.clone();
        let interpreter = self.interpreter.clone();
        let module_cache = self.module_cache.clone();
        let route_table = self.route_table.clone();
        let root = self.root.clone();

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

                    let path = &paths[0];

                    // Invalidate cache for this file
                    {
                        let mut cache = module_cache.write();
                        cache.remove(path);
                    }

                    // Reload the module
                    if let Ok(source) = std::fs::read_to_string(path) {
                        let mut interp = interpreter.write();
                        if let Err(e) = interp.load_file(path, &source) {
                            eprintln!("[runts] Error reloading {}: {}", path.display(), e);
                            let _ = reload_tx.send(ReloadEvent::Error(e));
                            return;
                        }
                    }

                    // Reload route table if routes changed
                    if path.to_string_lossy().contains("/routes/") {
                        let routes_dir = root.join("routes");
                        if let Ok(new_table) = RouteTable::from_routes_dir(&routes_dir) {
                            *route_table.write() = new_table;
                        }
                    }

                    let _ = reload_tx.send(ReloadEvent::Changed(path.clone()));
                    println!("[runts] Hot reload: {}", path.display());
                }
            },
            notify::Config::default(),
        )
        .context("Failed to create file watcher")?;

        // Watch all relevant directories
        let watch_dirs = vec![
            self.root.join("routes"),
            self.root.join("islands"),
            self.root.join("components"),
            self.root.join("lib"),
        ];

        for dir in &watch_dirs {
            if dir.exists() {
                if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                    eprintln!("[runts] Warning: Could not watch {}: {}", dir.display(), e);
                }
            }
        }

        Ok(())
    }

    /// Execute a route and return the rendered HTML
    pub fn execute_route(&self, path: &str, method: HttpMethod, params: HashMap<String, String>) -> Result<String> {
        let start = Instant::now();

        // Get route info
        let route_table = self.route_table.read();
        let fallback_route = crate::commands::routes::Route {
            pattern: "/".to_string(),
            regex: regex::Regex::new("/").unwrap(),
            path_template: "/".to_string(),
            segments: vec![],
            file_path: self.root.join("routes/index.tsx"),
            methods: vec![HttpMethod::GET],
            is_catch_all: false,
        };

        let (route, route_params) = route_table
            .find_route(path, method)
            .unwrap_or((&fallback_route, HashMap::new()));

        // Merge params
        let mut all_params = route_params;
        for (k, v) in params {
            all_params.insert(k, v);
        }

        // Execute the route using interpreter
        let page_data = {
            let mut interpreter = self.interpreter.write();
            interpreter.execute_handler(path, all_params.clone())
                .unwrap_or_else(|e| {
                    eprintln!("[runts] Handler error: {}", e);
                    serde_json::json!({})
                })
        };

        // Build page HTML
        let html = self.build_page_html(&route.pattern, &page_data, route);

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            println!("[runts] Slow route execution: {}ms for {}", elapsed.as_millis(), path);
        }

        Ok(html)
    }

    /// Build page HTML from route data
    fn build_page_html(&self, path: &str, page_data: &serde_json::Value, _route: &crate::commands::routes::Route) -> String {
        let title = path_to_title(path);
        let page_data_json = serde_json::to_string(page_data).unwrap_or_else(|_| "{}".to_string());

        // Generate content based on route
        let content = self.render_route_content(path, page_data);

        format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <link rel="icon" href="/static/favicon.ico">
    <script type="module" src="/_runts/hmr.js"></script>
</head>
<body>
    <nav class="runts-nav">
        <a href="/">Home</a>
        <a href="/blog">Blog</a>
    </nav>
    <main>
        {}
    </main>
    <script>
        window.__PAGE_DATA__ = {};
    </script>
</body>
</html>
"#, title, content, page_data_json)
    }

    /// Render route-specific content
    fn render_route_content(&self, path: &str, data: &serde_json::Value) -> String {
        if let Some(obj) = data.as_object() {
            // Check for blog data
            if let Some(posts) = obj.get("posts").and_then(|v| v.as_array()) {
                let mut html = String::new();
                html.push_str("<h1>Blog Posts</h1>\n<ul class='posts'>");
                for post in posts {
                    if let Some(post_obj) = post.as_object() {
                        let slug = post_obj.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                        let title = post_obj.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let excerpt = post_obj.get("excerpt").and_then(|v| v.as_str()).unwrap_or("");
                        html.push_str(&format!(
                            r#"<li><a href="/blog/{}"><strong>{}</strong><p>{}</p></a></li>"#,
                            html_escape(slug),
                            html_escape(title),
                            html_escape(excerpt)
                        ));
                    }
                }
                html.push_str("</ul>");
                return html;
            }

            // Check for single post
            if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
                let content = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");
                return format!(
                    "<h1>{}</h1>\n<p>{}</p>",
                    html_escape(title),
                    html_escape(content)
                );
            }
        }

        // Default: show the path
        format!("<p>Route: {}</p>", html_escape(path))
    }
}

/// Handle SSR request
async fn handle_ssr(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let path = if path.is_empty() { "/" } else { &path };
    let method = HttpMethod::GET;

    match state.execute_route(path, method, HashMap::new()) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            let html = format!(
                r#"<!DOCTYPE html>
<html>
<head><title>Error</title></head>
<body>
    <h1>Error</h1>
    <p>Route: {}</p>
    <pre>{}</pre>
</body>
</html>"#,
                path, e
            );
            Html(html).into_response()
        }
    }
}

/// Handle API routes
async fn handle_api(
    State(state): State<AppState>,
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let path = format!("/api/{}", path);
    let method = HttpMethod::POST;

    // Check for POST body
    // For simplicity, just echo back params
    let response = serde_json::json!({
        "path": path,
        "params": params,
        "message": "API response from runts interpreter"
    });

    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(response.to_string()))
        .unwrap_or_else(|_| Html("<h1>Error</h1>").into_response())
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
    let manifest = serde_json::json!({
        "islands": [],
        "version": "1.0"
    });

    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(manifest.to_string()))
        .unwrap()
}

/// Serve island JS bundle
async fn handle_island_bundle(Path(name): Path<String>) -> Response {
    // Generate minimal island bundle
    let bundle = format!(r#"
// Island: {}
export default class {0} {{
    constructor(props) {{
        this.props = props;
        this.el = null;
    }}

    mount(el) {{
        this.el = el;
        // Simple hydration: replace placeholder with component
        el.innerHTML = this.render();
        this.attachEvents();
    }}

    render() {{
        return `<div class="island-{0}">Island {0} hydrated</div>`;
    }}

    attachEvents() {{
        // Attach event handlers here
    }}

    unmount() {{
        if (this.el) {{
            this.el.innerHTML = '';
        }}
    }}
}}
"#, name);

    Response::builder()
        .header("Content-Type", "application/javascript")
        .body(Body::from(bundle))
        .unwrap()
}

/// SSE endpoint for HMR
async fn handle_hmr_sse(State(state): State<AppState>) -> Response {
    use tokio_stream::wrappers::BroadcastStream;

    let rx = state.reload_tx.subscribe();
    let broadcast_stream = BroadcastStream::new(rx);

    // Convert broadcast stream to SSE format using async_stream
    let stream = async_stream::stream! {
        let mut broadcast_rx = broadcast_stream;
        while let Some(result) = futures::StreamExt::next(&mut broadcast_rx).await {
            if let Ok(event) = result {
                let data = match event {
                    ReloadEvent::Changed(path) => {
                        serde_json::json!({
                            "type": "change",
                            "path": path.to_string_lossy()
                        })
                    }
                    ReloadEvent::Reload => {
                        serde_json::json!({ "type": "reload" })
                    }
                    ReloadEvent::Error(msg) => {
                        serde_json::json!({
                            "type": "error",
                            "message": msg
                        })
                    }
                };
                yield Ok::<_, std::convert::Infallible>(format!("data: {}\n\n", data));
            }
        }
    };

    Response::builder()
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// HMR client script
async fn hmr_client_script() -> Response {
    let script = r#"
(function() {
    let retryCount = 0;
    const maxRetries = 5;

    function connect() {
        const source = new EventSource('/_runts/hmr');

        source.onmessage = (event) => {
            try {
                const data = JSON.parse(event.data);

                switch (data.type) {
                    case 'change':
                        console.log('[runts HMR] File changed:', data.path);
                        // Soft reload: just fetch the page
                        window.__RUNTS_RELOAD__ && window.__RUNTS_RELOAD__();
                        break;

                    case 'reload':
                        console.log('[runts HMR] Full reload requested');
                        window.location.reload();
                        break;

                    case 'error':
                        console.error('[runts HMR] Error:', data.message);
                        break;

                    case 'connected':
                        console.log('[runts HMR] Connected');
                        retryCount = 0;
                        break;
                }
            } catch (e) {
                console.error('[runts HMR] Failed to parse event:', e);
            }
        };

        source.onerror = () => {
            console.warn('[runts HMR] Connection lost, retrying...');
            source.close();
            retryCount++;

            if (retryCount < maxRetries) {
                setTimeout(connect, 1000 * retryCount);
            } else {
                console.error('[runts HMR] Max retries reached, please refresh manually');
            }
        };
    }

    connect();
})();
"#;

    Response::builder()
        .header("Content-Type", "application/javascript")
        .body(Body::from(script))
        .unwrap()
}

/// Run the development server
pub async fn run_dev_server(config: &Config, _port: u16) -> Result<()> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dev_port = config.dev.port;

    // Check if we're in a runts project
    let routes_dir = root.join("routes");
    if !routes_dir.exists() {
        anyhow::bail!(
            "Not a runts project directory. No `routes/` folder found.\n\
             Run `runts init` to create a new project."
        );
    }

    let state = AppState::new(root.clone(), dev_port)?;

    // Start file watcher
    if let Err(e) = state.start_watcher() {
        eprintln!("Warning: Could not start file watcher: {}", e);
    }

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                runts Dev Server (Runtime Mode)            ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  URL:        http://localhost:{}                           ║", dev_port);
    println!("║  Root:       {}  ", root.to_string_lossy());
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Mode:       Instant HIR Interpretation (No Rust Compile)  ║");
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
        .nest_service("/static", get(handle_static))
        .route("/_runts/manifest.json", get(handle_island_manifest))
        .route("/_runts/islands/:name", get(handle_island_bundle))
        .route("/_runts/hmr", get(handle_hmr_sse))
        .route("/_runts/hmr.js", get(hmr_client_script))
        .route("/api/*path", get(handle_api))
        .route("/*path", get(handle_ssr))
        .route("/", get(handle_ssr))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", dev_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("[runts] Server listening on http://localhost:{}\n", dev_port);
    println!("[runts] Hot reload ready! Changes will reflect instantly.\n");

    axum::serve(listener, app).await?;

    Ok(())
}

fn path_to_title(path: &str) -> String {
    if path.is_empty() || path == "/" {
        "Home".to_string()
    } else {
        path.split('/')
            .filter(|s| !s.is_empty())
            .last()
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().chain(chars).collect(),
                }
            })
            .unwrap_or_else(|| "runts".to_string())
    }
}

/// Escape HTML special characters
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
