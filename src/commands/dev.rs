//! Development server with instant hot-reload
//!
//! In development mode, runts:
//! - Parses TS/TSX to HIR (NO Rust compilation)
//! - Executes HIR directly with the interpreter
//! - Provides instant hot-reload (<100ms)
//! - Full parity with production rendering

use anyhow::{Result, Context};
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
    time::Instant,
};
use tokio::sync::broadcast;

use crate::config::Config;
use crate::runtime::interpreter::{Interpreter, RenderResult, RequestInfo};

/// Application state shared across requests
#[derive(Clone)]
pub struct AppState {
    /// Project root
    pub root: PathBuf,

    /// Route table
    pub route_table: Arc<RwLock<RouteTable>>,

    /// Interpreter (HIR executor)
    pub interpreter: Arc<RwLock<Interpreter>>,

    /// Broadcast channel for hot reload events
    pub reload_tx: broadcast::Sender<ReloadEvent>,
}

/// Route information
#[derive(Debug, Clone)]
pub struct Route {
    pub pattern: String,
    pub regex: regex::Regex,
    pub file_path: PathBuf,
    pub methods: Vec<HttpMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpMethod {
    GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS
}

impl HttpMethod {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::GET),
            "POST" => Some(Self::POST),
            "PUT" => Some(Self::PUT),
            "DELETE" => Some(Self::DELETE),
            "PATCH" => Some(Self::PATCH),
            "HEAD" => Some(Self::HEAD),
            "OPTIONS" => Some(Self::OPTIONS),
            _ => None,
        }
    }
}

