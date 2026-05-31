//! Fresh plugin implementation - minimal 0.1 proof-of-concept
//!
//! Architecture: codegen_module receives full HIR JSON and traverses it to find
//! Decl::Function items with JSX bodies. Uses codegen.rs helpers to generate
//! VNode-based Rust code.

#![allow(unsafe_code)]

use proc_macro2::TokenStream;
use runts_plugin::{CargoDep, DevAction, DevContext, DevState, Plugin, PluginError, RouteInfo};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::codegen::{jsx_element, jsx_expr, jsx_fragment, jsx_text, page_component};

pub struct FreshPlugin;

/// Dev state for Fresh plugin - tracks server process
struct FreshDevState {
    /// Project root directory
    project_root: PathBuf,
    /// Whether server has been spawned
    spawned: Arc<Mutex<bool>>,
    /// Child process handle (None until spawned)
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl DevState for FreshDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FreshDevState {
    fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            spawned: Arc::new(Mutex::new(false)),
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Spawn the dev server if not already spawned
    fn ensure_server_running(&self) -> Result<(), PluginError> {
        // Check if already spawned
        {
            let mut spawned = self.spawned.lock().unwrap();
            if *spawned {
                // Check if still running
                let mut child_guard = self.child.lock().unwrap();
                if let Some(ref mut child) = *child_guard {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            // Server exited
                            *spawned = false;
                            *child_guard = None;
                        }
                        Ok(None) => {
                            // Still running
                            return Ok(());
                        }
                        Err(_) => {
                            // Can't check, assume dead
                            *spawned = false;
                            *child_guard = None;
                        }
                    }
                }
            }
        }

        // Need to spawn
        // Binary is at .runts/build/target/debug/runts-app
        let build_dir = self.project_root.join(".runts").join("build");
        let binary_path = build_dir.join("target").join("debug").join("runts-app");

        // Compile if needed
        if !binary_path.exists() {
            self.compile_project()?;
        }

        // Spawn server
        println!("Starting dev server at http://127.0.0.1:8000");
        println!("Note: Hot reload coming in v0.2 - restart server manually for now");

        let child = Command::new(&binary_path)
            .current_dir(&self.project_root)
            .spawn()
            .map_err(|e| PluginError::new("fresh", "", &format!("failed to start server: {}", e)))?;

        {
            let mut spawned = self.spawned.lock().unwrap();
            *spawned = true;
            let mut child_guard = self.child.lock().unwrap();
            *child_guard = Some(child);
        }

