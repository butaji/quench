//! Production build command
//!
//! Transpiles all TS/TSX files and generates:
//! - Route table
//! - Islands manifest
//! - Rust source files
//! - Compiles to native binary (via cargo)

use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use walkdir::WalkDir;
use regex::Regex;
use tracing::{info, error};

use crate::config::Config;
use crate::transpile::{Transpiler, hir::Module, codegen::CodeGenerator, analyzer::Analyzer};

/// Build result
pub struct BuildResult {
    /// Generated Rust source files
    pub generated_files: Vec<GeneratedFile>,
    
    /// Route manifest
    pub routes: Vec<RouteEntry>,
    
    /// Islands manifest
    pub islands: Vec<IslandEntry>,
    
    /// Components manifest
    pub components: Vec<ComponentEntry>,
}

/// A generated source file
#[derive(Debug)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
}

/// Route entry
#[derive(Debug, Clone)]
pub struct RouteEntry {
    pub pattern: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub file: String,
    pub params: Vec<String>,
    pub methods: Vec<HttpMethod>,
}

/// HTTP method
#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

impl Default for HttpMethod {
    fn default() -> Self {
        HttpMethod::GET
    }
}

/// Island entry
#[derive(Debug, Clone)]
pub struct IslandEntry {
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub file: String,
    pub props_type: Option<String>,
}

/// Component entry
#[derive(Debug, Clone)]
pub struct ComponentEntry {
    pub name: String,
    pub path: PathBuf,
    pub file: String,
}

/// Run production build
pub async fn run_build(config: &Config, path: PathBuf) -> Result<BuildResult> {
    info!("Starting production build...");

    // Ensure we're in a project directory
    let project_root = find_project_root(&path)?;
    info!("Project root: {:?}", project_root);

    // Create transpiler and code generator
    let mut transpiler = Transpiler::new(config);
    let mut code_gen = CodeGenerator::new();

    // Collect files to transpile
    let mut routes = Vec::new();
    let mut islands = Vec::new();
    let mut components = Vec::new();
    let mut generated_files = Vec::new();

    // Process routes directory
    let routes_dir = project_root.join("routes");
    if routes_dir.exists() {
        info!("Processing routes...");
        process_routes_dir(
            &routes_dir, 
            &routes_dir, 
            &mut routes, 
            &mut transpiler, 
            &mut code_gen, 
            &mut generated_files
        )?;
    }

    // Process islands directory
    let islands_dir = project_root.join("islands");
    if islands_dir.exists() {
        info!("Processing islands...");
        process_islands_dir(
            &islands_dir, 
            &islands_dir, 
            &mut islands, 
            &mut transpiler, 
            &mut code_gen, 
            &mut generated_files
        )?;
    }

    // Process components directory
    let components_dir = project_root.join("components");
    if components_dir.exists() {
        info!("Processing components...");
        process_components_dir(
            &components_dir, 
            &mut transpiler, 
            &mut code_gen, 
            &mut generated_files,
            &mut components
        )?;
    }

    // Generate route table
    let route_table = generate_route_table(&routes);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/routes.rs"),
        content: route_table,
    });

    // Generate islands manifest
    let islands_manifest = generate_islands_manifest(&islands);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/islands.rs"),
        content: islands_manifest,
    });

    // Generate components module
    let components_module = generate_components_module(&components);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/components.rs"),
        content: components_module,
    });

    // Generate lib.rs
    let lib_content = generate_lib(&routes, &islands, &components);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/lib.rs"),
        content: lib_content,
    });

    // Generate main.rs if it doesn't exist
    let main_path = project_root.join("src/main.rs");
    if !main_path.exists() {
        let main_content = generate_main(&project_root);
        generated_files.push(GeneratedFile {
            path: main_path,
            content: main_content,
        });
    }

    // Write generated files
    for file in &generated_files {
        if let Some(parent) = file.path.parent() {
            fs::create_dir_all(parent).context("Failed to create directory")?;
        }
        fs::write(&file.path, &file.content).context("Failed to write file")?;
        info!("Generated: {:?}", file.path);
    }

    // Generate mod.rs files for all gen/ subdirectories
    let file_tuples: Vec<(PathBuf, String)> = generated_files.iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect();
    generate_mod_files(&project_root, &file_tuples);

    info!("Build complete! Generated {} files", generated_files.len());
    info!("  Routes: {}", routes.len());
    info!("  Islands: {}", islands.len());
    info!("  Components: {}", components.len());

    Ok(BuildResult {
        generated_files,
        routes,
        islands,
        components,
    })
}

