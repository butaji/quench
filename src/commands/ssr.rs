//! Server-Side Rendering for runts dev server
//!
//! Handles:
//! - Component rendering to HTML
//! - Island placeholder injection
//! - HTML document assembly

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

use super::routes::{HttpMethod, Route, RouteTable};
use super::layouts::{LayoutContext, LayoutManager, Layout};
use crate::transpile::Parser;

/// Island manifest entry
#[derive(Debug, Clone, serde::Serialize)]
pub struct IslandManifestEntry {
    pub name: String,
    pub hash: String,
    pub props: Vec<String>,
    pub module_path: String,
}

/// Island manifest
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IslandManifest {
    pub islands: HashMap<String, IslandManifestEntry>,
}

impl IslandManifest {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn add_island(&mut self, name: String, props: Vec<String>) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish())[..8].to_string();
        
        let entry = IslandManifestEntry {
            name: name.clone(),
            hash: hash.clone(),
            props,
            module_path: format!("/_runts/islands/{}.{}.js", name, hash),
        };
        
        self.islands.insert(name, entry);
        hash
    }
    
    pub fn get_hash(&self, name: &str) -> Option<String> {
        self.islands.get(name).map(|e| e.hash.clone())
    }
}

/// SSR context for a single request
#[derive(Debug, Clone)]
pub struct SsrContext {
    pub params: HashMap<String, String>,
    pub route: String,
    pub layout_ctx: LayoutContext,
    pub page_data: Option<serde_json::Value>,
    pub island_manifest: IslandManifest,
    island_id_counter: usize,
}

impl SsrContext {
    pub fn new(route: &str, params: HashMap<String, String>) -> Self {
        Self {
            params,
            route: route.to_string(),
            layout_ctx: LayoutContext::new(),
            page_data: None,
            island_manifest: IslandManifest::new(),
            island_id_counter: 0,
        }
    }
    
    pub fn next_island_id(&mut self) -> String {
        self.island_id_counter += 1;
        format!("island-{:x}", self.island_id_counter)
    }
    
    pub fn set_page_data(&mut self, data: serde_json::Value) {
        self.page_data = Some(data);
    }
}

/// Reference to an island used in a route
#[derive(Debug, Clone)]
struct IslandRef {
    name: String,
    props: HashMap<String, serde_json::Value>,
}

/// SSR renderer
pub struct SsrRenderer {
    route_table: RouteTable,
    layout_manager: LayoutManager,
    routes_dir: PathBuf,
    islands_dir: PathBuf,
}

impl SsrRenderer {
    pub fn new(routes_dir: PathBuf, islands_dir: PathBuf) -> Result<Self> {
        let route_table = RouteTable::from_routes_dir(&routes_dir)?;
        let layout_manager = LayoutManager::from_routes_dir(&routes_dir)?;
        
        Ok(Self {
            route_table,
            layout_manager,
            routes_dir,
            islands_dir,
        })
    }
    
    /// Render a request
    pub async fn render(&self, path: &str, method: HttpMethod) -> Result<SsrResult> {
        let (route, params) = self.route_table
            .find_route(path, method)
            .ok_or_else(|| anyhow!("Route not found: {} {}", method_to_str(method), path))?;
        
        let mut ctx = SsrContext::new(&route.pattern, params);
        
        // Execute the route handler to get page data
        let page_data = self.execute_handler(route, &ctx).await?;
        ctx.set_page_data(page_data.clone());
        
        let layouts = self.layout_manager.find_layouts_for_path(path);
        
        // Render the page component
        let page_html = self.render_page_component(route, &ctx).await?;
        
        let final_html = self.compose_with_layouts(page_html, &layouts, &ctx).await?;
        
        let html = if let Some(_app_path) = self.layout_manager.get_app_wrapper() {
            self.wrap_with_app(final_html, &ctx).await?
        } else {
            self.wrap_in_document(final_html, path, &page_data)
        };
        
        Ok(SsrResult {
            html,
            island_manifest: ctx.island_manifest,
        })
    }
    