        Ok(())
    }

    fn compile_project(&self) -> Result<(), PluginError> {
        // Use cargo to compile the project
        // For dev mode, we compile from .runts/build directory
        let build_dir = self.project_root.join(".runts").join("build");

        if !build_dir.exists() {
            return Err(PluginError::new("fresh", "", "runts build directory not found. Run 'runts build' first."));
        }

        println!("Compiling...");
        let output = Command::new("cargo")
            .current_dir(&build_dir)
            .args(&["build"])
            .output()
            .map_err(|e| PluginError::new("fresh", "", &format!("cargo build failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PluginError::new("fresh", "", &format!("cargo build failed:\n{}", stderr)));
        }

        Ok(())
    }
}

impl Plugin for FreshPlugin {
    fn name(&self) -> &str { "fresh" }
    fn help_text(&self) -> &str { "Fresh/Preact web framework" }

    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError> {
        let hir: runts_plugin::hir::Module = serde_json::from_str(hir_str)
            .map_err(|e| PluginError::codegen("fresh", "unknown", format!("failed to parse HIR: {e}")))?;

        // If we have HIR items, try to extract JSX and generate VNode code
        if let Some(items_json) = &hir.items_json {
            if let Some(jsx_code) = self.try_codegen_jsx(items_json, &hir) {
                return Ok(jsx_code);
            }
        }

        // Fall back to stub-based codegen
        let source_path = hir.source_path.as_deref().unwrap_or("");
        let route_info = hir.route_info.as_ref();

        if route_info.is_some() {
            self.codegen_route_module(source_path, route_info)
        } else if source_path.contains("/islands/") {
            self.codegen_island_module(source_path)
        } else if source_path.contains("/components/") {
            self.codegen_component_module(source_path)
        } else {
            Ok(self.codegen_generic_module())
        }
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep { name: "axum".to_string(), version: Some("0.7".to_string()), path: None, features: vec!["macros".to_string()] },
            CargoDep { name: "tokio".to_string(), version: Some("1.0".to_string()), path: None, features: vec!["full".to_string()] },
            CargoDep { name: "serde".to_string(), version: Some("1.0".to_string()), path: None, features: vec!["derive".to_string()] },
            CargoDep { name: "serde_json".to_string(), version: Some("1.0".to_string()), path: None, features: vec![] },
        ]
    }

    fn codegen_entry(&self, modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        self.generate_main_entry(modules)
    }

    fn dev_init(&self, ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(FreshDevState::new(ctx.root.clone())))
    }
    fn dev_run_once(&self, state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        let dev_state = state
            .as_any()
            .downcast_ref::<FreshDevState>()
            .ok_or_else(|| PluginError::new("fresh", "", "invalid dev state type"))?;

        dev_state.ensure_server_running()?;

        Ok(DevAction::Continue)
    }
    fn dev_reload(&self, ctx: &mut DevContext, state: &mut dyn DevState) -> Result<(), PluginError> {
        let dev_state = state
            .as_any()
            .downcast_ref::<FreshDevState>()
            .ok_or_else(|| PluginError::new("fresh", "", "invalid dev state type"))?;

        println!("File change detected, recompiling...");
        dev_state.compile_project()?;

        // Kill old server
        {
            let mut spawned = dev_state.spawned.lock().unwrap();
            let mut child_guard = dev_state.child.lock().unwrap();
            if let Some(ref mut child) = *child_guard {
                let _ = child.kill();
            }
            *spawned = false;
            *child_guard = None;
        }

        // Spawn new server
        dev_state.ensure_server_running()?;

        println!("Modules rescanned: {} files", ctx.modules.len());
        Ok(())
    }
}

