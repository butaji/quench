const COMPONENT_TMPL: &str = r#"//! React component: {name}
//! Source: {file_path}
//!
//! v0.1: Static HTML rendering via QuickJS
//! v0.2: Full JSX → Rust compilation

pub struct {name};

impl {name} {
    /// Render component to HTML string
    pub fn render() -> String {
        format!(
            "<div data-component=\"{name}\" data-source=\"{file_path}\">{{{name}}}</div>",
            name = name,
            file_path = file_path
        )
    }

    /// Render component with props (v0.2 feature)
    #[allow(dead_code)]
    pub fn render_with_props(props: &serde_json::Value) -> String {
        let props_json = serde_json::to_string(props).unwrap_or_default();
        format!(
            "<div data-component=\"{name}\" data-props=\'{{}}\' data-source=\"{file_path}\">{{{name}}}</div>",
            props_json
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_render() {
        let html = {name}::render();
        assert!(html.contains("{name}"));
    }

    #[test]
    fn test_component_render_with_props() {
        let props = serde_json::json!({{"key": "value"}});
        let html = {name}::render_with_props(&props);
        assert!(html.contains("key"));
    }
}
"#;

fn component_template(name: &str, file_path: &str) -> String {
    COMPONENT_TMPL.replace("{name}", name).replace("{file_path}", file_path)
}

fn extract_component_name(file_path: &str) -> String {
    file_path
        .split('/')
        .last()
        .unwrap_or("Component")
        .replace(".jsx", "")
        .replace(".js", "")
        .replace("index", "Index")
}

fn extract_handler_name(file_path: &str) -> String {
    let base = file_path
        .split('/')
        .last()
        .unwrap_or("server")
        .replace(".js", "")
        .replace(".jsx", "");
    
    if base == "server" || base == "server1" || base == "server2" {
        "handler".to_string()
    } else {
        format!("handler_{}", base.replace("server", ""))
    }
}

fn extract_route_path(file_path: &str) -> String {
    let file_name = file_path.split('/').last().unwrap_or("server.js");
    
    if file_name == "server.js" || file_name == "server1.js" {
        "/".to_string()
    } else if file_name == "server2.js" {
        "/2".to_string()
    } else {
        let base = file_name.replace(".js", "").replace("server", "");
        if base.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", base)
        }
    }
}

fn extract_component_from_imports(_file_path: &str) -> String {
    // TODO(v0.2): Parse actual imports to find the component
    // For v0.1, default to App
    "App".to_string()
}

fn find_first_component_name(modules: &[runts_plugin::hir::Module]) -> Option<String> {
    modules
        .iter()
        .filter_map(|m| m.source_path.as_ref())
        .filter(|p| p.contains("/component/") || p.ends_with(".jsx"))
        .map(|p| extract_component_name(p))
        .next()
}

fn collect_routes_code(modules: &[runts_plugin::hir::Module]) -> String {
    let mut routes_code = String::new();
    let mut has_routes = false;

    for module in modules {
        if let Some(source_path) = &module.source_path {
            if source_path.contains("server") || source_path.contains("main") {
                has_routes = true;
                let route_path = extract_route_path(source_path);
                let handler_name = extract_handler_name(source_path);
                routes_code.push_str(&format!(
                    "        .route(\"{}\", axum::routing::get({}))\n",
                    route_path, handler_name
                ));
            }
        }
    }

    if !has_routes {
        routes_code.push_str("        .route(\"/\", axum::routing::get(handler))\n");
    }

    routes_code
}

fn generate_axum_main(routes_code: &str, component_name: &str) -> String {
    let fallback_app = if component_name == "App" {
        r#"
/// Fallback App component (no .jsx files found)
pub struct App;

impl App {
    pub fn render() -> String {
        "<div>Welcome to React SSR</div>".to_string()
    }
}
"#
    } else {
        ""
    };

    format!(r#"use axum::{{
    Router,
    routing::get,
    response::Html,
    http::StatusCode,
    response::IntoResponse,
}};
use tokio::net::TcpListener;
use std::net::SocketAddr;

{fallback_app}

#[tokio::main]
async fn main() {{
    let app = Router::new()
{routes_code}
        .into_make_service();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("React SSR server running on http://{{}}", addr);
    axum::serve(listener, app).await.unwrap();
}}

async fn handler() -> impl IntoResponse {{
    let body = {component_name}::render();
    Html(format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>React SSR</title></head><body>{{body}}</body></html>",
        body = body
    ))
}}
"#)
}

fn generate_server_handler(
    handler_name: &str,
    route_path: &str,
    component_name: &str,
    file_path: &str,
) -> String {
    format!(r#"//! React SSR server: {file_path}
//!
//! Generated Axum route handler for React streaming SSR

use axum::{{
    Router,
    routing::get,
    response::Html,
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
}};

pub fn app() -> Router {{
    Router::new()
        .route("{route_path}", get({handler_name}))
}}

async fn {handler_name}() -> impl IntoResponse {{
    let body = {component_name}::render();
    Html(format!(
        "<!DOCTYPE html>
<html>
<head>
    <meta charset=\"utf-8\">
    <title>React SSR</title>
    <script src=\"/static/react-assets/client.js\"></script>
</head>
<body>
{{body}}
</body>
</html>",
        body = body
    ))
}}

async fn {handler_name}_with_params(Path(params): Path<std::collections::HashMap<String, String>>) -> impl IntoResponse {{
    let props = serde_json::json!({{"params": params}});
    let body = {component_name}::render_with_props(&props);
    Html(format!(
        "<!DOCTYPE html>
<html>
<head>
    <meta charset=\"utf-8\">
    <title>React SSR</title>
</head>
<body>
{{body}}
</body>
</html>",
        body = body
    ))
}}

#[cfg(test)]
mod tests {{
    use super::*;
    use axum::{{Router, body::Body}};
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_handler() {{
        let app = app();
        let response = app
            .oneshot(
                http::Request::builder()
                    .uri("{route_path}")
                    .method(http::Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("<!DOCTYPE html>"));
    }}
}}
"#,
        handler_name = handler_name,
        route_path = route_path,
        component_name = component_name,
        file_path = file_path
    )
}