    /// Execute a route handler and return the data for rendering
    async fn execute_handler(&self, route: &Route, ctx: &SsrContext) -> Result<serde_json::Value> {
        // Check if there's a handler export in the route file
        let source = fs::read_to_string(&route.file_path)?;
        
        // Parse the route file
        let mut parser = Parser::new();
        let module = match parser.parse_source(&source) {
            Ok(m) => m,
            Err(e) => {
                // If parsing fails, return empty data
                eprintln!("[runts] Warning: Could not parse handler: {}", e);
                return Ok(serde_json::json!({}));
            }
        };
        
        // Check for handler export
        let mut has_handler = false;
        for item in &module.items {
            if let crate::transpile::hir::ModuleItem::Export(export) = item {
                match export {
                    crate::transpile::hir::Export::NamedWithValue { name, .. } => {
                        if name == "handler" {
                            has_handler = true;
                            break;
                        }
                    }
                    crate::transpile::hir::Export::Named { name } => {
                        if name == "handler" {
                            has_handler = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        
        if !has_handler {
            // No handler, return empty data
            return Ok(serde_json::json!({}));
        }
        
        // For dev mode, we simulate the handler response
        // In a full implementation, this would execute the handler in a sandbox
        // For now, return a mock response based on route params
        let mut data = serde_json::Map::new();
        
        // Add params to data
        for (key, value) in &ctx.params {
            data.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        
        // Add route info
        data.insert("route".to_string(), serde_json::Value::String(route.pattern.clone()));
        
        // If this is a blog route, add mock blog data
        if route.pattern.contains("blog") {
            if let Some(slug) = ctx.params.get("slug") {
                data.insert("title".to_string(), serde_json::Value::String(format!("Blog post: {}", slug)));
                data.insert("content".to_string(), serde_json::Value::String(format!("This is the content of blog post '{}'", slug)));
            } else {
                data.insert("posts".to_string(), serde_json::json!([
                    { "slug": "hello-world", "title": "Hello World", "excerpt": "Welcome to runts!" },
                    { "slug": "getting-started", "title": "Getting Started", "excerpt": "Learn how to build with runts" },
                ]));
            }
        }
        
        Ok(serde_json::Value::Object(data))
    }
    
    /// Render the page component to HTML
    async fn render_page_component(&self, route: &Route, ctx: &SsrContext) -> Result<String> {
        let component_name = path_to_component_name(&route.pattern);
        let page_data = ctx.page_data.as_ref();
        
        let mut html = String::new();
        
        // Get page data for rendering
        let page_props_json = if let Some(data) = page_data {
            serde_json::to_string(data).unwrap_or_default()
        } else {
            "{}".to_string()
        };
        
        // Check for islands in the route file
        let islands = self.find_islands_in_route(&route.file_path)?;
        
        // Generate component HTML based on route
        html.push_str(&format!(
            r#"<div class="runts-page" data-route="{}">"#,
            route.pattern
        ));
        
        // Add page props as JSON for client hydration
        html.push_str(&format!(
            r#"<script type="application/json" id="__page_props">{}</script>"#,
            page_props_json
        ));
        
        // Render based on route pattern
        html.push_str(&self.render_route_content(route, ctx).await?);
        
        // Render islands
        for island in &islands {
            let mut ctx_clone = ctx.clone();
            html.push_str(&self.render_island_placeholder(
                &island.name,
                &island.props,
                &mut ctx_clone
            ));
        }
        
        html.push_str("</div>");
        
        Ok(html)
    }
    
    /// Render the content of a route
    async fn render_route_content(&self, route: &Route, ctx: &SsrContext) -> Result<String> {
        let component_name = path_to_component_name(&route.pattern);
        
        // Generate mock HTML based on the route and data
        let mut content = String::new();
        
        if let Some(data) = ctx.page_data.as_ref() {
            if let Some(obj) = data.as_object() {
                // Render based on data keys
                if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
                    content.push_str(&format!("<h1>{}</h1>", html_escape(title)));
                }
                
                if let Some(content_text) = obj.get("content").and_then(|v| v.as_str()) {
                    content.push_str(&format!("<p>{}</p>", html_escape(content_text)));
                }
                
                if let Some(posts) = obj.get("posts").and_then(|v| v.as_array()) {
                    content.push_str("<ul class='posts'>");
                    for post in posts {
                        if let Some(post_obj) = post.as_object() {
                            let slug = post_obj.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                            let title = post_obj.get("title").and_then(|v| v.as_str()).unwrap_or("");
                            let excerpt = post_obj.get("excerpt").and_then(|v| v.as_str()).unwrap_or("");
                            content.push_str(&format!(
                                "<li><a href='/blog/{0}'><strong>{1}</strong><p>{2}</p></a></li>",
                                html_escape(slug),
                                html_escape(title),
                                html_escape(excerpt)
                            ));
                        }
                    }
                    content.push_str("</ul>");
                }
            }
        }
        
        // If no content, show a placeholder
        if content.is_empty() {
            content.push_str(&format!(
                "<!-- Component: {} would render here in production -->",
                component_name
            ));
        }
        
        Ok(content)
    }
    
    /// Find islands referenced in a route file
    fn find_islands_in_route(&self, route_path: &PathBuf) -> Result<Vec<IslandRef>> {
        let mut islands = Vec::new();
        
        if !route_path.exists() {
            return Ok(islands);
        }
        
        let content = fs::read_to_string(route_path)?;
        
        // Simple regex to find island imports and usages
        let import_re = regex::Regex::new(r#"import\s+.*?\s+from\s+["']\.\./islands/(\w+)\.tsx["']"#).unwrap();
        
        // Check imports for islands
        for cap in import_re.captures_iter(&content) {
            if let Some(name) = cap.get(1) {
                let island_path = self.islands_dir.join(format!("{}.tsx", name.as_str()));
                if island_path.exists() {
                    // Get props from the component usage
                    let usage_re = regex::Regex::new(&format!(r#"<{}\s+([^>]*?)/?>"#, name.as_str())).unwrap();
                    let mut props = HashMap::new();
                    
                    if let Some(usage_cap) = usage_re.captures(&content) {
                        if let Some(props_str) = usage_cap.get(1) {
                            // Parse simple props like initial={5}
                            let prop_re = regex::Regex::new(r#"(\w+)\s*=\s*\{([^}]+)\}"#).unwrap();
                            for prop_cap in prop_re.captures_iter(props_str.as_str()) {
                                if let (Some(key), Some(value)) = (prop_cap.get(1), prop_cap.get(2)) {
                                    props.insert(key.as_str().to_string(), serde_json::json!(value.as_str()));
                                }
                            }
                        }
                    }
                    
                    islands.push(IslandRef {
                        name: name.as_str().to_string(),
                        props,
                    });
                }
            }
        }
        
        Ok(islands)
    }
    
    async fn compose_with_layouts(
        &self,
        page_html: String,
        layouts: &[Layout],
        _ctx: &SsrContext,
    ) -> Result<String> {
        let mut result = page_html;
        
        for layout in layouts {
            let layout_html = format!(
                r#"<div class="runts-layout" data-layout="{}">{}</div>"#,
                layout.pattern, result
            );
            result = layout_html;
        }
        
        Ok(result)
    }
    
    async fn wrap_with_app(&self, content: String, ctx: &SsrContext) -> Result<String> {
        let mut html = String::new();
        
        html.push_str(r#"<div id="app">"#);
        html.push_str(&content);
        html.push_str("</div>");
        
        if !ctx.layout_ctx.state.is_empty() {
            html.push_str(&format!(
                r#"<script>window.__RUNTS_STATE__ = {};</script>"#,
                serde_json::to_string(&ctx.layout_ctx.state).unwrap_or_default()
            ));
        }
        
        Ok(html)
    }
    
    fn wrap_in_document(&self, content: String, path: &str, page_data: &serde_json::Value) -> String {
        let title = path_to_title(path);
        
        // Serialize page data for client hydration
        let page_data_json = serde_json::to_string(page_data).unwrap_or_else(|_| "{}".to_string());
        
        let mut html = String::new();
        html.push_str("<!DOCTYPE html>\n");
        html.push_str("<html lang=\"en\">\n");
        html.push_str("<head>\n");
        html.push_str("    <meta charset=\"UTF-8\">\n");
        html.push_str("    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
        html.push_str(&format!("    <title>{}</title>\n", title));
        html.push_str("    <link rel=\"icon\" href=\"/static/favicon.ico\">\n");
        html.push_str("    <script type=\"module\" src=\"/_runts/hmr.js\"></script>\n");
        html.push_str("</head>\n");
        html.push_str("<body>\n");
        html.push_str("    <nav class=\"runts-nav\">\n");
        html.push_str("        <a href=\"/\">Home</a>\n");
        html.push_str("        <a href=\"/blog\">Blog</a>\n");
        html.push_str("    </nav>\n");
        html.push_str(&content);
        html.push_str("\n");
        html.push_str("    <script>\n");
        html.push_str("        // Page data for hydration\n");
        html.push_str(&format!("        window.__PAGE_DATA__ = {};\n", page_data_json));
        html.push_str("        // Island manifest\n");
        html.push_str("        window.__ISLAND_MANIFEST__ = {};\n");
        html.push_str("    </script>\n");
        html.push_str("</body>\n");
        html.push_str("</html>\n");
        html
    }
    
    /// Render an island placeholder
    pub fn render_island_placeholder(
        &self,
        name: &str,
        props: &HashMap<String, serde_json::Value>,
        ctx: &mut SsrContext,
    ) -> String {
        let id = ctx.next_island_id();
        let hash = ctx.island_manifest.add_island(
            name.to_string(),
            props.keys().map(|s| s.clone()).collect()
        );
        let props_json = serde_json::to_string(props).unwrap_or_default();
        
        // For SSR, we render a placeholder that will be hydrated by the client
        format!(
            r#"<div data-island="{}" data-id="{}" data-hash="{}" data-props='{}'>
    <span class="runts-island-loading">Loading...</span>
</div>"#,
            name, id, hash, props_json
        )
    }
}

fn method_to_str(method: HttpMethod) -> &'static str {
    match method {
        HttpMethod::GET => "GET",
        HttpMethod::POST => "POST",
        HttpMethod::PUT => "PUT",
        HttpMethod::DELETE => "DELETE",
        HttpMethod::PATCH => "PATCH",
        HttpMethod::HEAD => "HEAD",
        HttpMethod::OPTIONS => "OPTIONS",
    }
}

fn path_to_component_name(path: &str) -> String {
    if path.is_empty() {
        return "Index".to_string();
    }
    
    // Get the last segment (the actual route file/component)
    let last_segment = path.split('/')
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("Index");
    
    // Convert to PascalCase
    last_segment.split(|c: char| c == '-' || c == '_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
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

/// SSR result
#[derive(Debug)]
pub struct SsrResult {
    pub html: String,
    pub island_manifest: IslandManifest,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_component_name() {
        assert_eq!(path_to_component_name(""), "Index");
        assert_eq!(path_to_component_name("/blog"), "Blog");
        assert_eq!(path_to_component_name("/blog/my-post"), "MyPost");
    }
    
    #[test]
    fn test_page_title() {
        assert_eq!(path_to_title("/"), "Home");
        assert_eq!(path_to_title("/blog"), "Blog");
    }
    
    #[test]
    fn test_island_manifest() {
        let mut manifest = IslandManifest::new();
        
        let hash1 = manifest.add_island("Counter".to_string(), vec!["initial".to_string()]);
        let hash2 = manifest.add_island("Counter".to_string(), vec!["initial".to_string()]);
        
        assert_eq!(hash1, hash2);
        
        let hash3 = manifest.add_island("TodoList".to_string(), vec!["items".to_string()]);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<div>"), "&lt;div&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }
}
