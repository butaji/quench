//! Production SSR Engine & Server Utilities
//!
//! This module provides the server-side rendering pipeline for production builds.
//! It is NOT the dev server — see `commands::dev` for that.
//!
//! Responsibilities:
//! - Synchronous VNode → HTML string rendering
//! - Island hydration manifest generation
//! - Static file serving helpers
//! - Production middleware composition

use serde::Serialize;

use crate::runtime::vdom::{VNode, Render};
use crate::runtime::islands::{HydrationStrategy, IslandManifest, IslandManifestEntry};

/// Rendered page result
#[derive(Debug, Clone)]
pub struct PageResult {
    /// Full HTML document
    pub html: String,
    /// HTTP status code
    pub status: u16,
    /// Page-specific data (serialized to window.__PAGE_DATA__)
    pub page_data: serde_json::Value,
    /// Island instances for hydration
    pub islands: Vec<IslandInstance>,
}

/// Island instance embedded in a page
#[derive(Debug, Clone)]
pub struct IslandInstance {
    pub name: String,
    pub id: String,
    pub props_json: String,
    pub strategy: HydrationStrategy,
    pub html: String,
}

/// Production SSR engine
pub struct SsrEngine {
    /// Base HTML template (head, nav, scripts)
    template: DocumentTemplate,
}

/// Document template configuration
#[derive(Debug, Clone)]
pub struct DocumentTemplate {
    pub title_prefix: String,
    pub nav_html: String,
    pub head_extra: String,
    pub script_modules: Vec<String>,
    pub stylesheet_links: Vec<String>,
}

impl Default for DocumentTemplate {
    fn default() -> Self {
        Self {
            title_prefix: String::new(),
            nav_html: r#"<nav class="runts-nav">
    <a href="/">Home</a>
    <a href="/blog">Blog</a>
</nav>"#.to_string(),
            head_extra: String::new(),
            script_modules: vec![
                "/_runts/client.js".to_string(),
            ],
            stylesheet_links: vec![],
        }
    }
}

impl SsrEngine {
    /// Create a new SSR engine with default template
    pub fn new() -> Self {
        Self {
            template: DocumentTemplate::default(),
        }
    }

    /// Create with custom template
    pub fn with_template(template: DocumentTemplate) -> Self {
        Self { template }
    }

