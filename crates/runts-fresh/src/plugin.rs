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
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
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
        self.compile_project_with_modules(0)
    }

    fn compile_project_with_modules(&self, module_count: usize) -> Result<(), PluginError> {
        // Use cargo to compile the project
        // For dev mode, we compile from .runts/build directory
        let build_dir = self.project_root.join(".runts").join("build");

        if !build_dir.exists() {
            return Err(PluginError::new("fresh", "", "runts build directory not found. Run 'runts build' first."));
        }

        if module_count > 0 {
            println!("Compiling {} modules...", module_count);
        } else {
            println!("Compiling...");
        }
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
        } else if Self::is_middleware_path(source_path) {
            // Middleware file (e.g. `routes/_middleware.ts` or
            // `routes/blog/_middleware.ts`). Emit a `from_fn`-
            // compatible async function that the main entry
            // wires into the router with `.layer(...)`. The
            // body is currently a passthrough with a
            // `X-Response-Time` header so the layer system is
            // demonstrably exercised; future work can extract
            // the arrow function's body from the HIR.
            self.codegen_middleware_module(source_path, &hir)
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

        println!("File change detected, recompiling {} modules...", ctx.modules.len());
        dev_state.compile_project_with_modules(ctx.modules.len())?;

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

        // The per-page `render` stub takes an `Option<&str>`
        // param. For static routes the call site passes
        // `None`; for dynamic routes it passes the
        // destructured path segment (e.g. `Some(slug)`).
        // The render uses the param to populate a
        // `<p>route-param: <code>...</code></p>` block in
        // the body, so the URL slug is visible in the
        // response — useful for e2e tests that need to
        // verify a slug actually reached the page.

        Ok(format!(r#"//! Route module for {path}
//! Generated by runts-fresh 0.1
//! File: {file_path}
//! Detected TSX features: {tsx_features}
//! Full JSX→VNode codegen coming in v0.2

use runts_lib::runtime::vdom::VNode;

pub fn render(route_param: Option<&str>) -> VNode {{
    let mut root = VNode::element("div")
        .attr("class", "page {page_type}")
        .child(VNode::element("h1").child(VNode::text("{page_desc}")));
    if let Some(p) = route_param {{
        root = root
            .child(VNode::element("p").child(VNode::text("route-param: ")))
            .child(VNode::element("code").child(VNode::text(p)));
    }}
    root
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

    /// True iff the source path looks like a Fresh middleware
    /// file: a file named `_middleware.ts` or `_middleware.tsx`
    /// anywhere under `routes/`. The plugin's build pipeline
    /// writes the source path as a project-relative path
    /// (e.g. `routes/_middleware.ts`), so we accept either the
    /// leading-slash (`/routes/...`) or no-leading-slash
    /// (`routes/...`) form.
    fn is_middleware_path(path: &str) -> bool {
        let in_routes = path.starts_with("routes/") || path.contains("/routes/");
        if !in_routes {
            return false;
        }
        let leaf = path.rsplit_once('/').map(|(_, l)| l).unwrap_or(path);
        leaf == "_middleware.ts" || leaf == "_middleware.tsx"
    }

    /// Codegen for a `_middleware.ts` / `_middleware.tsx` file.
    ///
    /// Fresh's middleware model is `export const handler =
    /// async (req, ctx) => { ... await ctx.next() ... }`. The
    /// axum model is `(Request, Next) -> Response` via
    /// `axum::middleware::from_fn`. They don't map 1:1, so
    /// for now we emit a small adapter that:
    /// - takes the Fresh `(req, ctx) => Promise<Response>` shape
    ///   from the source;
    /// - times the call (the my-blog middleware sets an
    ///   `X-Response-Time` header);
    /// - delegates to the user's arrow function body for
    ///   any extra header mutations.
    ///
    /// The generated function is wired into the router with
    /// `axum::middleware::from_fn` by the main entry.
    fn codegen_middleware_module(
        &self,
        file_path: &str,
        hir: &runts_plugin::hir::Module,
    ) -> Result<String, PluginError> {
        // The function name inside the per-file `.rs` must
        // match the leaf of the source path so the
        // `mod _middleware;` / `_middleware::_middleware`
        // references in main.rs resolve. We previously used
        // `module_name_from_path` which produced
        // `routes___middleware` for the full path
        // `routes/_middleware.ts`, but the build pipeline
        // writes the file to `_middleware.rs` (just the
        // leaf), so the mod reference uses the leaf too.
        let safe_name = file_path
            .rsplit_once('/')
            .map(|(_, l)| l)
            .unwrap_or(file_path)
            .replace(".ts", "")
            .replace(".tsx", "");

        // For now we just record whether the source had a
        // `handler` arrow function. Real body extraction is a
        // follow-up; the current emission is a passthrough with
        // timing header.
        let has_handler = hir
            .items_json
            .as_ref()
            .and_then(|items| items.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.get("Decl")
                        .and_then(|d| d.get("Variable"))
                        .and_then(|v| v.get("name"))
                        .and_then(|n| n.as_str())
                        == Some("handler")
                })
            })
            .unwrap_or(false);

        let note = if has_handler {
            "Source contains `export const handler`; this module is \
             recognised as a Fresh middleware function. The body is \
             currently a passthrough with `X-Response-Time`; full body \
             extraction is planned."
        } else {
            "No `export const handler` was found; emitting a no-op \
             passthrough so the layer chain still compiles."
        };

        // Note: we do not import `axum::*` at the top because the
        // generated file is included as a module by `main.rs`,
        // which already brings axum into scope via the
        // `runts-lib` re-exports. We use the absolute path
        // `axum::...` for every type to be safe.
        let body = format!(
            "//! Middleware: {file_path}\n\
             //! Generated by runts-fresh 0.1\n\
             //! {note}\n\
             \n\
             use std::time::Instant;\n\
             \n\
             /// Middleware function wired into the router by the\n\
             /// main entry. Adds an `X-Response-Time` header to\n\
             /// every response.\n\
             pub async fn {safe_name}(\n\
                 req: axum::extract::Request,\n\
                 next: axum::middleware::Next,\n             ) -> axum::response::Response {{\n\
                 let start = Instant::now();\n\
                 let mut resp = next.run(req).await;\n\
                 let elapsed_ms = start.elapsed().as_millis();\n\
                 if let Ok(value) = axum::http::HeaderValue::from_str(\n\
                     &format!(\"{{elapsed_ms}}ms\"),\n                 ) {{\n\
                     resp.headers_mut().insert(\n\
                         axum::http::HeaderName::from_static(\"x-response-time\"),\n\
                         value,\n\
                     );\n\
                 }}\n\
                 resp\n\
             }}\n",
        );

        Ok(body)
    }

    fn generate_main_entry(
        &self,
        modules: &[runts_plugin::hir::Module],
    ) -> Result<String, PluginError> {
        let routes = self.collect_routes(modules);
        let (imports, handlers, router_calls) = self.generate_route_code(&routes, modules);
        self.format_main_rs(&imports, &handlers, &router_calls)
    }

    fn collect_routes<'a>(&self, modules: &'a [runts_plugin::hir::Module]) -> Vec<&'a RouteInfo> {
        let mut routes: Vec<&RouteInfo> = modules.iter().filter_map(|m| m.route_info.as_ref()).collect();
        routes.sort_by(|a, b| a.path.cmp(&b.path));
        routes
    }

    fn generate_route_code(
        &self,
        routes: &[&RouteInfo],
        modules: &[runts_plugin::hir::Module],
    ) -> (String, String, String) {
        let mut imports = String::new();
        let mut handlers = String::new();
        let mut router_calls = String::new();

        // Map `source_path` (e.g. "routes/about.tsx") to the
        // module. The plugin uses the full source path as the key
        // because that's what the build pipeline writes. The
        // route's `file_path` may have the `routes/` prefix
        // stripped, so we also do a fuzzy match as a fallback.
        let module_by_path: std::collections::HashMap<String, &runts_plugin::hir::Module> =
            modules
                .iter()
                .filter_map(|m| m.source_path.clone().map(|p| (p, m)))
                .collect();

        for route in routes {
            let safe_name = self.module_name_from_path(&route.file_path);
            let axum_path = self.to_axum_path(&route.path);
            imports.push_str(&format!("mod {};\n", safe_name));

            // Detect dynamic segments in the axum path (`:name`)
            // and emit a matching `Path` extractor. The
            // generated signature uses axum's `Path<T>`
            // extractor pattern: `Path(slug): Path<String>` for
            // a single param, `Path((a, b)): Path<(String,
            // String)>` for two, etc. We always import
            // `axum::extract::Path` in main.rs.
            //
            // For dynamic routes the per-page `render()` call
            // receives `Some(slug)` so the slug reaches the
            // page body; for static routes it receives `None`.
            // The render stub uses the param to populate a
            // `data-route-param` attribute on the wrapping
            // `<div>`, so the URL slug is visible in the
            // response and the e2e tests can assert that a
            // real slug reached the page.
            let dynamic_params: Vec<String> = axum_path
                .split('/')
                .filter_map(|seg| seg.strip_prefix(':').map(|s| s.to_string()))
                .collect();
            if !dynamic_params.is_empty() {
                imports.push_str("use axum::extract::Path;\n");
            }
            // The render-call site is `render(None)` for static
            // routes and `render(Some(slug))` / `render(Some(a,
            // b))` for dynamic ones. axum's `Path<String>`
            // extractor hands us an owned `String`, so the
            // call site dereferences with `&`.
            let render_arg = match dynamic_params.len() {
                0 => "None".to_string(),
                1 => {
                    let p = &dynamic_params[0];
                    format!("Some(&{p})")
                }
                _ => {
                    let names = dynamic_params.join(", ");
                    format!("Some((&({names})))")
                }
            };
            let handler_sig = match dynamic_params.len() {
                0 => format!(
                    "async fn {safe_name}_handler() -> axum::response::Html<String> {{ let v = {safe_name}::render({render_arg}); axum::response::Html(v.to_html()) }}\n"
                ),
                1 => {
                    let p = &dynamic_params[0];
                    format!(
                        "async fn {safe_name}_handler(Path({p}): Path<String>) -> axum::response::Html<String> {{ let v = {safe_name}::render({render_arg}); axum::response::Html(v.to_html()) }}\n"
                    )
                }
                _ => {
                    // Tuple destructuring. axum will parse
                    // the path as a tuple. The destructure
                    // pattern needs an extra pair of parens
                    // (e.g. `Path((a, b))`) because `Path(a, b)`
                    // would be two extractor arguments, not
                    // a tuple extractor.
                    let names = dynamic_params.join(", ");
                    let types = (0..dynamic_params.len())
                        .map(|_| "String".to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let bind = format!("(({names}))");
                    format!(
                        "async fn {safe_name}_handler(Path{bind}: Path<({types})>) -> axum::response::Html<String> {{ let v = {safe_name}::render({render_arg}); axum::response::Html(v.to_html()) }}\n"
                    )
                }
            };
            handlers.push_str(&handler_sig);

            // If the source file declared per-method handlers,
            // emit one axum route per method. Otherwise fall back
            // to a single GET that renders the page.
            let route_file = route.file_path.as_str();
            let route_methods: Vec<String> = module_by_path
                .get(route_file)
                .copied()
                .or_else(|| {
                    module_by_path
                        .values()
                        .find(|m| {
                            m.source_path
                                .as_deref()
                                .map(|p| p.contains(route_file) || route_file.contains(p))
                                .unwrap_or(false)
                        })
                        .copied()
                })
                .and_then(|m| m.items_json.as_ref())
                .map(Self::extract_handler_methods)
                .unwrap_or_default();

            if route_methods.is_empty() {
                router_calls.push_str(&format!(
                    "        .route(\"{}\", axum::routing::get({}_handler))\n",
                    axum_path, safe_name
                ));
            } else {
                for method in &route_methods {
                    let axum_name = match method.as_str() {
                        "GET" => "get",
                        "POST" => "post",
                        "PUT" => "put",
                        "DELETE" => "delete",
                        "PATCH" => "patch",
                        "HEAD" => "head",
                        "OPTIONS" => "options",
                        _ => "get",
                    };
                    router_calls.push_str(&format!(
                        "        .route(\"{}\", axum::routing::{}({}_handler))\n",
                        axum_path, axum_name, safe_name
                    ));
                }
            }
        }

        // Collect middleware modules (any module whose source path
        // looks like a Fresh middleware file). Layer them in
        // order so the first middleware in the project tree
        // runs first.
        //
        // The per-file codegen writes the generated `.rs` file
        // to `src_dir.join(file_path).with_extension("rs")` where
        // `file_path` is the route-stripped, extension-stripped
        // path (see `run_plugin_build` in `src/commands/build/mod.rs`).
        // For `routes/_middleware.ts` that path becomes
        // `src_dir/_middleware.rs`, so the `mod` declaration must
        // use the leaf-name `_middleware`, not the full source
        // path. The middleware function name inside the file is
        // also derived from the leaf so we just use that name
        // twice (for both `mod` and the function reference).
        let mut middleware_names: Vec<String> = Vec::new();
        for m in modules {
            let sp = match &m.source_path {
                Some(s) => s,
                None => continue,
            };
            if !Self::is_middleware_path(sp) {
                continue;
            }
            // Skip the layout file (`_layout.tsx`); that's not
            // a middleware, it's a wrapper component.
            if sp.ends_with("_layout.ts") || sp.ends_with("_layout.tsx") {
                continue;
            }
            // The leaf (after stripping the `routes/` prefix and
            // the extension) is the module name the per-file
            // codegen wrote to disk.
            let leaf = sp
                .rsplit_once('/')
                .map(|(_, l)| l)
                .unwrap_or(sp)
                .replace(".ts", "")
                .replace(".tsx", "");
            middleware_names.push(leaf);
        }
        middleware_names.sort();
        middleware_names.dedup();

        // Emit the per-middleware mod declaration and a
        // `.layer(axum::middleware::from_fn(mw_fn))` call.
        for mw_name in &middleware_names {
            imports.push_str(&format!("mod {mw_name};\n"));
            router_calls.push_str(&format!(
                "        .layer(axum::middleware::from_fn({mw_name}::{mw_name}))\n"
            ));
        }

        (imports, handlers, router_calls)
    }

    fn format_main_rs(
        &self,
        imports: &str,
        handlers: &str,
        router_calls: &str,
    ) -> Result<String, PluginError> {
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

    /// Walk the HIR items JSON for a module and return the HTTP
    /// method names declared in its `export const handler = { ... }`
    /// object. The plugin sees the HIR as opaque JSON and parses
    /// only what it needs (the property keys).
    ///
    /// Two serialisations are accepted:
    /// - `ObjectProp::Method { key, value: Function }` — the
    ///   method-shorthand case (`{ GET(req) { ... } }`).
    /// - `ObjectProp::Init { key, value: Function }` — the
    ///   explicit-property case (`{ GET: async (req) => ... }` or
    ///   `{ async GET(req) { ... }` after oxc's HIR pass).
    ///
    /// Both shapes have `key: PropKey::Str("GET")` and a function
    /// value, so we treat them uniformly here.
    fn extract_handler_methods(items: &serde_json::Value) -> Vec<String> {
        let mut out = Vec::new();
        let arr = match items.as_array() {
            Some(a) => a,
            None => return out,
        };
        for item in arr {
            // Externally tagged: ModuleItem is `{"Decl": <Decl>}`.
            // Decl is `{"Variable": <VariableDecl>}`.
            let decl = match item.get("Decl") {
                Some(d) => d,
                None => continue,
            };
            let variable = match decl.get("Variable") {
                Some(v) => v,
                None => continue,
            };
            let name = match variable.get("name").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => continue,
            };
            if name != "handler" {
                continue;
            }
            let init = match variable.get("init") {
                Some(i) => i,
                None => continue,
            };
            // Externally tagged Expr: `{"Object": { "members": [...] }}`.
            let obj = match init.get("Object") {
                Some(o) => o,
                None => continue,
            };
            let members = match obj.get("members").and_then(|m| m.as_array()) {
                Some(m) => m,
                None => continue,
            };
            for m in members {
                // Each member: `{"prop": <ObjectProp>}`. ObjectProp is
                // an externally-tagged enum: `{"Init": {...}}` or
                // `{"Method": {...}}`. Both have a `key` field.
                let prop = match m.get("prop") {
                    Some(p) => p,
                    None => continue,
                };
                // Look for the function value under either
                // `Method.value` or `Init.value`. We only care
                // about properties whose value is a `Function`
                // expression — that's how we know it's a handler
                // (vs. a config object whose values are objects or
                // strings).
                let value = prop
                    .get("Method")
                    .and_then(|m| m.get("value"))
                    .or_else(|| prop.get("Init").and_then(|i| i.get("value")));
                if value.is_none() {
                    continue;
                }
                let value = value.unwrap();
                if value.get("Function").is_none() {
                    // Property value is not a function (e.g. a
                    // string config like `description: "..."`).
                    continue;
                }
                let key = match prop.get("Method").and_then(|m| m.get("key")).or_else(|| {
                    prop.get("Init").and_then(|i| i.get("key"))
                }) {
                    Some(k) => k,
                    None => continue,
                };
                // PropKey: `{"Str": <String>}` for string keys.
                if let Some(s) = key.get("Str").and_then(|s| s.as_str()) {
                    let upper = s.to_uppercase();
                    if matches!(
                        upper.as_str(),
                        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
                    ) {
                        out.push(upper);
                    }
                }
            }
        }
        out
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
    fn try_codegen_jsx(
        &self,
        items: &serde_json::Value,
        _hir: &runts_plugin::hir::Module,
    ) -> Option<String> {
        // Parse items as array
        let items_arr = items.as_array()?;

        for item in items_arr {
            // ModuleItem's variants are newtype-tuple. With
            // `#[serde(tag = "kind")]`, serde doesn't support that combination
            // and instead silently serializes the *inner* struct's fields at
            // the top level. So a `ModuleItem::Decl(Decl::Function(f))` shows
            // up as the FunctionDecl's own fields (e.g. `{"kind": "Function",
            // "name": "...", ...}`), not as `{"Decl": {...}}`.
            //
            // A `ModuleItem::Import(i)` shows up as the Import struct's fields
            // (no `kind` tag because Import has no `kind` field). A
            // `ModuleItem::Stmt(s)` shows up as the Stmt variant's fields.
            //
            // We accept any of these shapes:
            //   - externally tagged Decl: `{"Decl": {"kind": "Function", ...}}`
            //   - flattened: `{"kind": "Function", "name": "...", ...}` or
            //     `{"name": "handler", "kind": "Const", ...}` (Decl::Variable)
            //   - Stmt-shaped: `{"kind": "Type", ...}`, `{"kind": "Expr", ...}`
            //
            // To find a function declaration, look at the value of the
            // "kind" key — if it's "Function", it's a Decl::Function (no
            // matter whether we're inside a `Decl` wrapper or not).
            let decl_value = if let Some(d) = item.get("Decl") {
                d.clone()
            } else {
                item.clone()
            };

            // Skip imports (no "Decl" key).
            let decl_value = match item.get("Decl") {
                Some(d) => d.clone(),
                None => continue,
            };

            // Skip non-function declarations. With the externally tagged
            // Decl, the variant name is a top-level key in decl_value:
            //   {"Function": {"name": "About", "body": ...}}
            //   {"Variable": {"name": "handler", "kind": "Const", ...}}
            //   {"Type": {"name": "AboutData", ...}}
            // We only want Function.
            let func_value = match decl_value.get("Function") {
                Some(f) => f.clone(),
                None => continue,
            };

            // Extract function name and body
            let name = func_value.get("name")?.as_str()?;
            let body = func_value.get("body")?;

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
        // Body structure varies by HIR encoding. With the externally
        // tagged `Block` (after the newtype-variant fix), `Option<Block>`
        // serializes as the inner `Vec<Stmt>` directly, so `body` is
        // either:
        //   - a bare array of stmts: `[ ... ]`     (current shape)
        //   - a `Block` object: `{"stmts": [...]}` (after Block becomes
        //     a struct variant, with `stmts` field)
        //   - an internally tagged wrapper: `{"Block": {"stmts": [...]}}`
        //   - a bare JSX expression
        let stmts_opt: Option<&Vec<serde_json::Value>> = body
            .as_array()
            .or_else(|| body.get("stmts").and_then(|v| v.as_array()))
            .or_else(|| {
                body.get("Block")
                    .and_then(|b| b.get("stmts"))
                    .and_then(|v| v.as_array())
            });

        if let Some(stmts) = stmts_opt {
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
    /// Accepts both the externally-tagged shape
    /// `{"Return": {"arg": ...}}` and the older internally-tagged
    /// shape `{"kind": "Return", "arg": ...}`.
    fn find_jsx_in_stmt(&self, stmt: &serde_json::Value) -> Option<serde_json::Value> {
        // For internally-tagged enums (the Stmt enum uses
        // `#[serde(tag = "kind")]`), "kind" is the discriminator and
        // there are also other fields alongside it. For
        // externally-tagged enums (the JSX/Cond enums), the variant
        // name is the top-level key. Check "kind" first, then fall
        // back to the first key.
        let obj = stmt.as_object()?;
        let (variant, inner) = if let Some(kind) = obj.get("kind").and_then(|v| v.as_str()) {
            (kind.to_string(), stmt.clone())
        } else {
            let (k, v) = obj.iter().next()?;
            (k.clone(), v.clone())
        };

        match variant.as_str() {
            "Return" => {
                let arg = inner.get("arg")?;
                if self.is_jsx_expr(arg) {
                    return Some(arg.clone());
                }
                return self.find_jsx_in_expr(arg);
            }
            "Expr" => {
                let expr = inner.get("expr")?;
                if self.is_jsx_expr(expr) {
                    return Some(expr.clone());
                }
                return self.find_jsx_in_expr(expr);
            }
            "Block" => {
                if let Some(stmts) = inner.get("stmts").and_then(|s| s.as_array()) {
                    for s in stmts {
                        if let Some(jsx) = self.find_jsx_in_stmt(s) {
                            return Some(jsx);
                        }
                    }
                }
            }
            "If" => {
                if let Some(cons) = inner.get("consequent") {
                    if let Some(jsx) = self.find_jsx_in_stmt(cons) {
                        return Some(jsx);
                    }
                }
                if let Some(alt) = inner.get("alternate") {
                    return self.find_jsx_in_stmt(alt);
                }
            }
            _ => {}
        }
        None
    }

    /// Find JSX in an expression. Accepts both the externally-tagged
    /// shape `{"JSX": {...}}` / `{"Cond": {...}}` and the older
    /// internally-tagged shape `{"kind": "JSX", ...}`.
    fn find_jsx_in_expr(&self, expr: &serde_json::Value) -> Option<serde_json::Value> {
        // Detect variant: externally tagged uses the variant name as the
        // top-level key; internally tagged uses "kind" + the value at
        // the same level.
        let obj = expr.as_object()?;
        let (variant, inner) = if obj.contains_key("kind") {
            let kind = obj.get("kind")?.as_str()?.to_string();
            (kind, expr.clone())
        } else {
            let (k, v) = obj.iter().next()?;
            (k.clone(), v.clone())
        };

        match variant.as_str() {
            "JSX" => {
                // For externally-tagged, the JSX fields are on `inner`.
                // For internally-tagged, the fields are on `expr`.
                if self.is_jsx_expr(&inner) {
                    return Some(inner.clone());
                }
                if self.is_jsx_expr(expr) {
                    return Some(expr.clone());
                }
                None
            }
            "Cond" => {
                let consequent = inner.get("consequent")?;
                if let Some(jsx) = self.find_jsx_in_expr(consequent) {
                    return Some(jsx);
                }
                if let Some(alt) = inner.get("alternate") {
                    return self.find_jsx_in_expr(alt);
                }
                None
            }
            _ => None,
        }
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
    fn jsx_expr_to_tokenstream(&self, expr: &serde_json::Value) -> Option<Option<TokenStream>> {
        if let Some(name) = expr.get("Ident")?.as_str() { return Some(Some(jsx_expr(quote::quote! { #name }))); }
        if let Some(s) = expr.get("String")?.as_str() { return Some(Some(jsx_text(s))); }
        if let Some(n) = expr.get("Number")?.as_f64() { return Some(Some(jsx_expr(quote::quote! { #n }))); }
        if let Some(b) = expr.get("Boolean")?.as_bool() { return Some(Some(jsx_expr(quote::quote! { #b }))); }
        if let Some(kind) = expr.get("kind")?.as_str() { match kind { "Ident" => { let name = expr.get("name")?.as_str()?; return Some(Some(jsx_expr(quote::quote! { #name }))); } "String" => { return Some(Some(jsx_text(expr.get("String")?.as_str()?))); } "Number" => { let n = expr.get("0")?.as_f64()?; return Some(Some(jsx_expr(quote::quote! { #n }))); } "Boolean" => { let b = expr.get("0")?.as_bool()?; return Some(Some(jsx_expr(quote::quote! { #b }))); } _ => return Some(None), } }
        Some(None)
    }

    /// Wrap page component code in a module.
    fn wrap_page_module(&self, name: &str, page_fn: &str) -> String {
        format!(
            r#"//! Page component: {name}
//! Generated by runts-fresh 0.1

use runts_lib::runtime::vdom::VNode;

{page_fn}

pub fn render(route_param: Option<&str>) -> VNode {{
    let inner = {name}();
    if let Some(p) = route_param {{
        // Dynamic route — wrap the inner JSX in a div
        // that shows the slug. e2e tests assert on this
        // marker.
        VNode::element("div")
            .attr("data-route-param", p)
            .child(inner)
    }} else {{
        // Static route.
        inner
    }}
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
        let items_json = serde_json::json!([{"Decl": {"Function": {"name": "Hello", "body": {"stmts": [{"Return": {"arg": {"JSX": {"opening": {"name": { "Ident": "div" }, "attrs": [], "self_closing": false}, "children": [{"Text": "Hello World"}], "closing": {"name": { "Ident": "div" }}}}}}]}}}}}]);
        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);
        assert!(result.is_some(), "Should generate code for JSX");
        let code = result.unwrap();
        eprintln!("Generated code:\n{}", code);
        assert!(code.contains("VNode::Element") || code.contains("VNode"), "Should contain VNode");
        assert!(code.contains("\"div\""), "Should contain div tag");
        assert!(code.contains("Hello World"), "Should contain text");
    }

    #[test]
    fn test_try_codegen_jsx_with_attributes() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([{"Decl": {"Function": {"name": "Home", "body": {"stmts": [{"Return": {"arg": {"JSX": {"opening": {"name": { "Ident": "div" }, "attrs": [{"Attr": {"name": "class", "value": "home"}}], "self_closing": false}, "children": [{"Text": "Welcome"}], "closing": {"name": { "Ident": "div" }}}}}}]}}}}}]);
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
    #[test]
    fn test_try_codegen_jsx_nested_elements() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([{"Decl": {"Function": {"name": "Outer", "body": {"stmts": [{"Return": {"arg": {"JSX": {"opening": {"name": { "Ident": "div" }, "attrs": [], "self_closing": false}, "children": [{"JSX": {"opening": {"name": { "Ident": "span" }, "attrs": [], "self_closing": false}, "children": [{"Text": "nested"}], "closing": {"name": { "Ident": "span" }}}}], "closing": {"name": { "Ident": "div" }}}}}}]}}}}}]);
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
        let items_json = serde_json::json!([{"Decl": {"Function": {"name": "WithExpr", "body": {"stmts": [{"Return": {"arg": {"JSX": {"opening": {"name": { "Ident": "div" }, "attrs": [], "self_closing": false}, "children": [{"Expr": {"expr": {"Ident": { "name": "name" }}}}], "closing": {"name": { "Ident": "div" }}}}}}]}}}}}]);
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
    fn test_extract_handler_methods_picks_verbs() {
        let items = serde_json::json!([
            {
                "Decl": {
                    "Variable": {
                        "name": "handler",
                        "init": {
                            "Object": {
                                "members": [
                                    {"prop": {"Method": {"key": {"Str": "GET"}, "value": {"Function": {}}}}},
                                    {"prop": {"Method": {"key": {"Str": "POST"}, "value": {"Function": {}}}}}
                                ]
                            }
                        }
                    }
                }
            }
        ]);
        let methods = FreshPlugin::extract_handler_methods(&items);
        assert_eq!(methods, vec!["GET".to_string(), "POST".to_string()]);
    }

    #[test]
    fn test_extract_handler_methods_ignores_non_verbs() {
        let items = serde_json::json!([
            {
                "Decl": {
                    "Variable": {
                        "name": "handler",
                        "init": {
                            "Object": {
                                "members": [
                                    {"prop": {"Method": {"key": {"Str": "config"}, "value": {"Function": {}}}}},
                                    {"prop": {"Init": {"key": {"Str": "data"}, "value": {"String": "x"}}}}
                                ]
                            }
                        }
                    }
                }
            }
        ]);
        let methods = FreshPlugin::extract_handler_methods(&items);
        assert!(methods.is_empty(), "non-HTTP keys should be ignored, got {:?}", methods);
    }

    #[test]
    fn test_is_middleware_path_recognises_underscore_route() {
        assert!(FreshPlugin::is_middleware_path("routes/_middleware.ts"));
        assert!(FreshPlugin::is_middleware_path(
            "routes/blog/_middleware.ts"
        ));
        assert!(FreshPlugin::is_middleware_path(
            "routes/blog/_middleware.tsx"
        ));
        // File is a route, not a middleware.
        assert!(!FreshPlugin::is_middleware_path("routes/index.tsx"));
        assert!(!FreshPlugin::is_middleware_path("routes/blog/index.tsx"));
        // _layout is not a middleware.
        assert!(!FreshPlugin::is_middleware_path(
            "routes/blog/_layout.tsx"
        ));
        // Outside `routes/`.
        assert!(!FreshPlugin::is_middleware_path(
            "islands/_middleware.ts"
        ));
        assert!(!FreshPlugin::is_middleware_path("_middleware.ts"));
    }

    #[test]
    fn test_codegen_middleware_emits_axum_from_fn_signature() {
        let plugin = FreshPlugin;
        let hir = runts_plugin::hir::Module::new();
        let code = plugin
            .codegen_middleware_module("routes/_middleware.ts", &hir)
            .expect("codegen should succeed");
        // The middleware is wired via axum's `from_fn` adapter
        // and so must take a `Request` and a `Next`, return a
        // `Response`, and the function name must match the leaf
        // of the source path (so `mod _middleware;` in main.rs
        // resolves).
        assert!(code.contains("pub async fn _middleware("));
        assert!(code.contains("axum::extract::Request"));
        assert!(code.contains("axum::middleware::Next"));
        assert!(code.contains("axum::response::Response"));
        assert!(code.contains("x-response-time"));
    }

    #[test]
    fn test_codegen_middleware_uses_full_path_when_no_slash() {
        let plugin = FreshPlugin;
        let hir = runts_plugin::hir::Module::new();
        let code = plugin
            .codegen_middleware_module("_middleware.ts", &hir)
            .expect("codegen should succeed");
        // Without a slash, the whole path is the leaf (after
        // stripping the extension), so the function name should
        // still be `_middleware`.
        assert!(code.contains("pub async fn _middleware("));
    }

    #[test]
    fn test_render_module_emits_optional_route_param() {
        // The per-page `render` stub takes `Option<&str>`:
        // static routes pass `None`, dynamic routes pass
        // `Some(slug)`. The render surfaces the param so
        // e2e tests can assert a slug reached the page.
        use runts_plugin::RouteInfo;
        let route = RouteInfo {
            path: "/about".to_string(),
            methods: vec!["GET".to_string()],
            file_path: "about.tsx".to_string(),
        };
        let module = runts_plugin::hir::Module::new();
        let plugin = FreshPlugin;
        let code = plugin
            .codegen_route_module("about.tsx", Some(&route))
            .expect("codegen should succeed");
        assert!(
            code.contains("pub fn render(route_param: Option<&str>) -> VNode"),
            "static-path render should take Option<&str>, got: {code}"
        );
    }

    #[test]
    fn test_render_module_surfaces_slug_in_body_for_dynamic_routes() {
        // A dynamic route at `/blog/:slug` should produce
        // a `render` whose body includes a "route-param: "
        // prefix and a `<code>` block holding the slug. The
        // slug is the value, the prefix is the marker e2e
        // tests assert on.
        use runts_plugin::RouteInfo;
        let route = RouteInfo {
            path: "/blog/:slug".to_string(),
            methods: vec!["GET".to_string()],
            file_path: "blog/[slug].tsx".to_string(),
        };
        let module = runts_plugin::hir::Module::new();
        let plugin = FreshPlugin;
        let code = plugin
            .codegen_route_module("blog/[slug].tsx", Some(&route))
            .expect("codegen should succeed");
        assert!(
            code.contains("pub fn render(route_param: Option<&str>) -> VNode"),
            "dynamic-path render should take Option<&str>, got: {code}"
        );
        assert!(
            code.contains("\"route-param: \""),
            "dynamic-path render should include a visible 'route-param: ' label, got: {code}"
        );
        assert!(
            code.contains("VNode::element(\"code\").child(VNode::text(p))"),
            "dynamic-path render should wrap the slug in a <code> block, got: {code}"
        );
    }

    #[test]
    fn test_dynamic_route_emits_path_extractor_for_single_param() {
        // Build a synthetic module set with one route at
        // `/blog/:slug`. The plugin should emit a handler
        // whose signature uses axum's `Path(slug): Path<String>`
        // extractor and add a `use axum::extract::Path;` to
        // the generated main.rs.
        use runts_plugin::RouteInfo;
        let route = RouteInfo {
            path: "/blog/:slug".to_string(),
            methods: vec!["GET".to_string()],
            file_path: "blog/[slug].tsx".to_string(),
        };
        let module = runts_plugin::hir::Module::new();
        let plugin = FreshPlugin;
        let (imports, handlers, router_calls) =
            plugin.generate_route_code(&[&route], &[module]);
        assert!(
            handlers.contains("Path(slug): Path<String>"),
            "handler should use Path(slug): Path<String>, got: {handlers}"
        );
        assert!(
            imports.contains("use axum::extract::Path;"),
            "imports should include `use axum::extract::Path;`, got: {imports}"
        );
        assert!(
            router_calls.contains(".route(\"/blog/:slug\""),
            "router_calls should contain `/blog/:slug` route, got: {router_calls}"
        );
    }

    #[test]
    fn test_dynamic_route_no_extractor_for_static_path() {
        // A static path (`/about`) should NOT generate a Path
        // extractor — the handler takes no arguments and
        // there's no `use axum::extract::Path;` import.
        use runts_plugin::RouteInfo;
        let route = RouteInfo {
            path: "/about".to_string(),
            methods: vec!["GET".to_string()],
            file_path: "about.tsx".to_string(),
        };
        let module = runts_plugin::hir::Module::new();
        let plugin = FreshPlugin;
        let (imports, handlers, _router_calls) =
            plugin.generate_route_code(&[&route], &[module]);
        assert!(
            !imports.contains("use axum::extract::Path;"),
            "static path should not import axum::extract::Path, got: {imports}"
        );
        assert!(
            !handlers.contains("Path("),
            "static-path handler should not have a Path extractor, got: {handlers}"
        );
    }

    #[test]
    fn test_dynamic_route_emits_tuple_extractor_for_multiple_params() {
        // A route with two dynamic segments should emit a
        // tuple `Path((a, b)): Path<(String, String)>`.
        use runts_plugin::RouteInfo;
        let route = RouteInfo {
            path: "/u/:user/:post".to_string(),
            methods: vec!["GET".to_string()],
            file_path: "u/[user]/[post].tsx".to_string(),
        };
        let module = runts_plugin::hir::Module::new();
        let plugin = FreshPlugin;
        let (_imports, handlers, _router_calls) =
            plugin.generate_route_code(&[&route], &[module]);
        assert!(
            handlers.contains("Path((user, post)): Path<(String, String)>"),
            "two-param handler should use tuple Path extractor, got: {handlers}"
        );
    }

    #[test]
    fn test_extract_handler_methods_picks_init_with_function() {
        // Real HIR shape from `export const handler = {
        //   async GET(req: Request, ctx: HandlerContext): Promise<Response> {...}
        // }` — oxc serialises this method-shorthand as
        // `ObjectProp::Init` with a `Function` value, NOT as
        // `ObjectProp::Method`. This test pins down that
        // behaviour so the next refactor doesn't regress it.
        let items = serde_json::json!([
            {
                "Decl": {
                    "Variable": {
                        "name": "handler",
                        "init": {
                            "Object": {
                                "members": [
                                    {"prop": {"Init": {
                                        "computed": false,
                                        "key": {"Str": "GET"},
                                        "value": {"Function": {
                                            "is_async": true,
                                            "body": [],
                                            "params": []
                                        }}
                                    }}}
                                ]
                            }
                        }
                    }
                }
            }
        ]);
        let methods = FreshPlugin::extract_handler_methods(&items);
        assert_eq!(methods, vec!["GET".to_string()]);
    }

    #[test]
    fn test_extract_handler_methods_lowercases_and_filters() {
        // Mixed-case key. We uppercase, then check the allowlist.
        let items = serde_json::json!([
            {
                "Decl": {
                    "Variable": {
                        "name": "handler",
                        "init": {
                            "Object": {
                                "members": [
                                    {"prop": {"Method": {"key": {"Str": "delete"}, "value": {"Function": {}}}}}
                                ]
                            }
                        }
                    }
                }
            }
        ]);
        let methods = FreshPlugin::extract_handler_methods(&items);
        assert_eq!(methods, vec!["DELETE".to_string()]);
    }

    #[test]
    fn test_extract_handler_methods_skips_non_handler() {
        // Variable named "config", not "handler".
        let items = serde_json::json!([
            {
                "Decl": {
                    "Variable": {
                        "name": "config",
                        "init": {
                            "Object": {
                                "members": [
                                    {"prop": {"Method": {"key": {"Str": "GET"}, "value": {"Function": {}}}}}
                                ]
                            }
                        }
                    }
                }
            }
        ]);
        let methods = FreshPlugin::extract_handler_methods(&items);
        assert!(methods.is_empty());
    }

    #[test]
    fn test_try_codegen_jsx_with_internally_tagged_stmt() {
        let plugin = FreshPlugin;
        let items_json = serde_json::json!([{"Decl": {"Function": {"name": "Counter", "body": [{"kind": "Return", "arg": {"JSX": {"opening": {"name": { "Ident": "div" }, "attrs": [], "self_closing": false}, "children": [{"Text": "Count: 0"}], "closing": {"name": { "Ident": "div" }}}}}]}}}]]);
        let hir = runts_plugin::hir::Module::new();
        let result = plugin.try_codegen_jsx(&items_json, &hir);
        assert!(result.is_some(), "Should generate code for JSX inside a Stmt::Return (the actual HIR shape)");
        let code = result.unwrap();
        assert!(code.contains("VNode"), "Should contain VNode");
        assert!(code.contains("div"), "Should contain div tag");
        assert!(code.contains("render"), "Wrapped module should expose a render function");
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