impl FreshPlugin {
    fn codegen_route_module(&self, file_path: &str, route_info: Option<&RouteInfo>) -> Result<String, PluginError> {
        let path = route_info.map(|r| r.path.as_str()).unwrap_or("/");

        // Determine page type based on route path for more realistic stubs
        let (page_type, page_desc) = Self::classify_route(path);

        // Detect TSX features from file extension (crude proxy for actual HIR analysis)
        let tsx_features = Self::detect_tsx_features(file_path);

        Ok(format!(r#"//! Route module for {path}
//! Generated by runts-fresh 0.1
//! File: {file_path}
//! Detected TSX features: {tsx_features}
//! Full JSX→VNode codegen coming in v0.2

use runts_lib::runtime::vdom::VNode;

pub fn render() -> VNode {{
    VNode::element("div")
        .attr("class", "page {page_type}")
        .child(VNode::element("h1").child(VNode::text("{page_desc}")))
        .child(VNode::element("p").child(VNode::text("Page: {path}")))
        .child(VNode::element("p").child(VNode::text("Generated by runts-fresh 0.1")))
        .child(VNode::element("a").attr("href", "/").child(VNode::text("Go Home")))
}}
"#, path = path, file_path = file_path, page_type = page_type, page_desc = page_desc, tsx_features = tsx_features))
    }

    fn codegen_island_module(&self, file_path: &str) -> Result<String, PluginError> {
        let name = file_path.split('/').last().unwrap_or("Island").replace(".tsx", "").replace(".ts", "");

        // Generate a more realistic island stub based on common patterns
        // For 0.1, we generate static VNode; interactivity comes in 0.2
        Ok(format!(r#"//! Island: {name}
//! Generated by runts-fresh 0.1
//! Islands are client-side interactive components.
//! Full island codegen (useState, event handlers) coming in v0.2.

use runts_lib::runtime::vdom::VNode;

#[derive(serde::Serialize)]
pub struct {name}Props {{
    // Props will be injected at runtime
}}

pub fn render(props: {name}Props) -> VNode {{
    VNode::element("div")
        .attr("class", "island island-{name}")
        .child(VNode::element("span").child(VNode::text("Island: {name}")))
        .child(VNode::element("button")
            .attr("class", "island-btn")
            .child(VNode::text("Click me (interactive in 0.2)")))
}}
"#, name = name))
    }

    fn codegen_component_module(&self, file_path: &str) -> Result<String, PluginError> {
        let name = file_path.split('/').last().unwrap_or("Component").replace(".tsx", "").replace(".ts", "");

        Ok(format!(r#"//! Component: {name}
//! Generated by runts-fresh 0.1
//! Components are reusable UI pieces.
//! Full component codegen with props coming in v0.2.

use runts_lib::runtime::vdom::VNode;

pub fn render() -> VNode {{
    VNode::element("div")
        .attr("class", "component component-{name}")
        .child(VNode::element("span").child(VNode::text("Component: {name}")))
}}
"#, name = name))
    }

    fn codegen_generic_module(&self) -> String {
        "// Generic module - auto-generated by runts-fresh 0.1\n".to_string()
    }

    fn generate_main_entry(&self, modules: &[runts_plugin::hir::Module]) -> Result<String, PluginError> {
        let routes = self.collect_routes(modules);
        let (imports, handlers, router_calls) = self.generate_route_code(&routes);
        self.format_main_rs(&imports, &handlers, &router_calls)
    }

    fn collect_routes<'a>(&self, modules: &'a [runts_plugin::hir::Module]) -> Vec<&'a RouteInfo> {
        let mut routes: Vec<&RouteInfo> = modules.iter().filter_map(|m| m.route_info.as_ref()).collect();
        routes.sort_by(|a, b| a.path.cmp(&b.path));
        routes
    }

    fn generate_route_code(&self, routes: &[&RouteInfo]) -> (String, String, String) {
        let mut imports = String::new();
        let mut handlers = String::new();
        let mut router_calls = String::new();

        for route in routes {
            let safe_name = self.module_name_from_path(&route.file_path);
            let axum_path = self.to_axum_path(&route.path);
            imports.push_str(&format!("mod {};\n", safe_name));
            handlers.push_str(&format!(
                "async fn {}_handler() -> axum::response::Html<String> {{ let v = {}::render(); axum::response::Html(v.to_html()) }}\n",
                safe_name, safe_name
            ));
            router_calls.push_str(&format!("        .route(\"{}\", axum::routing::get({}_handler))\n", axum_path, safe_name));
        }

        (imports, handlers, router_calls)
    }

    fn format_main_rs(&self, imports: &str, handlers: &str, router_calls: &str) -> Result<String, PluginError> {
        // If no routes, add a fallback root handler so Router::new() has valid syntax
        let router_body = if router_calls.trim().is_empty() {
            r#"        .route("/", axum::routing::get(|| async { "Hello from runts-fresh! Run 'runts build --plugin fresh' with TSX routes." }))
"#.to_string()
        } else {
            router_calls.trim().to_string()
        };

        Ok(format!(r#"//! Auto-generated by runts-fresh plugin 0.1
use axum::Router;
use std::net::SocketAddr;

{}

{}

#[tokio::main]
async fn main() {{
    let app = Router::new()
{};
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    println!("Starting server on {{}}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await
        .expect("failed to bind to port");
    axum::serve(listener, app).await
        .expect("server error");
}}
"#, imports.trim(), handlers.trim(), router_body))
    }

    fn module_name_from_path(&self, path: &str) -> String {
        // Convert path like "blog/[slug].tsx" or "blog/index.tsx" to Rust module name
        // Strip extension, remove brackets, replace slashes with __
        let without_ext = path.replace(".tsx", "").replace(".ts", "").replace(".jsx", "").replace(".js", "");
        let no_brackets = without_ext.replace("[", "").replace("]", "");
        let with_underscores = no_brackets.replace("/", "__");
        with_underscores.replace("-", "_").replace("\\", "__")
    }

    fn to_axum_path(&self, path: &str) -> String {
        let p = path.replace("[", ":").replace("]", "");
        if p.starts_with('/') { p } else { format!("/{}", p) }
    }

    /// Classify route path to generate more specific stubs
    fn classify_route(path: &str) -> (&'static str, &'static str) {
        if path == "/" { return ("home", "Welcome to runts-fresh"); }
        if path == "/about" { return ("about", "About Page"); }
        if path == "/contact" { return ("contact", "Contact Us"); }
        if path == "/blog" || path == "/blog/" || path == "/blog/index" {
            return ("blog-list", "Blog");
        }
        if path.starts_with("/blog/") && !path.contains('[') {
            return ("blog-post", "Blog Post");
        }
        if path.contains('[') { return ("dynamic", "Dynamic Route"); }
        ("page", "Page")
    }

    /// Detect TSX features from file path (crude proxy for actual HIR analysis)
    /// Full HIR-based detection coming in 0.2
    fn detect_tsx_features(file_path: &str) -> &'static str {
        if file_path.contains("/islands/") {
            "JSX elements, island interactivity, useState hooks"
        } else if file_path.contains("/components/") {
            "JSX elements, component composition"
        } else if file_path.ends_with(".tsx") {
            "JSX elements, default export function, handler"
        } else {
            "TypeScript statements"
        }
    }

    /// Try to codegen JSX from HIR items JSON.
    /// Returns Some(code) if JSX was detected and codegen succeeded, None otherwise.
    fn try_codegen_jsx(&self, items: &serde_json::Value, _hir: &runts_plugin::hir::Module) -> Option<String> {
        // Parse items as array
        let items_arr = items.as_array()?;

        for item in items_arr {
            // Find Decl::Function items
            if item.get("type")?.as_str()? != "Decl" {
                continue;
            }
            let decl = item.get("Decl")?;
            if decl.get("kind")?.as_str()? != "Function" {
                continue;
            }

            // Extract function name and body
            let name = decl.get("name")?.as_str()?;
            let body = decl.get("body")?;

            // Check if body is present and contains JSX
            if body.is_null() {
                continue;
            }
            let body_str = body.to_string();
            if !body_str.contains("\"opening\"") && !body_str.contains("JSX") {
                continue;
            }

            // Find JSX expression in the return statement
            if let Some(jsx_expr) = self.find_jsx_in_body(body) {
                let jsx_code = self.generate_jsx_vnode_code(jsx_expr)?;
                let page_fn = page_component(name, jsx_code);
                let code = self.wrap_page_module(name, &page_fn.to_string());
                return Some(code);
            }
        }

        None
    }

    /// Find JSX expression in function body.
    /// Returns the JSX expression JSON if found.
    fn find_jsx_in_body(&self, body: &serde_json::Value) -> Option<serde_json::Value> {
        // Body structure: { "Block": { "stmts": [...] } } or direct JSX
        if let Some(block) = body.get("Block") {
            let stmts = block.get("stmts")?.as_array()?;
            for stmt in stmts {
                if let Some(jsx) = self.find_jsx_in_stmt(stmt) {
                    return Some(jsx);
                }
            }
        } else if self.is_jsx_expr(body) {
            return Some(body.clone());
        }
        None
    }

    /// Find JSX in a statement.
    // allow:complexity,too_many_lines
    fn find_jsx_in_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = stmt.get("kind")?.as_str()?;
        match kind {
            "Return" => {
                // Direct return: return <expr>
                let arg = stmt.get("arg")?;
                if self.is_jsx_expr(arg) {
                    return Some(arg.clone());
                }
                // Check nested expressions
                return self.find_jsx_in_expr(arg);
            }
            "Expr" => {
                // Expression statement: <expr>
                let expr = stmt.get("expr")?;
                if self.is_jsx_expr(expr) {
                    return Some(expr.clone());
                }
                return self.find_jsx_in_expr(expr);
            }
            "Block" => {
                // Block - traverse statements
                if let Some(stmts) = stmt.get("stmts").and_then(|s| s.as_array()) {
                    for s in stmts {
                        if let Some(jsx) = self.find_jsx_in_stmt(s) {
                            return Some(jsx);
                        }
                    }
                }
            }
            "If" => {
                // If statement - check consequent and alternate
                if let Some(cons) = stmt.get("consequent") {
                    if let Some(jsx) = self.find_jsx_in_stmt(cons) {
                        return Some(jsx);
                    }
                }
                if let Some(alt) = stmt.get("alternate") {
                    return self.find_jsx_in_stmt(alt);
                }
            }
            _ => {}
        }
        None
    }

    /// Find JSX in an expression.
    fn find_jsx_in_expr(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        let kind = expr.get("kind")?.as_str()?;
        match kind {
            "JSX" => return Some(expr.clone()),
            "Cond" => {
                // Ternary: test ? consequent : alternate
                if let Some(cons) = expr.get("consequent") {
                    if let Some(jsx) = self.find_jsx_in_expr(cons) {
                        return Some(jsx);
                    }
                }
                if let Some(alt) = expr.get("alternate") {
                    return self.find_jsx_in_expr(alt);
                }
            }
            _ => {}
        }
        None
    }

    /// Check if JSON value is a JSX expression.
    fn is_jsx_expr(&self, val: &serde_json::Value) -> bool {
        val.get("opening").is_some() && val.get("children").is_some()
    }

    /// Generate VNode code from JSX expression JSON.
    fn generate_jsx_vnode_code(&self, jsx: serde_json::Value) -> Option<TokenStream> {
        let opening = jsx.get("opening")?;
        let name = opening.get("name")?;
        let tag = self.jsx_name_to_tag(name)?;

        // Convert attributes
        let attrs = self.extract_jsx_attrs(opening.get("attrs")?)?;

        // Convert children
        let children = self.extract_jsx_children(jsx.get("children")?)?;

        Some(jsx_element(&tag, attrs, children))
    }

    /// Convert JSXName to tag string.
    fn jsx_name_to_tag(&self, name: &serde_json::Value) -> Option<String> {
        match name {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(obj) => {
                if let Some(ident) = obj.get("Ident") {
                    return ident.as_str().map(String::from);
                }
                None
            }
            _ => None,
        }
    }

    /// Extract attributes from JSX opening element.
    fn extract_jsx_attrs(&self, attrs: &serde_json::Value) -> Option<Vec<(String, TokenStream)>> {
        let arr = attrs.as_array()?;
        let mut result = Vec::new();
        for attr in arr {
            if let Some(obj) = attr.get("Attr") {
                let name = obj.get("name")?.as_str()?.to_string();
                let value = self.jsx_attr_value_to_tokenstream(obj.get("value")?)?;
                result.push((name, value));
            }
            // Ignore Spread attributes for now
        }
        Some(result)
    }

    /// Convert JSX attribute value to TokenStream.
    // allow:complexity
    fn jsx_attr_value_to_tokenstream(&self, val: &serde_json::Value) -> Option<TokenStream> {
        match val {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => {
                let lit = proc_macro2::Literal::string(s);
                Some(quote::quote! { #lit })
            }
            serde_json::Value::Object(obj) => {
                // Handle actual HIR format: { "String": "value" }
                if let Some(s) = obj.get("String")?.as_str() {
                    let lit = proc_macro2::Literal::string(s);
                    return Some(quote::quote! { #lit });
                }
                // Expression value: { "Expr": <expr> }
                if let Some(expr_val) = obj.get("Expr") {
                    let kind = expr_val.get("kind")?.as_str()?;
                    match kind {
                        "Ident" => {
                            let name = expr_val.get("name")?.as_str()?;
                            Some(quote::quote! { #name })
                        }
                        "String" => {
                            let s = expr_val.get("String")?.as_str()?;
                            Some(quote::quote! { #s })
                        }
                        "Number" => {
                            let n = expr_val.get("0")?.as_f64()?;
                            Some(quote::quote! { #n })
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract children from JSX element.
    fn extract_jsx_children(&self, children: &serde_json::Value) -> Option<Vec<TokenStream>> {
        let arr = children.as_array()?;
        let mut result = Vec::new();
        for child in arr {
            match self.jsx_child_to_tokenstream(child) {
                Some(Some(ts)) => {
                    result.push(ts);
                }
                Some(None) => {
                    // Child processed but resulted in nothing (e.g., Spread)
                }
                None => {
                    // Child processing returned None - skip
                }
            }
        }
        Some(result)
    }

    /// Convert a JSX child to TokenStream.
    // allow:complexity
    fn jsx_child_to_tokenstream(&self, child: &serde_json::Value) -> Option<Option<TokenStream>> {
        // Text child
        if let Some(text) = child.as_str() {
            return Some(Some(jsx_text(text)));
        }

        // Handle actual HIR format: {"Text": "..."}, {"JSX": {...}}, etc.
        // without "kind" wrapper
        if let Some(text) = child.get("Text").and_then(|v| v.as_str()) {
            return Some(Some(jsx_text(text)));
        }
        if child.get("JSX").is_some() {
            let jsx_expr = child.get("JSX")?;
            return self.generate_jsx_vnode_code(jsx_expr.clone()).map(Some);
        }
        if child.get("Fragment").is_some() {
            let frag_children = child.get("Fragment")?.get("children")?;
            let children = self.extract_jsx_children(frag_children)?;
            return Some(Some(jsx_fragment(children)));
        }
        if child.get("Expr").is_some() {
            let expr_val = child.get("Expr")?;
            if let Some(ts) = self.jsx_expr_to_tokenstream(expr_val)? {
                return Some(Some(ts));
            } else {
                return Some(None);
            }
        }
        if child.get("Spread").is_some() {
            // Spread children - skip for v0.1
            return Some(None);
        }

        // Fallback: check for "kind" field (old format)
        if let Some(kind) = child.get("kind").and_then(|v| v.as_str()) {
            match kind {
                "Text" => {
                    let text = child.get("Text").and_then(|v| v.as_str())?;
                    return Some(Some(jsx_text(text)));
                }
                "JSX" => {
                    let jsx_expr = child.get("JSX")?;
                    return self.generate_jsx_vnode_code(jsx_expr.clone()).map(Some);
                }
                "Fragment" => {
                    let frag_children = child.get("Fragment")?.get("children")?;
                    let children = self.extract_jsx_children(frag_children)?;
                    return Some(Some(jsx_fragment(children)));
                }
                "Expr" => {
                    let expr_val = child.get("Expr")?;
                    if let Some(ts) = self.jsx_expr_to_tokenstream(expr_val)? {
                        return Some(Some(ts));
                    } else {
                        return Some(None);
                    }
                }
                "Spread" => {
                    return Some(None);
                }
                _ => return Some(None),
            }
        }

        Some(None)
    }

    /// Convert JSX expression to TokenStream.
    // allow:complexity
    fn jsx_expr_to_tokenstream(&self, expr: &serde_json::Value) -> Option<Option<TokenStream>> {
        // Handle actual HIR format: direct keys without "kind" wrapper
        if let Some(name) = expr.get("Ident")?.as_str() {
            return Some(Some(jsx_expr(quote::quote! { #name })));
        }
        if let Some(s) = expr.get("String")?.as_str() {
            return Some(Some(jsx_text(s)));
        }
        if let Some(n) = expr.get("Number")?.as_f64() {
            return Some(Some(jsx_expr(quote::quote! { #n })));
        }
        if let Some(b) = expr.get("Boolean")?.as_bool() {
            return Some(Some(jsx_expr(quote::quote! { #b })));
        }

        // Fallback: check for "kind" field (old format)
        if let Some(kind) = expr.get("kind")?.as_str() {
            match kind {
                "Ident" => {
                    let name = expr.get("name")?.as_str()?;
                    return Some(Some(jsx_expr(quote::quote! { #name })));
                }
                "String" => {
                    let s = expr.get("String")?.as_str()?;
                    return Some(Some(jsx_text(s)));
                }
                "Number" => {
                    let n = expr.get("0")?.as_f64()?;
                    return Some(Some(jsx_expr(quote::quote! { #n })));
                }
                "Boolean" => {
                    let b = expr.get("0")?.as_bool()?;
                    return Some(Some(jsx_expr(quote::quote! { #b })));
                }
                "Bin" => {
                    return Some(None);
                }
                "Call" => {
                    return Some(None);
                }
                _ => return Some(None),
            }
        }

        Some(None)
    }

    /// Wrap page component code in a module.
    fn wrap_page_module(&self, name: &str, page_fn: &str) -> String {
        format!(
            r#"//! Page component: {name}
//! Generated by runts-fresh 0.1

use runts_lib::runtime::vdom::VNode;

{page_fn}

pub fn render() -> VNode {{
    {name}()
}}
"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_codegen_jsx_with_simple_div() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Hello",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "div" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            { "Text": "Hello World" }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "div" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);

        assert!(result.is_some(), "Should generate code for JSX");
        let code = result.unwrap();
        eprintln!("Generated code:\n{}", code);

        // Check for VNode generation
        assert!(code.contains("VNode::Element") || code.contains("VNode"), "Should contain VNode");
        assert!(code.contains("\"div\""), "Should contain div tag");
        assert!(code.contains("Hello World"), "Should contain text");
    }

    #[test]
    fn test_try_codegen_jsx_with_attributes() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Home",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "div" },
                                            "attrs": [
                                                {
                                                    "Attr": {
                                                        "name": "class",
                                                        "value": "home"
                                                    }
                                                }
                                            ],
                                            "self_closing": false
                                        },
                                        "children": [
                                            { "Text": "Welcome" }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "div" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);

        assert!(result.is_some(), "Should generate code for JSX with attrs");
        let code = result.unwrap();
        eprintln!("Generated code with attrs:\n{}", code);

        assert!(code.contains("\"class\""), "Should contain class attribute");
    }

    #[test]
    fn test_try_codegen_jsx_no_jsx_returns_none() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "NoJsx",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": { "String": "hello" }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);

        assert!(result.is_none(), "Should return None for non-JSX functions");
    }

    #[test]
    fn test_try_codegen_jsx_nested_elements() {
        let plugin = FreshPlugin;
        // Simplified nested: just the inner JSX structure without the outer wrapper
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "Nested",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "div" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            {
                                                "kind": "JSX",
                                                "JSX": {
                                                    "opening": {
                                                        "name": { "Ident": "span" },
                                                        "attrs": [],
                                                        "self_closing": false
                                                    },
                                                    "children": [
                                                        { "Text": "nested" }
                                                    ],
                                                    "closing": {
                                                        "name": { "Ident": "span" }
                                                    }
                                                }
                                            }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "div" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);
        eprintln!("Result: {:?}", result);

        assert!(result.is_some(), "Should generate code for nested JSX");
        let code = result.unwrap();
        eprintln!("Generated nested code:\n{}", code);

        assert!(code.contains("\"div\""), "Should contain outer div");
        assert!(code.contains("\"span\""), "Should contain inner span");
    }

    #[test]
    fn test_try_codegen_jsx_with_expr_child() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "WithExpr",
                    "body": {
                        "Block": {
                            "stmts": [
                                {
                                    "kind": "Return",
                                    "arg": {
                                        "kind": "JSX",
                                        "opening": {
                                            "name": { "Ident": "div" },
                                            "attrs": [],
                                            "self_closing": false
                                        },
                                        "children": [
                                            {
                                                "kind": "Expr",
                                                "Expr": {
                                                    "kind": "Ident",
                                                    "name": "name"
                                                }
                                            }
                                        ],
                                        "closing": {
                                            "name": { "Ident": "div" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);

        assert!(result.is_some(), "Should generate code for JSX with expression child");
        let code = result.unwrap();
        eprintln!("Generated expr code:\n{}", code);

        assert!(code.contains("\"div\""), "Should contain div");
    }

    #[test]
    fn test_try_codegen_jsx_no_body_returns_none() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Function",
                    "name": "NoBody",
                    "body": null
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);

        assert!(result.is_none(), "Should return None for function with null body");
    }

    #[test]
    fn test_try_codegen_jsx_not_function_decl_returns_none() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([
            {
                "type": "Decl",
                "Decl": {
                    "kind": "Variable",
                    "name": "x",
                    "init": { "kind": "Number", "0": 42 }
                }
            }
        ]);

        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);

        assert!(result.is_none(), "Should return None for non-function declarations");
    }

    #[test]
    fn test_dev_state_init() {
        use std::path::PathBuf;
        let plugin = FreshPlugin;
        let mut ctx = DevContext {
            root: PathBuf::from("/tmp/test-project"),
            modules: vec![],
        };
        let state = plugin.dev_init(&mut ctx);
        assert!(state.is_ok(), "Should initialize dev state");
        let _ = state.unwrap();
    }

    #[test]
    fn test_dev_state_spawn_requires_build_dir() {
        use std::path::PathBuf;
        use std::fs;

        let plugin = FreshPlugin;
        let project_root = PathBuf::from("/tmp/nonexistent-runts-test");
        let _ = fs::create_dir_all(&project_root);

        let mut ctx = DevContext {
            root: project_root.clone(),
            modules: vec!["test.tsx".to_string()],
        };
        let state = plugin.dev_init(&mut ctx);
        assert!(state.is_ok(), "Should initialize dev state");
        let mut state = state.unwrap();

        // dev_run_once should error since build dir doesn't exist
        let result = plugin.dev_run_once(&mut *state);
        assert!(result.is_err(), "Should error when .runts/build missing");

        // Cleanup
        let _ = fs::remove_dir_all(&project_root);
    }
}
