//! Server-side rendering and HTTP handling
//!
//! This module provides utilities for building Fresh-style web applications
//! with Axum as the HTTP framework.

use std::collections::HashMap;
use std::sync::Arc;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    /// User-defined state
    pub user_state: Arc<HashMap<String, serde_json::Value>>,
    /// Routes manifest
    pub routes: Arc<Vec<RouteManifest>>,
    /// Islands manifest
    pub islands: Arc<Vec<IslandManifest>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            user_state: Arc::new(HashMap::new()),
            routes: Arc::new(Vec::new()),
            islands: Arc::new(Vec::new()),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_user_state(mut self, state: HashMap<String, serde_json::Value>) -> Self {
        self.user_state = Arc::new(state);
        self
    }

    pub fn with_routes(mut self, routes: Vec<RouteManifest>) -> Self {
        self.routes = Arc::new(routes);
        self
    }

    pub fn with_islands(mut self, islands: Vec<IslandManifest>) -> Self {
        self.islands = Arc::new(islands);
        self
    }
}

/// Route manifest entry
#[derive(Clone, Debug)]
pub struct RouteManifest {
    /// URL pattern (e.g., "/blog/:slug")
    pub pattern: String,
    /// File path in the routes directory
    pub file: String,
}

/// Island manifest entry
#[derive(Clone, Debug)]
pub struct IslandManifest {
    /// Component name
    pub name: String,
    /// File path
    pub file: String,
    /// Props type
    pub props_type: Option<String>,
}

/// Response wrapper
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Response {
    pub fn new(html: impl Into<String>) -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
            body: html.into(),
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn html(body: impl Into<String>) -> Self {
        Self::new(body).with_header("Content-Type", "text/html")
    }

    pub fn json<T: serde::Serialize>(body: &T) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_string(body)?;
        Ok(Self::new(json).with_header("Content-Type", "application/json"))
    }

    pub fn not_found() -> Self {
        Self::html(r#"<!DOCTYPE html>
<html>
<head><title>404</title></head>
<body>
    <h1>404 - Not Found</h1>
    <a href="/">Home</a>
</body>
</html>"#)
        .with_status(404)
    }
}

/// Response builder
pub struct ResponseBuilder {
    status: u16,
    headers: HashMap<String, String>,
}

impl ResponseBuilder {
    pub fn new() -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
        }
    }

    pub fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn build(self, body: impl Into<String>) -> Response {
        Response {
            status: self.status,
            headers: self.headers,
            body: body.into(),
        }
    }
}

impl Default for ResponseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Island Hydration
// =============================================================================

/// Island hydration script
pub fn island_hydration_script(name: &str, props: &serde_json::Value, id: &str) -> String {
    format!(
        r#"<script type="application/x-runts-island" id="{}" data-component="{}">{}</script>"#,
        id,
        name,
        serde_json::to_string(props).unwrap_or_default()
    )
}

/// Island container HTML
pub fn island_container(name: &str, id: &str, props: &serde_json::Value, server_rendered: &str) -> String {
    format!(
        r#"<div data-island="{}" data-props="{}" data-url="/_islands/{}">{}</div>"#,
        name,
        serde_json::to_string(props).unwrap_or_default(),
        name,
        server_rendered
    )
}

// =============================================================================
// Page Rendering
// =============================================================================

/// Render full HTML page
pub fn render_page(
    title: &str,
    content: super::vdom::VNode,
    _app_wrapper: Option<super::vdom::VNode>,
) -> String {
    let page_html = content.to_html();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
</head>
<body>
    <div id="app">{}</div>
</body>
</html>"#,
        title,
        page_html
    )
}