/// Find project root
fn find_project_root(path: &PathBuf) -> Result<PathBuf> {
    let mut current = path.clone();
    
    // If it's a file, get the directory
    if current.is_file() {
        current = current.parent().unwrap_or(&current).to_path_buf();
    }
    
    loop {
        if current.join("Cargo.toml").exists() 
            || current.join("runts.config.json").exists() 
            || current.join("runts.config.ts").exists()
        {
            return Ok(current);
        }
        
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            anyhow::bail!("Not a runts project (no Cargo.toml or runts.config.* found)");
        }
    }
}

/// Process routes directory
fn process_routes_dir(
    dir: &PathBuf,
    base: &PathBuf,
    routes: &mut Vec<RouteEntry>,
    transpiler: &mut Transpiler,
    code_gen: &mut CodeGenerator,
    generated_files: &mut Vec<GeneratedFile>,
) -> Result<()> {
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path().to_path_buf();
        
        if path.is_dir() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "ts" && ext != "tsx" {
            continue;
        }

        // Skip special files: _app.tsx, _layout.tsx, _middleware.ts, _404.tsx, _500.tsx
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if filename.starts_with('_') {
            continue;
        }

        // Parse and analyze
        let module = match transpiler.parse_file(&path) {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to parse {:?}: {}", path, e);
                continue;
            }
        };

        let mut analyzer = Analyzer::new();
        if let Err(errors) = analyzer.analyze(&module) {
            for err in &errors {
                error!("Analysis error: {:?}", err);
            }
            continue;
        }

        // Generate Rust code (routes need handlers, layouts don't)
        let is_layout = filename == "_layout.tsx" || filename == "_layout.ts";
        code_gen.set_generate_handlers(!is_layout);
        let rust_code = match code_gen.generate_module(&module) {
            Ok(code) => code,
            Err(e) => {
                error!("Code generation failed for {:?}: {}", path, e);
                continue;
            }
        };

        // Extract route pattern
        let relative = path.strip_prefix(base).unwrap_or(&path);
        let pattern = extract_route_pattern(relative);
        let params = extract_params(&pattern);
        let methods = extract_http_methods(&module);

        routes.push(RouteEntry {
            pattern: pattern.clone(),
            path: path.clone(),
            file: relative.to_string_lossy().to_string(),
            params,
            methods,
        });

        // Generate output path (sanitize module names)
        let relative_without_ext = relative.with_extension("");
        let sanitized: PathBuf = relative_without_ext
            .components()
            .map(|c| {
                let s = c.as_os_str().to_string_lossy().to_string();
                sanitize_module_name(&s)
            })
            .collect();
        let output_path = base
            .parent()
            .unwrap_or(base)
            .join("src")
            .join("gen")
            .join(sanitized)
            .with_extension("rs");

        generated_files.push(GeneratedFile {
            path: output_path,
            content: rust_code,
        });
    }

    Ok(())
}

