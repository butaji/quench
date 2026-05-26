//! SSR Server utilities
//!
//! Provides utilities for server-side rendering:
//! - HTML rendering
//! - Asset management
//! - Caching utilities

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Asset manifest entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    /// File path
    pub file: String,
    /// MIME type
    pub mime: String,
    /// Content hash for cache busting
    pub hash: Option<String>,
    /// Size in bytes
    pub size: Option<usize>,
}

/// Asset manifest
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetManifest {
    #[serde(rename = "entrypoints")]
    pub entrypoints: HashMap<String, Vec<String>>,
    
    #[serde(rename = "routes")]
    pub routes: HashMap<String, RouteAssets>,
}

/// Assets for a specific route
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteAssets {
    /// CSS files
    pub css: Vec<String>,
    /// JS files
    pub js: Vec<String>,
}

/// SSR Render options
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Whether to include dev toolbar
    pub dev_mode: bool,
    /// Base URL for assets
    pub base_url: String,
    /// Additional head elements
    pub head: Vec<HeadElement>,
}

/// Head element types
#[derive(Debug, Clone)]
pub enum HeadElement {
    /// <title> element
    Title(String),
    /// <meta> element
    Meta { name: String, content: String },
    /// <link> element
    Link { rel: String, href: String },
    /// <script> element
    Script { src: Option<String>, content: Option<String> },
    /// <style> element
    Style(String),
    /// Raw HTML
    Raw(String),
}

impl HeadElement {
    pub fn to_html(&self) -> String {
        match self {
            HeadElement::Title(title) => format!("<title>{}</title>", html_escape(title)),
            HeadElement::Meta { name, content } => {
                format!(r#"<meta name="{}" content="{}">"#, 
                    html_escape(name), html_escape(content))
            }
            HeadElement::Link { rel, href } => {
                format!(r#"<link rel="{}" href="{}">"#, 
                    html_escape(rel), html_escape(href))
            }
            HeadElement::Script { src, content } => {
                if let Some(src) = src {
                    format!(r#"<script src="{}"></script>"#, html_escape(src))
                } else if let Some(content) = content {
                    format!("<script>{}</script>", content)
                } else {
                    String::new()
                }
            }
            HeadElement::Style(css) => {
                format!("<style>{}</style>", css)
            }
            HeadElement::Raw(html) => html.clone(),
        }
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            dev_mode: false,
            base_url: String::new(),
            head: Vec::new(),
        }
    }
}

/// HTML renderer for SSR
pub struct HtmlRenderer {
    options: RenderOptions,
}

impl HtmlRenderer {
    pub fn new(options: RenderOptions) -> Self {
        Self { options }
    }

