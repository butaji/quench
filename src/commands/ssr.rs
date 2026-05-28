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
use serde_json;

use super::routes::{HttpMethod, Route, RouteTable};
use super::layouts::{LayoutContext, LayoutManager, Layout};
use crate::transpile::TsParser;

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
        
        let page_data = self.execute_handler(route, &ctx).await?;
        ctx.set_page_data(page_data.clone());
        
        let layouts = self.layout_manager.find_layouts_for_path(path);
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
        let source = fs::read_to_string(&route.file_path)?;
        let module = match TsParser::new().parse_source(&source) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[runts] Warning: Could not parse handler: {}", e);
                return Ok(serde_json::json!({}));
            }
        };
        
        if !has_handler_export(&module) {
            return Ok(serde_json::json!({}));
        }
        
        Ok(build_mock_page_data(route, ctx))
    }
    
    /// Render the page component to HTML
    async fn render_page_component(&self, route: &Route, ctx: &SsrContext) -> Result<String> {
        let page_props_json = ctx.page_data
            .as_ref()
            .and_then(|d| serde_json::to_string(d).ok())
            .unwrap_or_else(|| "{}".to_string());

        let islands = self.find_islands_in_route(&route.file_path)?;
        let content = self.render_route_content(route, ctx).await?;
        let island_html = self.render_islands(&islands, ctx);

        Ok(format!(
            r#"<div class="runts-page" data-route="{}">\n{}\n<script type="application/json" id="__page_props">{}</script>\n{}\n</div>"#,
            route.pattern,
            content,
            page_props_json,
            island_html
        ))
    }

    fn render_islands(&self, islands: &[IslandRef], ctx: &SsrContext) -> String {
        islands.iter()
            .map(|island| {
                let mut ctx_clone = ctx.clone();
                self.render_island_placeholder(&island.name, &island.props, &mut ctx_clone)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    
    /// Render the content of a route
    async fn render_route_content(&self, route: &Route, ctx: &SsrContext) -> Result<String> {
        let component_name = path_to_component_name(&route.pattern);
        let mut content = String::new();

        if let Some(obj) = ctx.page_data.as_ref().and_then(|d| d.as_object()) {
            if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
                content.push_str(&format!("<h1>{}</h1>\n", html_escape(title)));
            }
            if let Some(text) = obj.get("content").and_then(|v| v.as_str()) {
                content.push_str(&format!("<p>{}</p>\n", html_escape(text)));
            }
            if let Some(posts) = obj.get("posts").and_then(|v| v.as_array()) {
                content.push_str(&render_posts_list(posts));
            }
        }

        if content.is_empty() {
            content.push_str(&format!(
                "<!-- Component: {} would render here in production -->\n",
                component_name
            ));
        }

        Ok(content)
    }
    
    /// Find islands referenced in a route file
    fn find_islands_in_route(&self, route_path: &PathBuf) -> Result<Vec<IslandRef>> {
        if !route_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(route_path)?;
        let import_re = regex::Regex::new(r#"import\s+.*?\s+from\s+["']\.\./islands/(\w+)\.tsx["']"#)?;

        import_re.captures_iter(&content)
            .filter_map(|cap| {
                let name = cap.get(1)?.as_str();
                self.build_island_ref(name, &content)
            })
            .collect::<Result<Vec<_>>>()
    }

    fn build_island_ref(&self, name: &str, content: &str) -> Option<Result<IslandRef>> {
        let island_path = self.islands_dir.join(format!("{}.tsx", name));
        if !island_path.exists() {
            return None;
        }

        let props = self.extract_island_props(name, content);
        Some(Ok(IslandRef {
            name: name.to_string(),
            props,
        }))
    }

    fn extract_island_props(&self, name: &str, content: &str) -> HashMap<String, serde_json::Value> {
        let usage_re = regex::Regex::new(&format!(r#"<{}\s+([^>]*?)/?>"#, name)).ok()
            .and_then(|re| re.captures(content))
            .and_then(|cap| cap.get(1));

        let mut props = HashMap::new();
        if let Some(props_str) = usage_re {
            let prop_re = regex::Regex::new(r#"(\w+)\s*=\s*\{([^}]+)\}"#).unwrap();
            for prop_cap in prop_re.captures_iter(props_str.as_str()) {
                if let (Some(key), Some(value)) = (prop_cap.get(1), prop_cap.get(2)) {
                    props.insert(key.as_str().to_string(), serde_json::json!(value.as_str()));
                }
            }
        }
        props
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
        let page_data_json = serde_json::to_string(page_data).unwrap_or_else(|_| "{}".to_string());
        let nav_links = self.build_nav_links();
        
        format!(
            r#"<!DOCTYPE html>
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
{}
    </nav>
    {}
    <script>
        window.__PAGE_DATA__ = {};
        window.__ISLAND_MANIFEST__ = {{}};
    </script>
</body>
</html>
"#,
            title,
            nav_links,
            content,
            page_data_json
        )
    }
    
    /// Generate nav links from the current route table
    fn build_nav_links(&self) -> String {
        let mut links = vec![("/".to_string(), "Home".to_string())];
        
        for route in self.route_table.all_routes() {
            let pattern = &route.pattern;
            if pattern.contains(':') || pattern.contains('*') || pattern == "/" {
                continue;
            }
            
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
            
            let display = label.chars().enumerate()
                .map(|(i, c)| if i == 0 { c.to_uppercase().to_string() } else { c.to_string() })
                .collect::<String>();
            
            links.push((pattern.clone(), display));
        }
        
        links.sort_by(|a, b| a.0.cmp(&b.0));
        links.dedup_by(|a, b| a.0 == b.0);
        
        links.into_iter()
            .map(|(href, label)| format!("        <a href=\"{}\">{}</a>", href, label))
            .collect::<Vec<_>>()
            .join("\n")
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
            props.keys().cloned().collect()
        );
        let props_json = serde_json::to_string(props).unwrap_or_default();
        
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
    
    let last_segment = path.split('/')
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("Index");
    
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

/// Check if a module exports a handler
fn has_handler_export(module: &crate::transpile::hir::Module) -> bool {
    module.items.iter().any(|item| {
        if let crate::transpile::hir::ModuleItem::Export(export) = item {
            matches!(export,
                crate::transpile::hir::Export::NamedWithValue { name, .. }
                    | crate::transpile::hir::Export::Named { name }
                    if name == "handler"
            )
        } else {
            false
        }
    })
}

/// Build mock page data for a route (dev mode simulation)
fn build_mock_page_data(route: &Route, ctx: &SsrContext) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    for (key, value) in &ctx.params {
        data.insert(key.clone(), serde_json::Value::String(value.clone()));
    }
    data.insert("route".to_string(), serde_json::Value::String(route.pattern.clone()));
    
    if route.pattern.contains("blog") {
        if let Some(slug) = ctx.params.get("slug") {
            data.insert("title".to_string(), serde_json::Value::String(format!("Blog post: {}", slug)));
            data.insert("content".to_string(), serde_json::Value::String(format!("Content of '{}'", slug)));
        } else {
            data.insert("posts".to_string(), serde_json::json!([
                { "slug": "hello-world", "title": "Hello World", "excerpt": "Welcome!" },
            ]));
        }
    }
    
    serde_json::Value::Object(data)
}

/// Render a list of blog posts
fn render_posts_list<'a, I>(posts: I) -> String
where
    I: IntoIterator<Item = &'a serde_json::Value>,
{
    let items: Vec<String> = posts.into_iter()
        .filter_map(|post| {
            let obj = post.as_object()?;
            let slug = obj.get("slug")?.as_str()?;
            let title = obj.get("title")?.as_str()?;
            let excerpt = obj.get("excerpt")?.as_str()?;
            Some(format!(
                "<li><a href='/blog/{0}'><strong>{1}</strong><p>{2}</p></a></li>",
                html_escape(slug),
                html_escape(title),
                html_escape(excerpt)
            ))
        })
        .collect();
    format!("<ul class='posts'>\n{}\n</ul>", items.join("\n"))
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