/// Process islands directory
fn process_islands_dir(
    dir: &PathBuf,
    base: &PathBuf,
    islands: &mut Vec<IslandEntry>,
    transpiler: &mut Transpiler,
    code_gen: &mut CodeGenerator,
    generated_files: &mut Vec<GeneratedFile>,
) -> Result<()> {
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path().to_path_buf();
        
        if path.is_dir() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "ts" && ext != "tsx" {
            continue;
        }

        let module = match transpiler.parse_file(&path) {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to parse {:?}: {}", path, e);
                continue;
            }
        };

        // Get island name from filename
        let name = path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Islands don't need route handlers
        code_gen.set_generate_handlers(false);
        let rust_code = match code_gen.generate_module(&module) {
            Ok(code) => code,
            Err(e) => {
                error!("Code generation failed for {:?}: {}", path, e);
                continue;
            }
        };

        let relative = path.strip_prefix(base).unwrap_or(&path);

        islands.push(IslandEntry {
            name: name.clone(),
            path: path.clone(),
            file: relative.to_string_lossy().to_string(),
            props_type: None,
        });

        // Generate output path
        let output_path = base
            .parent()
            .unwrap_or(base)
            .join("src")
            .join("gen")
            .join("islands")
            .join(format!("{}.rs", to_snake_case(&name)));

        generated_files.push(GeneratedFile {
            path: output_path,
            content: rust_code,
        });
    }

    Ok(())
}

/// Process components directory
fn process_components_dir(
    dir: &PathBuf,
    transpiler: &mut Transpiler,
    code_gen: &mut CodeGenerator,
    generated_files: &mut Vec<GeneratedFile>,
    components: &mut Vec<ComponentEntry>,
) -> Result<()> {
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path().to_path_buf();
        
        if path.is_dir() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "ts" && ext != "tsx" {
            continue;
        }

        let module = match transpiler.parse_file(&path) {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to parse {:?}: {}", path, e);
                continue;
            }
        };

        // Components don't need route handlers
        code_gen.set_generate_handlers(false);
        let rust_code = match code_gen.generate_module(&module) {
            Ok(code) => code,
            Err(e) => {
                error!("Code generation failed for {:?}: {}", path, e);
                continue;
            }
        };

        let name = path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let output_path = dir
            .parent()
            .unwrap_or(dir)
            .join("src")
            .join("gen")
            .join("components")
            .join(format!("{}.rs", to_snake_case(&name)));

        components.push(ComponentEntry {
            name,
            path: path.clone(),
            file: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
        });

        generated_files.push(GeneratedFile {
            path: output_path,
            content: rust_code,
        });
    }

    Ok(())
}

/// Extract route pattern from file path
fn extract_route_pattern(relative: &Path) -> String {
    let parts: Vec<String> = relative.iter()
        .filter_map(|p| p.to_str())
        .filter(|p| !["routes", "islands", "components"].contains(p))
        .map(|p| {
            if p.starts_with('[') && (p.ends_with(".tsx") || p.ends_with(".ts")) {
                let end = if p.ends_with(".tsx") { 5 } else { 3 };
                format!(":{}", &p[1..p.len() - end])
            } else if p.ends_with(".tsx") {
                p[..p.len() - 4].to_string()
            } else if p.ends_with(".ts") {
                p[..p.len() - 3].to_string()
            } else {
                p.to_string()
            }
        })
        .collect();

    let pattern = if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    };

    if pattern.ends_with("/index") {
        let stripped = &pattern[..pattern.len() - 6];
        if stripped.is_empty() { "/".to_string() } else { stripped.to_string() }
    } else {
        pattern
    }
}