    /// Render a complete HTML document
    pub fn render_document(&self, title: &str, body: &str, manifest: Option<&AssetManifest>) -> String {
        let head_html = self.render_head(manifest);
        
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    {head_html}
</head>
<body>
{body}
</body>
</html>"#,
            title = html_escape(title),
            head_html = head_html,
            body = body
        )
    }

    /// Render the <head> section
    fn render_head(&self, manifest: Option<&AssetManifest>) -> String {
        let mut parts = Vec::new();

        // Add custom head elements
        for element in &self.options.head {
            parts.push(element.to_html());
        }

        // Add asset links
        if let Some(manifest) = manifest {
            for (name, files) in &manifest.entrypoints {
                match name.as_str() {
                    "main" => {
                        for file in files {
                            if file.ends_with(".css") {
                                parts.push(format!(
                                    r#"<link rel="stylesheet" href="{}{}">"#,
                                    self.options.base_url,
                                    file
                                ));
                            }
                        }
                        for file in files {
                            if file.ends_with(".js") {
                                parts.push(format!(
                                    r#"<script type="module" src="{}{}"></script>"#,
                                    self.options.base_url,
                                    file
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Add favicon
        parts.push(r#"<link rel="icon" href="/favicon.ico">"#.to_string());

        // Add dev mode toolbar
        if self.options.dev_mode {
            parts.push(self.render_dev_toolbar());
        }

        parts.join("\n    ")
    }

    /// Render the dev mode toolbar
    fn render_dev_toolbar(&self) -> String {
        r#"<script>
window.__RUNTS_DEV__ = true;
</script>
<div id="__runts-toolbar" style="position:fixed;bottom:0;left:0;right:0;background:#1a1a1a;color:#fff;padding:8px 16px;font-family:monospace;font-size:12px;z-index:9999;display:flex;justify-content:space-between;">
    <span>runts dev</span>
    <span id="__runts-status">Ready</span>
</div>"#.to_string()
    }
}

/// Utility function for HTML escaping
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Utility function for HTML attribute escaping
pub fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Generate inline script for page data
pub fn render_page_data_script(data: &serde_json::Value) -> String {
    let json = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"<script id="__page-data" type="application/json">{}</script>"#,
        html_escape(&json)
    )
}

/// Generate inline script for island manifest
pub fn render_island_manifest_script(manifest: &crate::runtime::islands::IslandManifest) -> String {
    let json = serde_json::to_string(manifest).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"<script id="__island-manifest" type="application/json">{}</script>"#,
        html_escape(&json)
    )
}

/// Cache utilities
pub mod cache {
    use std::time::{Duration, Instant};
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Simple in-memory cache
    pub struct Cache<K, V> {
        entries: RwLock<HashMap<K, CacheEntry<V>>>,
        ttl: Duration,
    }

    struct CacheEntry<V> {
        value: V,
        expires: Instant,
    }

    impl<K, V> Cache<K, V>
    where
        K: std::hash::Hash + Eq + Clone,
        V: Clone,
    {
        pub fn new(ttl: Duration) -> Self {
            Self {
                entries: RwLock::new(HashMap::new()),
                ttl,
            }
        }

        pub fn get(&self, key: &K) -> Option<V> {
            let mut entries = self.entries.write().ok()?;
            if let Some(entry) = entries.get_mut(key) {
                if Instant::now() < entry.expires {
                    return Some(entry.value.clone());
                } else {
                    entries.remove(key);
                }
            }
            None
        }

        pub fn set(&self, key: K, value: V) {
            let mut entries = match self.entries.write() {
                Ok(e) => e,
                Err(_) => return,
            };
            entries.insert(key, CacheEntry {
                value,
                expires: Instant::now() + self.ttl,
            });
        }

        pub fn invalidate(&self, key: &K) {
            if let Ok(mut entries) = self.entries.write() {
                entries.remove(key);
            }
        }

        pub fn clear(&self) {
            if let Ok(mut entries) = self.entries.write() {
                entries.clear();
            }
        }
    }

    /// Create a cache with common TTL values
    pub fn short_lived() -> Duration {
        Duration::from_secs(5)
    }

    pub fn medium_lived() -> Duration {
        Duration::from_secs(60)
    }

    pub fn long_lived() -> Duration {
        Duration::from_secs(3600)
    }
}

/// Response caching utilities
pub mod response_cache {
    use http::{HeaderMap, HeaderName, HeaderValue};
    use std::collections::HashMap;
    use std::sync::RwLock;

    static CACHE: RwLock<Option<HashMap<String, CachedResponse>>> = RwLock::new(None);

    /// Cached HTTP response
    #[derive(Clone)]
    pub struct CachedResponse {
        pub status: u16,
        pub headers: HashMap<String, String>,
        pub body: Vec<u8>,
    }

    /// Generate a cache key from request
    pub fn cache_key(method: &str, path: &str, query: Option<&str>) -> String {
        match query {
            Some(q) => format!("{}:{}?{}", method, path, q),
            None => format!("{}:{}", method, path),
        }
    }

    /// Check if response should be cached
    pub fn is_cacheable(status: u16, headers: &HeaderMap<HeaderValue>) -> bool {
        // Only cache successful GET responses
        if status != 200 {
            return false;
        }

        // Don't cache if Cache-Control: no-store
        if let Some(cc) = headers.get("Cache-Control") {
            if let Ok(cc_str) = cc.to_str() {
                if cc_str.contains("no-store") || cc_str.contains("no-cache") {
                    return false;
                }
            }
        }

        true
    }

    /// Add cache headers to response
    pub fn add_cache_headers(headers: &mut HeaderMap<HeaderValue>, max_age_secs: u64) {
        headers.insert(
            HeaderName::from_static("cache-control"),
            HeaderValue::try_from(format!("public, max-age={}", max_age_secs)).unwrap_or_else(|_| {
                HeaderValue::from_static("public, max-age=3600")
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("Hello & World"), "Hello &amp; World");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_render_document() {
        let renderer = HtmlRenderer::new(RenderOptions::default());
        let html = renderer.render_document("Test", "<p>Hello</p>", None);
        
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>Test</title>"));
        assert!(html.contains("<p>Hello</p>"));
    }

    #[test]
    fn test_cache_key() {
        assert_eq!(
            cache_key("GET", "/api/users", None),
            "GET:/api/users"
        );
        assert_eq!(
            cache_key("GET", "/api/users", Some("page=1")),
            "GET:/api/users?page=1"
        );
    }
}