    /// Render a complete page from a VNode tree
    pub fn render_page(
        &self,
        title: &str,
        content: VNode,
        page_data: impl Serialize,
        islands: Vec<IslandInstance>,
    ) -> PageResult {
        let content_html = content.render_to_html();
        let page_data_json = serde_json::to_value(&page_data).unwrap_or(serde_json::Value::Null);

        let island_manifest = IslandManifest {
            islands: islands.iter().map(|i| IslandManifestEntry {
                name: i.name.clone(),
                selector: format!("[data-island=\"{}\"][data-id=\"{}\"]", i.name, i.id),
                props: i.props_json.clone(),
                strategy: i.strategy,
            }).collect(),
        };

        let island_manifest_json = serde_json::to_string(&island_manifest).unwrap_or_default();

        let hydration_scripts: Vec<String> = islands.iter().map(|i| {
            format!(
                r#"<script type="module">
import {{ hydrate }} from '/_runts/islands/{}.js';
hydrate('{}', {});
</script>"#,
                i.name, i.id, i.props_json
            )
        }).collect();

        let stylesheets = self.template.stylesheet_links.iter()
            .map(|href| format!(r#"<link rel="stylesheet" href="{}" />"#, href))
            .collect::<Vec<_>>()
            .join("\n");

        let scripts = self.template.script_modules.iter()
            .map(|src| format!(r#"<script type="module" src="{}"></script>"#, src))
            .collect::<Vec<_>>()
            .join("\n");

        let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}{}</title>
    <link rel="icon" href="/static/favicon.ico">
    {}
    {}
</head>
<body>
    {}
    <main>
        {}
    </main>
    <script>
        window.__PAGE_DATA__ = {};
        window.__ISLAND_MANIFEST__ = {};
    </script>
    {}
    {}
</body>
</html>"#,
            self.template.title_prefix,
            title,
            stylesheets,
            self.template.head_extra,
            self.template.nav_html,
            content_html,
            serde_json::to_string(&page_data_json).unwrap_or_default(),
            island_manifest_json,
            hydration_scripts.join("\n"),
            scripts,
        );

        PageResult {
            html,
            status: 200,
            page_data: page_data_json,
            islands,
        }
    }

    /// Render an error page
    pub fn render_error(
        &self,
        status: u16,
        path: &str,
        message: Option<&str>,
    ) -> PageResult {
        let title = match status {
            404 => "Page Not Found",
            500 => "Internal Server Error",
            _ => "Error",
        };

        let default_msg = match status {
            404 => format!("The page '{}' could not be found.", path),
            500 => "An unexpected error occurred.".to_string(),
            _ => format!("Error {} occurred.", status),
        };

        let message = message.unwrap_or(&default_msg);

        let content = format!(r#"
<div style="text-align:center;padding:4rem;font-family:system-ui,sans-serif">
    <h1 style="font-size:6rem;color:#333;margin:0">{}</h1>
    <h2 style="font-size:2rem;color:#666;margin:1rem 0">{}</h2>
    <p style="color:#888">{}</p>
    <p><a href="/" style="color:#1a1a2e">← Go home</a></p>
</div>"#, status, title, message);

        PageResult {
            html: format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>{} - {}</title>
</head>
<body>
    {}
</body>
</html>"#, status, title, content),
            status,
            page_data: serde_json::Value::Null,
            islands: vec![],
        }
    }
}

impl Default for SsrEngine {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Static File Serving
// =============================================================================

use std::path::{Path, PathBuf};
use tokio::fs;

/// Serve a static file with appropriate MIME type.
/// Returns (status, content_type, body) or None if not found.
pub async fn serve_static_file(
    root: &Path,
    request_path: &str,
) -> Option<(u16, String, Vec<u8>)> {
    let file_path = sanitize_path(root, request_path)?;

    if !file_path.exists() || !file_path.is_file() {
        return None;
    }

    // Ensure the resolved path is still within root
    let canonical_root = std::fs::canonicalize(root).ok()?;
    let canonical_file = std::fs::canonicalize(&file_path).ok()?;
    if !canonical_file.starts_with(&canonical_root) {
        return None; // Path traversal attempt
    }

    let contents = fs::read(&file_path).await.ok()?;

    let mime = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    Some((200, mime, contents))
}

/// Sanitize a request path into a safe file system path.
fn sanitize_path(root: &Path, request_path: &str) -> Option<PathBuf> {
    let trimmed = request_path.trim_start_matches('/');
    if trimmed.contains("..") {
        return None;
    }
    Some(root.join(trimmed))
}

// =============================================================================
// Middleware Composition (Tower)
// =============================================================================

use tower::Layer;

/// Compose multiple Tower layers into a single middleware stack.
pub fn compose_middleware<S>(
    service: S,
    _layers: Vec<Box<dyn Layer<S, Service = S> + Send + Sync>>,
) -> S {
    // In a full implementation, this would fold layers onto the service.
    // For now, return the service directly — Axum's Router handles layering.
    service
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::vdom::VNode;

    #[test]
    fn test_ssr_basic_page() {
        let engine = SsrEngine::new();
        let content = VNode::text("Hello, world!");
        let result = engine.render_page("Test", content, serde_json::json!({}), vec![]);

        assert!(result.html.contains("<!DOCTYPE html>"));
        assert!(result.html.contains("<title>Test</title>"));
        assert!(result.html.contains("Hello, world!"));
        assert_eq!(result.status, 200);
    }

    #[test]
    fn test_ssr_error_page() {
        let engine = SsrEngine::new();
        let result = engine.render_error(404, "/missing", None);

        assert!(result.html.contains("404"));
        assert!(result.html.contains("Page Not Found"));
        assert_eq!(result.status, 404);
    }

    #[test]
    fn test_sanitize_path() {
        let root = Path::new("/var/www");
        assert_eq!(
            sanitize_path(root, "style.css"),
            Some(PathBuf::from("/var/www/style.css"))
        );
        assert_eq!(
            sanitize_path(root, "../../etc/passwd"),
            None
        );
    }
}