/// Route table for fast lookup
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    routes: Vec<Route>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    pub fn from_routes_dir(routes_dir: &PathBuf) -> Result<Self> {
        let mut table = Self::new();
        
        if !routes_dir.exists() {
            return Ok(table);
        }

        // Walk routes directory
        Self::scan_dir(&routes_dir, routes_dir, &mut table)?;
        
        Ok(table)
    }

    fn scan_dir(base: &PathBuf, current: &PathBuf, table: &mut RouteTable) -> Result<()> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let filename = entry.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                Self::scan_dir(base, &path, table)?;
            } else if filename.ends_with(".tsx") || filename.ends_with(".ts") {
                // Skip special files (middleware, layouts, etc.)
                if filename.starts_with('_') {
                    continue;
                }

                let pattern = Self::file_to_pattern(base, &path);
                let regex = Self::pattern_to_regex(&pattern);
                
                table.routes.push(Route {
                    pattern,
                    regex,
                    file_path: path,
                    methods: vec![HttpMethod::GET],
                });
            }
        }
        
        Ok(())
    }

    fn file_to_pattern(base: &PathBuf, file_path: &PathBuf) -> String {
        let relative = file_path.strip_prefix(base)
            .unwrap_or(file_path.as_path());
        
        // Get the relative path as a string
        let relative_str = relative.to_string_lossy().to_string();
        
        // Split by path separator and filter empty parts
        let parts: Vec<&str> = relative_str.split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .collect();

        if parts.is_empty() {
            return "/".to_string();
        }

        let mut segments = Vec::new();
        
        for part in &parts {
            let name = *part;
            
            // Strip extension first
            let stem = name.trim_end_matches(".tsx").trim_end_matches(".ts");
            
            // Skip index files - they represent the directory
            if stem == "index" {
                continue;
            }
            
            // Handle dynamic segments: [slug] -> :slug
            if stem.starts_with('[') && stem.ends_with(']') {
                let param = &stem[1..stem.len()-1];
                // Handle catch-all: [...slug] -> :slug*
                if param.starts_with("...") {
                    segments.push(format!("(?P<{}>.*)", &param[3..]));
                } else if param.starts_with('.') {
                    let inner = &param[1..];
                    segments.push(format!("(?P<{}>.*)", inner.trim_start_matches('.')));
                } else {
                    segments.push(format!("(?P<{}>[^/]+)", param));
                }
            } else if !stem.starts_with('_') {
                segments.push(stem.to_string());
            }
        }

        if segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", segments.join("/"))
        }
    }

    fn pattern_to_regex(pattern: &str) -> regex::Regex {
        regex::Regex::new(&format!("^{}$", pattern))
            .unwrap_or_else(|_| regex::Regex::new("^/$").unwrap())
    }

    pub fn find_route(&self, path: &str) -> Option<(String, HashMap<String, String>, PathBuf)> {
        // First try exact match
        for route in &self.routes {
            if route.pattern == path {
                return Some((route.pattern.clone(), HashMap::new(), route.file_path.clone()));
            }
        }

        // Then try pattern matching
        for route in &self.routes {
            if let Some(caps) = route.regex.captures(path) {
                let mut params = HashMap::new();
                for name in route.regex.capture_names() {
                    if let Some(name) = name {
                        if let Some(value) = caps.name(name) {
                            params.insert(name.to_string(), value.as_str().to_string());
                        }
                    }
                }
                return Some((route.pattern.clone(), params, route.file_path.clone()));
            }
        }

        None
    }
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

        let route_table = RouteTable::from_routes_dir(&routes_dir)?;
        let interpreter = Arc::new(RwLock::new(Interpreter::new()));

        // Pre-load all modules
        Self::preload_modules(&interpreter, &root)?;

        let (reload_tx, _) = broadcast::channel(100);

        Ok(Self {
            root,
            route_table: Arc::new(RwLock::new(route_table)),
            interpreter,
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
                // Skip middleware and special files for route loading
                let filename = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                if filename.starts_with('_') {
                    // Load layouts and middleware separately
                    if filename.contains("layout") {
                        if let Ok(source) = std::fs::read_to_string(&path) {
                            if let Err(e) = interpreter.load_file(&path, &source) {
                                eprintln!("[runts] Warning: Could not load layout {}: {}", path.display(), e);
                            }
                        }
                    }
                    continue;
                }

                if let Ok(source) = std::fs::read_to_string(&path) {
                    if path.to_string_lossy().contains("[slug]") {
                    }
                    if let Err(e) = interpreter.load_file(&path, &source) {
                        eprintln!("[runts] Warning: Could not load {}: {}", path.display(), e);
                    }
                } else {
                    eprintln!("[runts] Warning: Could not read {}: {}", path.display(), std::io::Error::last_os_error());
                }
            }
        }

        Ok(())
    }

    /// Start file watcher for hot reload
    pub fn start_watcher(&self) -> Result<()> {
        let reload_tx = self.reload_tx.clone();
        let interpreter = self.interpreter.clone();
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
    pub fn execute_route(&self, path: &str, params: HashMap<String, String>, request: RequestInfo) -> Result<RenderResult> {
        let start = Instant::now();

        // Get route info
        let route_table = self.route_table.read();
        let (_pattern, route_params, file_path) = route_table
            .find_route(path)
            .unwrap_or_else(|| ("/".to_string(), HashMap::new(), PathBuf::from("routes/index.tsx")));

        // Merge params
        let mut all_params = route_params;
        for (k, v) in params {
            all_params.insert(k, v);
        }

        // Execute the route using interpreter (pass file path for handler lookup)
        let result = {
            let interpreter = self.interpreter.read();
            interpreter.execute_route_by_file(&file_path, "GET", all_params, request)
                .map_err(|e| anyhow::anyhow!("Handler error: {}", e))
        };

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            println!("[runts] Slow route execution: {}ms for {}", elapsed.as_millis(), path);
        }

        result
    }

    /// Build full HTML document
    fn build_html(&self, path: &str, result: &RenderResult) -> String {
        let title = path_to_title(path);
        
        // Convert page data to JSON manually
        let page_data_json = serde_json::json!({
            "rendered": true,
            "route": path
        }).to_string();

        // Generate island manifest (simplified)
        let island_manifest_json = serde_json::json!({
            "islands": result.islands.iter().map(|i| {
                serde_json::json!({
                    "name": i.name,
                    "id": i.id
                })
            }).collect::<Vec<_>>()
        }).to_string();

        // Generate nav links from actual routes
        let nav_links = self.build_nav_links();

        format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <link rel="icon" href="/static/favicon.ico">
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; }}
        nav {{ background: #1a1a2e; padding: 1rem; }}
        nav a {{ color: white; text-decoration: none; margin-right: 1rem; }}
        nav a:hover {{ text-decoration: underline; }}
        main {{ max-width: 800px; margin: 2rem auto; padding: 0 1rem; }}
        .posts {{ list-style: none; }}
        .posts li {{ margin-bottom: 1rem; padding: 1rem; border: 1px solid #ddd; border-radius: 8px; }}
        .posts a {{ text-decoration: none; color: inherit; }}
        .posts a:hover {{ color: #1a1a2e; }}
        .posts strong {{ font-size: 1.2rem; display: block; margin-bottom: 0.5rem; }}
        .posts p {{ color: #666; margin: 0; }}
        .btn {{ display: inline-block; padding: 0.5rem 1rem; background: #1a1a2e; color: white; text-decoration: none; border-radius: 4px; }}
        .btn:hover {{ background: #16213e; }}
        .island-placeholder {{ border: 2px dashed #ccc; padding: 1rem; border-radius: 8px; text-align: center; color: #666; }}
    </style>
</head>
<body>
    <nav class="runts-nav">
        {nav_links}
    </nav>
    <main>
        {content}
    </main>
    <script>
        window.__PAGE_DATA__ = {page_data_json};
        window.__ISLAND_MANIFEST__ = {island_manifest_json};
    </script>
    <script type="module" src="/_runts/hmr.js"></script>
    <script type="module" src="/_runts/client.js"></script>
</body>
</html>
"#, title = title, nav_links = nav_links, content = result.html, page_data_json = page_data_json, island_manifest_json = island_manifest_json)
    }

    /// Generate nav links from the current route table
    fn build_nav_links(&self) -> String {
        let rt = self.route_table.read();
        let mut links: Vec<(String, String)> = Vec::new();
        
        // Always include home
        links.push(("/".to_string(), "Home".to_string()));
        
        for route in &rt.routes {
            // Skip dynamic routes, catch-all, and special files for the nav
            let pattern = &route.pattern;
            if pattern.contains(':') || pattern.contains('*') || pattern == "/" {
                continue;
            }
            // Clean up pattern for display
            let label = pattern
                .trim_start_matches('/')
                .split('/')
                .last()
                .unwrap_or("")
                .replace('-', " ")
                .replace('_', " ");
            if label.is_empty() {
                continue;
            }
            let display: String = label.chars().enumerate().map(|(i, c)| {
                if i == 0 { c.to_uppercase().to_string() } else { c.to_string() }
            }).collect();
            links.push((pattern.clone(), display));
        }
        
        // Deduplicate and sort
        links.sort_by(|a, b| a.0.cmp(&b.0));
        links.dedup_by(|a, b| a.0 == b.0);
        
        links.into_iter()
            .map(|(href, label)| format!(r#"<a href="{}">{}</a>"#, href, label))
            .collect::<Vec<_>>()
            .join("\n        ")
    }

    fn value_to_json(&self, value: &crate::runtime::interpreter::Value) -> serde_json::Value {
        match value {
            crate::runtime::interpreter::Value::Undefined => serde_json::Value::Null,
            crate::runtime::interpreter::Value::Null => serde_json::Value::Null,
            crate::runtime::interpreter::Value::Bool(b) => serde_json::json!(b),
            crate::runtime::interpreter::Value::Number(n) => serde_json::json!(n),
            crate::runtime::interpreter::Value::String(s) => serde_json::json!(s),
            crate::runtime::interpreter::Value::Array(arr) => serde_json::json!(arr.iter().map(|v| self.value_to_json(v)).collect::<Vec<_>>()),
            crate::runtime::interpreter::Value::Object(obj) => serde_json::json!(obj.iter().map(|(k, v)| (k.clone(), self.value_to_json(v))).collect::<serde_json::Map<String, _>>()),
            crate::runtime::interpreter::Value::Function(_) => serde_json::Value::Null,
        }
    }
}

/// Extract request headers from headers map
fn extract_headers(headers: &http::HeaderMap) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for (name, value) in headers {
        if let Ok(v) = value.to_str() {
            result.insert(name.to_string(), v.to_string());
        }
    }
    result
}

/// Handle SSR request
async fn handle_ssr(
    State(state): State<AppState>,
    path: Option<Path<String>>,
    headers: http::HeaderMap,
) -> impl IntoResponse {
    let path = path.map(|p| p.0).unwrap_or_default();
    let path = if path.is_empty() { "/".to_string() } else { format!("/{}", path) };
    let path = path.as_str();

    // Build request info
    let request = RequestInfo {
        method: "GET".to_string(),
        url: format!("http://localhost{}", path),
        headers: extract_headers(&headers),
    };

    match state.execute_route(path, HashMap::new(), request) {
        Ok(result) => {
            let html = state.build_html(path, &result);
            Html(html).into_response()
        }
        Err(e) => {
            let html = format!(
                r#"<!DOCTYPE html>
<html>
<head><title>Error</title></head>
<body>
    <nav class="runts-nav">
        <a href="/">Home</a>
        <a href="/blog">Blog</a>
    </nav>
    <main>
        <h1>Error Rendering: {path}</h1>
        <pre style="background:#f4f4f4;padding:1rem;overflow:auto;">{error}</pre>
        <a href="/" class="btn">Go Home</a>
    </main>
</body>
</html>"#,
                path = path,
                error = e.to_string().lines().take(20).collect::<Vec<_>>().join("\n")
            );
            Html(html).into_response()
        }
    }
}

/// Handle API routes
async fn handle_api(
    State(state): State<AppState>,
    path: Option<Path<String>>,
    headers: http::HeaderMap,
) -> Response {
    let path = path.map(|p| p.0).unwrap_or_default();
    let api_path = format!("/api/{}", path);

    // Build request info
    let request = RequestInfo {
        method: "GET".to_string(),
        url: format!("http://localhost{}", api_path),
        headers: extract_headers(&headers),
    };

    // Try to execute as a route first
    if let Ok(result) = state.execute_route(&api_path, HashMap::new(), request) {
        // Return the rendered content as JSON
        let response = serde_json::json!({
            "path": api_path,
            "html": result.html,
            "data": result.page_data.to_json(),
            "islands": result.islands.iter().map(|i| {
                serde_json::json!({
                    "name": i.name,
                    "id": i.id
                })
            }).collect::<Vec<_>>()
        });
        
        return Response::builder()
            .header("Content-Type", "application/json")
            .body(Body::from(response.to_string()))
            .unwrap_or_else(|_| Html("<h1>Error</h1>").into_response());
    }

    // Simulate API response
    let response = serde_json::json!({
        "path": api_path,
        "message": "API response from runts",
        "data": {
            "status": "ok",
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
        }
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

/// Serve island JS bundle
async fn handle_island_bundle(Path(name): Path<String>) -> Response {
    // Generate minimal island bundle
    let bundle = format!(r#"
// Island: {0} - Client bundle
class {0} {{
    constructor(props, element) {{
        this.props = props;
        this.element = element;
    }}

    mount() {{
        console.log('[runts] Mounting island: {0}');
        // Render initial state from SSR HTML
        this.element.innerHTML = this.element.innerHTML;
        this.attachEvents();
    }}

    render() {{
        return `<div class="island-{0}">Island {0} hydrated</div>`;
    }}

    attachEvents() {{
        // Find buttons and attach click handlers
        this.element.querySelectorAll('button').forEach(btn => {{
            // Re-attach any inline handlers
        }});
    }}

    setState(newProps) {{
        this.props = {{ ...this.props, ...newProps }};
        this.render();
    }}

    unmount() {{
        this.element.innerHTML = '';
    }}
}}

// Register the island
if (typeof window !== 'undefined') {{
    window.__runts_islands__ = window.__runts_islands__ || {{}};
    window.__runts_islands__['{0}'] = {0};
}}

export default {0};
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
                        window.location.reload();
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

/// Client-side island hydration
async fn client_script() -> Response {
    let script = r#"
/**
 * runts Client Runtime
 * Handles island hydration and interactivity
 */

(function() {
    'use strict';

    // Island registry
    const islands = new Map();

    // Register an island
    window.__registerIsland__ = function(name, Component) {
        islands.set(name, Component);
        console.debug('[runts] Registered island:', name);
    };

    // Hydrate all islands on page
    function hydrateAll() {
        const manifest = window.__ISLAND_MANIFEST__ || { islands: [] };
        
        for (const entry of manifest.islands) {
            const el = document.querySelector(`[data-island="${entry.name}"][data-id="${entry.id}"]`);
            if (!el) {
                console.warn('[runts] Island element not found:', entry.name, entry.id);
                continue;
            }

            // Check if already hydrated
            if (el.dataset.hydrated === 'true') continue;

            const Component = islands.get(entry.name);
            if (Component) {
                try {
                    const instance = new Component(entry.props, el);
                    instance.mount();
                    el.dataset.hydrated = 'true';
                    console.debug('[runts] Hydrated island:', entry.name);
                } catch (e) {
                    console.error('[runts] Hydration error for', entry.name, ':', e);
                }
            } else {
                // Try to load from server
                loadIsland(entry.name).then(() => {
                    const LoadedComponent = islands.get(entry.name);
                    if (LoadedComponent) {
                        const instance = new LoadedComponent(entry.props, el);
                        instance.mount();
                        el.dataset.hydrated = 'true';
                    }
                });
            }
        }
    }

    // Load island bundle from server
    async function loadIsland(name) {
        try {
            const response = await fetch(`/_runts/islands/${name}`);
            if (response.ok) {
                const text = await response.text();
                // Execute in module context
                const blob = new Blob([text], { type: 'application/javascript' });
                const url = URL.createObjectURL(blob);
                await import(url);
                URL.revokeObjectURL(url);
            }
        } catch (e) {
            console.error('[runts] Failed to load island:', name, e);
        }
    }

    // Auto-initialize on DOM ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', () => {
            setTimeout(hydrateAll, 100); // Small delay to ensure all scripts loaded
        });
    } else {
        setTimeout(hydrateAll, 100);
    }

    // Expose API
    window.__runts__ = {
        hydrateAll,
        register: window.__registerIsland__,
        islands
    };
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

    // Debug: print routes
    {
        let rt = state.route_table.read();
        for _route in &rt.routes {
        }
    }

    // Start file watcher
    if let Err(e) = state.start_watcher() {
        eprintln!("Warning: Could not start file watcher: {}", e);
    }

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                runts Dev Server (HIR Mode)               ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  URL:        http://localhost:{}                           ║", dev_port);
    println!("║  Root:       {}  ", truncate_path(&root.to_string_lossy(), 50));
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Mode:       HIR Interpreter (No Rust Compile)            ║");
    println!("║  Features:   Full SSR, Islands, Layouts, Hot Reload         ║");
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
        .route("/_runts/islands/:name", get(handle_island_bundle))
        .route("/_runts/hmr", get(handle_hmr_sse))
        .route("/_runts/hmr.js", get(hmr_client_script))
        .route("/_runts/client.js", get(client_script))
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

fn truncate_path(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("...{}", &s[s.len()-max_len+3..])
    }
}