/// Extract parameter names from route pattern
fn extract_params(pattern: &str) -> Vec<String> {
    let re = Regex::new(r":(\w+)").unwrap();
    re.captures_iter(pattern)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// Extract HTTP methods from route handlers
fn extract_http_methods(module: &Module) -> Vec<HttpMethod> {
    let mut methods = vec![HttpMethod::GET]; // Default to GET
    
    // Look for handler exports
    for item in &module.items {
        if let crate::transpile::hir::ModuleItem::Export(export) = item {
            match export {
                crate::transpile::hir::Export::NamedWithValue { name, .. } => {
                    if name == "handler" {
                        // Handler object with GET/POST/etc methods
                        // For now, assume GET
                    }
                }
                crate::transpile::hir::Export::Named { name } => {
                    let name_lower = name.to_lowercase();
                    if name_lower.contains("get") && !name_lower.contains("post") {
                        if !methods.contains(&HttpMethod::GET) {
                            methods.push(HttpMethod::GET);
                        }
                    }
                    if name_lower.contains("post") {
                        if !methods.contains(&HttpMethod::POST) {
                            methods.push(HttpMethod::POST);
                        }
                    }
                    if name_lower.contains("put") {
                        if !methods.contains(&HttpMethod::PUT) {
                            methods.push(HttpMethod::PUT);
                        }
                    }
                    if name_lower.contains("delete") {
                        if !methods.contains(&HttpMethod::DELETE) {
                            methods.push(HttpMethod::DELETE);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    
    methods
}

/// Generate route table
/// Compute `crate::gen::...` module path from a route file like `blog/[slug].tsx`.
fn route_module_path(file: &str) -> String {
    let stem = Path::new(file)
        .with_extension("")
        .to_string_lossy()
        .to_string();
    let parts: Vec<String> = stem
        .split('/')
        .map(sanitize_module_name)
        .collect();
    format!("crate::gen::{}", parts.join("::"))
}

fn generate_route_table(routes: &[RouteEntry]) -> String {
    let mut code = String::from("// Generated by runts\n");
    code.push_str("//! Route table for the application\n\n");
    code.push_str("use axum::{Router, routing::{get, post, put, delete}};\n\n");

    // Import each route module
    let mut imports: Vec<String> = Vec::new();
    for route in routes {
        let path = route_module_path(&route.file);
        let alias = to_snake_case(
            &route.file
                .replace(".tsx", "")
                .replace(".ts", "")
                .replace('/', "_")
                .replace('[', "")
                .replace(']', "")
        );
        imports.push(format!("use {} as {};", path, alias));
    }
    imports.sort();
    imports.dedup();
    for imp in imports {
        code.push_str(&imp);
        code.push('\n');
    }
    code.push('\n');

    // Generate route handlers
    code.push_str("/// Build Axum router from routes\n");
    code.push_str("pub fn build_router() -> Router {\n");
    code.push_str("    let mut router = Router::new();\n\n");

    for route in routes {
        let alias = to_snake_case(
            &route.file
                .replace(".tsx", "")
                .replace(".ts", "")
                .replace('/', "_")
                .replace('[', "")
                .replace(']', "")
        );

        code.push_str(&format!(
            "    // {} ({})\n",
            route.pattern,
            route.file
        ));

        for method in &route.methods {
            let method_name = match method {
                HttpMethod::GET => "get",
                HttpMethod::POST => "post",
                HttpMethod::PUT => "put",
                HttpMethod::DELETE => "delete",
                _ => "get",
            };

            code.push_str(&format!(
                "    router = router.route(\"{}\", {}({}::handle_{}));\n",
                route.pattern, method_name, alias, method_name
            ));
        }
        if route.methods.is_empty() {
            code.push_str(&format!(
                "    router = router.route(\"{}\", get({}::handle_get));\n",
                route.pattern, alias
            ));
        }
        code.push('\n');
    }

    code.push_str("    router\n");
    code.push_str("}\n");

    code
}

/// Generate islands manifest
fn generate_islands_manifest(islands: &[IslandEntry]) -> String {
    let mut code = String::from("// Generated by runts\n");
    code.push_str("//! Islands manifest\n\n");
    code.push_str("use serde::{Serialize, Deserialize};\n\n");

    code.push_str("/// Island entry\n");
    code.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    code.push_str("pub struct Island {\n");
    code.push_str("    pub name: &'static str,\n");
    code.push_str("    pub file: &'static str,\n");
    code.push_str("    pub props_type: Option<&'static str>,\n");
    code.push_str("}\n\n");

    code.push_str("/// All islands\n");
    code.push_str("pub fn islands() -> Vec<Island> {\n");
    code.push_str("    vec![\n");

    for island in islands {
        code.push_str(&format!(
            "        Island {{\n            name: \"{}\",\n            file: \"{}\",\n            props_type: {},\n        }},\n",
            island.name,
            island.file,
            island.props_type.as_ref()
                .map(|t| format!("Some(\"{}\")", t))
                .unwrap_or_else(|| "None".to_string())
        ));
    }

    code.push_str("    ]\n");
    code.push_str("}\n\n");

    code.push_str("/// Re-export generated island components\n");
    for island in islands {
        let module_name = to_snake_case(&island.name);
        code.push_str(&format!(
            "pub use crate::gen::islands::{};\n",
            module_name
        ));
    }
    code.push('\n');

    code.push_str("/// Island renderer\n");
    code.push_str("pub async fn render_island(name: &str, _props: serde_json::Value) -> String {\n");
    code.push_str("    format!(\"<div data-island=\\\"{}\\\">\\n</div>\", name)\n");
    code.push_str("}\n");

    code
}

/// Generate components module
fn generate_components_module(components: &[ComponentEntry]) -> String {
    let mut code = String::from("// Generated by runts\n");
    code.push_str("//! Server components\n\n");
    code.push_str("// Re-export all generated components from crate::gen::components\n");
    for component in components {
        let module_name = to_snake_case(&component.name);
        code.push_str(&format!(
            "pub use crate::gen::components::{};\n",
            module_name
        ));
    }
    code
}

/// Generate lib.rs
fn generate_lib(_routes: &[RouteEntry], _islands: &[IslandEntry], _components: &[ComponentEntry]) -> String {
    let mut code = String::from("// Generated by runts\n");
    code.push_str("//! Application library\n\n");

    code.push_str("pub mod gen;\n");
    code.push_str("pub mod routes;\n");
    code.push_str("pub mod islands;\n");
    code.push_str("pub mod components;\n\n");

    code.push_str("// Re-export commonly used items\n");
    code.push_str("pub use routes::build_router;\n");
    code.push_str("pub use islands::{islands, Island};\n\n");

    code.push_str("// Re-export runtime\n");
    code.push_str("pub use runts_lib::runtime::prelude::*;\n");

    code
}

/// Generate main.rs
fn generate_main(project_root: &Path) -> String {
    let app_name = project_root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app");

    format!(r#"//! {app_name} - Generated by runts

use std::net::SocketAddr;
use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;

#[tokio::main]
async fn main() {{
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "{app_name}".into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Build router
    let app = {app_name}::build_router()
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    tracing::info!("Listening on http://{{}}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await
        .expect("Failed to bind to port");
    
    axum::serve(listener, app).await
        .expect("Server error");
}}
"#)
}

/// Convert PascalCase to snake_case
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Sanitize a file stem into a valid Rust module name.
fn sanitize_module_name(stem: &str) -> String {
    let mut s = stem
        .replace('[', "")
        .replace(']', "")
        .replace('-', "_")
        .replace('.', "_");
    // Ensure it starts with a letter or underscore
    if let Some(first) = s.chars().next() {
        if first.is_ascii_digit() {
            s = format!("_{}", s);
        }
    }
    to_snake_case(&s)
}

/// Compute the Rust module path for a generated file.
/// `src/gen/blog/slug.rs` → `crate::gen::blog::slug`
fn module_path_from_gen_file(path: &Path) -> String {
    let components: Vec<String> = path
        .components()
        .skip_while(|c| c.as_os_str() != "gen")
        .skip(1) // skip "gen"
        .map(|c| {
            let name = c.as_os_str().to_string_lossy().to_string();
            if name.ends_with(".rs") {
                sanitize_module_name(&name[..name.len()-3])
            } else {
                sanitize_module_name(&name)
            }
        })
        .collect();
    format!("crate::gen::{}", components.join("::"))
}

/// Collect all generated file paths and emit `mod.rs` files for every directory.
fn generate_mod_files(project_root: &Path, generated: &[(PathBuf, String)]) {
    use std::collections::HashSet;

    let gen_root = project_root.join("src").join("gen");
    let mut dirs: HashSet<PathBuf> = HashSet::new();

    for (path, _) in generated {
        if let Some(parent) = path.parent() {
            if parent.starts_with(&gen_root) {
                dirs.insert(parent.to_path_buf());
            }
        }
    }

    for dir in &dirs {
        let mut mods = Vec::new();
        // Direct child .rs files (excluding mod.rs itself)
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                        if name != "mod" {
                            mods.push(sanitize_module_name(name));
                        }
                    }
                }
                // Child directories that contain .rs files
                if path.is_dir() {
                    let has_rs = WalkDir::new(&path)
                        .max_depth(1)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .any(|e| {
                            let p = e.path();
                            p.is_file() && p.extension().map(|e| e == "rs").unwrap_or(false)
                        });
                    if has_rs {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            mods.push(sanitize_module_name(name));
                        }
                    }
                }
            }
        }
        mods.sort();
        mods.dedup();

        let content = mods.into_iter()
            .map(|m| format!("pub mod {};\n", m))
            .collect::<String>();
        let mod_path = dir.join("mod.rs");
        if let Err(e) = std::fs::write(&mod_path, content) {
            eprintln!("Warning: failed to write {:?}: {}", mod_path, e);
        }
    }
}

// =============================================================================
// Rust Compilation
// =============================================================================

/// Compile the generated Rust code using cargo
pub fn compile_rust(project_root: &Path, release: bool) -> Result<()> {
    info!("Compiling Rust code...");
    
    let mut cmd = Command::new("cargo");
    cmd.current_dir(project_root);
    
    if release {
        cmd.arg("build").arg("--release");
    } else {
        cmd.arg("build");
    }
    
    info!("Running: {:?}", cmd);
    
    let output = cmd.output().context("Failed to execute cargo")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        error!("Cargo build failed!");
        if !stdout.is_empty() {
            error!("stdout: {}", stdout);
        }
        if !stderr.is_empty() {
            error!("stderr: {}", stderr);
        }
        
        anyhow::bail!("cargo build failed with exit code: {:?}", output.status.code());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        info!("Compilation output:\n{}", stdout);
    }
    
    info!("Rust compilation complete!");
    Ok(())
}

/// Build result with compilation info
pub struct CompilationResult {
    pub binary_path: Option<PathBuf>,
    pub binary_size: Option<u64>,
}

/// Full build: transpile + compile
pub async fn run_full_build(config: &Config, path: PathBuf, release: bool) -> Result<CompilationResult> {
    // Phase 1: Transpile (returns manifest info we log for debugging)
    let _build_result = run_build(config, path.clone()).await?;
    info!("Generated {} routes, {} islands", 
        _build_result.routes.len(),
        _build_result.islands.len()
    );
    
    // Phase 2: Compile Rust
    let project_root = find_project_root(&path)?;
    compile_rust(&project_root, release)?;
    
    // Phase 3: Find binary
    let binary_path = if release {
        project_root.join("target").join("release")
    } else {
        project_root.join("target").join("debug")
    };
    
    // Find the binary (usually same name as project)
    let app_name = project_root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app");
    let binary = binary_path.join(if cfg!(windows) { format!("{}.exe", app_name) } else { app_name.to_string() });
    
    let binary_size = if binary.exists() {
        let size = fs::metadata(&binary)?.len();
        info!("Binary size: {:.2} KB", size as f64 / 1024.0);
        Some(size)
    } else {
        None
    };
    
    Ok(CompilationResult {
        binary_path: if binary.exists() { Some(binary) } else { None },
        binary_size,
    })
}
