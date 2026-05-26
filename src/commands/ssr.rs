//! Server-Side Rendering for runts dev server
//!
//! Handles:
//! - Component rendering to HTML
//! - Island placeholder injection
//! - HTML document assembly

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;

use super::routes::{HttpMethod, Route, RouteTable};
use super::layouts::{LayoutContext, LayoutManager, Layout};

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

/// SSR renderer
pub struct SsrRenderer {
    route_table: RouteTable,
    layout_manager: LayoutManager,
}

impl SsrRenderer {
    pub fn new(routes_dir: PathBuf, islands_dir: PathBuf) -> Result<Self> {
        let route_table = RouteTable::from_routes_dir(&routes_dir)?;
        let layout_manager = LayoutManager::from_routes_dir(&routes_dir)?;
        
        Ok(Self {
            route_table,
            layout_manager,
        })
    }
    
    /// Render a request
    pub async fn render(&self, path: &str, method: HttpMethod) -> Result<SsrResult> {
        let (route, params) = self.route_table
            .find_route(path, method)
            .ok_or_else(|| anyhow!("Route not found: {} {}", method_to_str(method), path))?;
        
        let mut ctx = SsrContext::new(&route.pattern, params);
        
        let page_data = self.execute_handler(route).await?;
        ctx.set_page_data(page_data);
        
        let layouts = self.layout_manager.find_layouts_for_path(path);
        
        let page_html = self.render_page_component(route, &ctx).await?;
        
        let final_html = self.compose_with_layouts(page_html, &layouts, &ctx).await?;
        
        let html = if let Some(_app_path) = self.layout_manager.get_app_wrapper() {
            self.wrap_with_app(final_html, &ctx).await?
        } else {
            self.wrap_in_document(final_html, path)
        };
        
        Ok(SsrResult {
            html,
            island_manifest: ctx.island_manifest,
        })
    }
    
    async fn execute_handler(&self, route: &Route) -> Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
    
    async fn render_page_component(&self, route: &Route, ctx: &SsrContext) -> Result<String> {
        let component_name = path_to_component_name(&route.pattern);
        let page_data = ctx.page_data.as_ref();
        
        let mut html = format!(
            r#"<div class="runts-page" data-route="{}">"#,
            route.pattern
        );
        
        if let Some(data) = page_data {
            html.push_str(&format!(
                r#"<script type="application/json" data-page-props>{}</script>"#,
                serde_json::to_string(data).unwrap_or_default()
            ));
        }
        
        html.push_str(&format!(
            r#"<div class="runts-component" data-component="{}">{}</div>"#,
            component_name,
            format!("<!-- Component: {} -->", component_name)
        ));
        
        html.push_str("</div>");
        
        Ok(html)
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
    
    fn wrap_in_document(&self, content: String, path: &str) -> String {
        let title = path_to_title(path);
        
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <link rel="icon" href="/static/favicon.ico">
</head>
<body>
    {}
</body>
</html>"#,
            title,
            content
        )
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
        
        format!(
            r#"<div data-island="{}" data-id="{}" data-hash="{}" data-props='{}'></div>"#,
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
}
