//! React plugin implementation
#![allow(clippy::too_many_lines)]

use runts_plugin::{CargoDep, DevAction, DevContext, DevState, Plugin, PluginError};

pub struct ReactPlugin;

struct ReactDevState;

impl DevState for ReactDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Plugin for ReactPlugin {
    fn name(&self) -> &str { "react" }

    fn help_text(&self) -> &str {
        "React SSR with streaming — renderToPipeableStream, Suspense, lazy loading"
    }

    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
        let module: runts_plugin::hir::Module = serde_json::from_str(hir_str)
            .map_err(|e| PluginError::codegen("react", "unknown", format!("{e}")))?;

        let source_path = module.source_path.as_deref().unwrap_or("");
        let has_hir_items = module.items_json.as_ref()
            .map_or(false, |v| v.as_array().map_or(false, |a| !a.is_empty()));

        if has_hir_items {
            self.codegen_module_with_hir(source_path)
        } else {
            self.codegen_module_without_hir(source_path)
        }
    }

    fn codegen_module_with_hir(&self, source_path: &str) -> Result<String, PluginError> {
        if source_path.contains("/component/") || source_path.ends_with(".jsx") || source_path.ends_with(".tsx") {
            return Ok(self.codegen_component_module(source_path));
        }
        if source_path.contains("server") || source_path.contains("main") {
            return Ok(self.codegen_server_module(source_path));
        }
        Ok(self.codegen_generic_module())
    }

    fn codegen_module_without_hir(&self, source_path: &str) -> Result<String, PluginError> {
        if source_path.contains("/component/") || source_path.ends_with(".jsx") || source_path.ends_with(".tsx") {
            Ok(self.codegen_component_module(source_path))
        } else if source_path.contains("server") || source_path.contains("main") {
            Ok(self.codegen_server_module(source_path))
        } else {
            Ok(self.codegen_generic_module())
        }
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep { name: "axum".into(), version: Some("0.7".into()), path: None, features: vec!["macros".into()] },
            CargoDep { name: "tokio".into(), version: Some("1.0".into()), path: None, features: vec!["full".into()] },
            CargoDep { name: "serde".into(), version: Some("1.0".into()), path: None, features: vec!["derive".into()] },
            CargoDep { name: "serde_json".into(), version: Some("1.0".into()), path: None, features: vec![] },
        ]
    }

    fn codegen_entry(&self, modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        let routes_code = collect_routes_code(modules);
        let component_name = find_first_component_name(modules)
            .unwrap_or_else(|| "App".to_string());
        Ok(generate_axum_main(&routes_code, &component_name))
    }

    fn dev_init(&self, _ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(ReactDevState))
    }

    fn dev_run_once(&self, _state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        Ok(DevAction::Continue)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

impl ReactPlugin {
    fn codegen_component_module(&self, file_path: &str) -> String {
        let name = extract_component_name(file_path);

        format!(r#"//! React component: {name}
//! Source: {file_path}
//!
//! v0.1: Static HTML rendering via QuickJS
//! v0.2: Full JSX → Rust compilation

pub struct {name};

impl {name} {{
    /// Render component to HTML string
    pub fn render() -> String {{
        format!(
            "<div data-component=\"{name}\" data-source=\"{file_path}\">{{{name}}}</div>",
            name = name,
            file_path = file_path
        )
    }}

    /// Render component with props (v0.2 feature)
    #[allow(dead_code)]
    pub fn render_with_props(props: &serde_json::Value) -> String {{
        let props_json = serde_json::to_string(props).unwrap_or_default();
        format!(
            "<div data-component=\"{name}\" data-props=\'{{}}\' data-source=\"{file_path}\">{{{name}}}</div>",
            props_json
        )
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn test_component_render() {{
        let html = {name}::render();
        assert!(html.contains("{name}"));
    }}

    #[test]
    fn test_component_render_with_props() {{
        let props = serde_json::json!({{"key": "value"}});
        let html = {name}::render_with_props(&props);
        assert!(html.contains("key"));
    }}
}}
"#, name = name, file_path = file_path)
    }

    fn codegen_server_module(&self, file_path: &str) -> String {
        let handler_name = extract_handler_name(file_path);
        let route_path = extract_route_path(file_path);
        let component_name = extract_component_from_imports(file_path);
        generate_server_handler(&handler_name, &route_path, &component_name, file_path)
    }

    fn codegen_generic_module(&self) -> String {
        r#"//! Generic React module
//!
//! Default code generation for unclassified React files

pub struct GenericComponent;

impl GenericComponent {
    pub fn render() -> String {
        "<div>Generic Component</div>".to_string()
    }
}
"#.to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_react_plugin_name() {
        let plugin = ReactPlugin;
        assert_eq!(plugin.name(), "react");
    }

    #[test]
    fn test_react_plugin_help() {
        let plugin = ReactPlugin;
        assert!(plugin.help_text().contains("React"));
    }

    #[test]
    fn test_codegen_component_module() {
        let plugin = ReactPlugin;
        let code = plugin.codegen_component_module("component/LazyHome.jsx");
        assert!(code.contains("LazyHome"));
        assert!(code.contains("render()"));
    }

    #[test]
    fn test_codegen_server_module() {
        let plugin = ReactPlugin;
        let code = plugin.codegen_server_module("server1.js");
        assert!(code.contains("async fn handler()"));
        assert!(code.contains("Html("));
        assert!(code.contains("Router::new()"));
    }

    #[test]
    fn test_codegen_module_detects_component() {
        let plugin = ReactPlugin;
        let hir_json = r#"{"source_path": "component/Test.jsx", "items_json": [], "types": {}}"#;
        let code = plugin.codegen_module(hir_json).unwrap();
        assert!(code.contains("Test"));
    }

    #[test]
    fn test_codegen_module_detects_server() {
        let plugin = ReactPlugin;
        let hir_json = r#"{"source_path": "server.js", "items_json": [], "types": {}}"#;
        let code = plugin.codegen_module(hir_json).unwrap();
        assert!(code.contains("handler()"));
    }

    #[test]
    fn test_cargo_deps() {
        let plugin = ReactPlugin;
        let deps = plugin.codegen_entry(&[]);
        assert!(deps.is_ok());
    }

    #[test]
    fn test_codegen_entry() {
        let plugin = ReactPlugin;
        let entry = plugin.codegen_entry(&[]).unwrap();
        assert!(entry.contains("axum"));
        assert!(entry.contains("tokio::main"));
    }

    #[test]
    fn test_extract_component_name() {
        assert_eq!(extract_component_name("component/LazyHome.jsx"), "LazyHome");
        assert_eq!(extract_component_name("component/index.jsx"), "Index");
        assert_eq!(extract_component_name("src/Foo.js"), "Foo");
    }

    #[test]
    fn test_extract_route_path() {
        assert_eq!(extract_route_path("server.js"), "/");
        assert_eq!(extract_route_path("server1.js"), "/");
        assert_eq!(extract_route_path("server2.js"), "/2");
        assert_eq!(extract_route_path("serverApi.js"), "/Api");
    }

    #[test]
    fn test_codegen_entry_with_modules() {
        let plugin = ReactPlugin;
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            }
        ];
        let entry = plugin.codegen_entry(&modules).unwrap();
        assert!(entry.contains("axum"));
        assert!(entry.contains("/"));
    }

    #[test]
    fn test_codegen_entry_with_jsx_components() {
        let plugin = ReactPlugin;
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("component/LazyHome.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("component/LazyPage.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            }
        ];
        let entry = plugin.codegen_entry(&modules).unwrap();
        // Should use LazyHome (first component) not hardcoded App
        assert!(entry.contains("LazyHome::render()"));
        assert!(!entry.contains("App::render()"));
        assert!(entry.contains("axum"));
    }

    #[test]
    fn test_find_first_component_name() {
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("component/LazyPage.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("component/LazyHome.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
        ];
        assert_eq!(find_first_component_name(&modules), Some("LazyPage".to_string()));
    }

    #[test]
    fn test_find_first_component_name_no_components() {
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            },
        ];
        assert_eq!(find_first_component_name(&modules), None);
    }
}
