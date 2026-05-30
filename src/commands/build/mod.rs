//! Production build command

pub mod cargo_gen;
pub mod island_gen;
pub mod route_gen;
pub mod source_gen;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{error, info};
use walkdir::WalkDir;

use crate::commands::incremental::BuildCache;
use crate::config::Config;
use crate::plugin::get_plugin;
use crate::transpile::hir;
use crate::transpile::parser::parse_source;
use runts_plugin::hir::Module as PluginModule;
use runts_plugin::RouteInfo;
use serde::{Deserialize, Serialize};

/// Hidden build directory
pub fn build_dir(project_root: &Path) -> PathBuf {
    project_root.join(".runts").join("build")
}

/// Generate Cargo.toml
pub fn generate_build_cargo_toml(project_root: &Path, build_dir: &Path) -> Result<()> {
    cargo_gen::generate(project_root, build_dir)
}

/// Build result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub generated_files: Vec<GeneratedFile>,
    pub routes: Vec<RouteEntry>,
    pub islands: Vec<IslandEntry>,
    pub components: Vec<ComponentEntry>,
    pub binary_path: Option<PathBuf>,
    pub binary_path_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    pub pattern: String,
    pub methods: Vec<String>,
    pub file: PathBuf,
    pub component: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandEntry {
    pub name: String,
    pub file: PathBuf,
    pub props: Vec<PropEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentEntry {
    pub name: String,
    pub file: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropEntry {
    pub name: String,
    pub type_: String,
}

/// Run build command
pub async fn run_build(_config: &Config, path: PathBuf) -> Result<BuildResult> {
    let project_root = resolve_project_root(&path);
    let build_dir = build_dir(&project_root);

    create_build_dir(&build_dir)?;
    generate_cargo(&project_root, &build_dir)?;

    let ts_files = find_ts_files(&project_root);
    info!("Found {} TypeScript files", ts_files.len());

    let routes = route_gen::scan_routes(&project_root);
    let islands = island_gen::scan_islands(&project_root);
    let components = source_gen::scan_components(&project_root);
    let generated_files = source_gen::generate_all(&ts_files)?;

    write_generated_files(&build_dir, &generated_files)?;
    write_manifests(&build_dir, &routes, &islands, &components)?;

    Ok(BuildResult {
        generated_files,
        routes,
        islands,
        components,
        binary_path: None,
        binary_path_size: None,
    })
}

fn resolve_project_root(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap().join(path)
    }
}

fn create_build_dir(build_dir: &Path) -> Result<()> {
    fs::create_dir_all(build_dir).context("Failed to create build directory")
}

fn generate_cargo(project_root: &Path, build_dir: &Path) -> Result<()> {
    generate_build_cargo_toml(project_root, build_dir)
}

fn find_ts_files(project_root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "tsx" || ext == "ts" {
                let components: Vec<_> = path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect();
                if !components
                    .iter()
                    .any(|c| c.starts_with('.') || c == "node_modules")
                {
                    files.push(path.to_path_buf());
                }
            }
        }
    }
    files
}

fn write_generated_files(build_dir: &Path, files: &[GeneratedFile]) -> Result<()> {
    for file in files {
        let out_path = build_dir.join(&file.path);
        fs::create_dir_all(out_path.parent().unwrap())?;
        fs::write(&out_path, &file.content)?;
    }
    Ok(())
}

fn write_manifests(
    build_dir: &Path,
    routes: &[RouteEntry],
    islands: &[IslandEntry],
    components: &[ComponentEntry],
) -> Result<()> {
    fs::write(
        build_dir.join("src/lib.rs"),
        source_gen::generate_lib(routes, islands, components),
    )?;
    fs::write(build_dir.join("src/main.rs"), source_gen::generate_main())?;
    fs::write(
        build_dir.join("src/routes.rs"),
        route_gen::generate_route_table(routes),
    )?;
    fs::write(
        build_dir.join("src/islands.rs"),
        island_gen::generate_manifest(islands),
    )?;
    source_gen::generate_mod_files(build_dir)
}

/// Run incremental build
pub async fn run_incremental_build(
    _config: &Config,
    path: PathBuf,
    _cache: &mut BuildCache,
) -> Result<BuildResult> {
    run_build(_config, path).await
}

/// Run full build including compilation
pub async fn run_full_build(config: &Config, path: PathBuf, release: bool) -> Result<BuildResult> {
    let project_root = resolve_project_root(&path);
    let result = run_build(config, path).await?;
    let build_dir = build_dir(&project_root);

    compile_project(&build_dir, release)?;

    let binary = find_binary(&project_root, &build_dir, release);
    let binary_size = binary
        .as_ref()
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| m.len());

    Ok(BuildResult {
        binary_path: binary,
        binary_path_size: binary_size,
        ..result
    })
}

fn compile_project(build_dir: &Path, release: bool) -> Result<()> {
    let mut args = vec!["build"];
    if release {
        args.push("--release");
    }

    info!("Compiling...");
    let status = Command::new("cargo")
        .current_dir(build_dir)
        .args(&args)
        .status()?;

    if !status.success() {
        error!("Compilation failed");
        anyhow::bail!("Cargo build failed");
    }
    Ok(())
}

fn find_binary(project_root: &Path, build_dir: &Path, release: bool) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    let app_name = project_root.file_name()?.to_str()?.replace('-', "_");
    let binary = build_dir.join("target").join(profile).join(&app_name);
    if binary.exists() {
        Some(binary)
    } else {
        None
    }
}

// =============================================================================
// Plugin-based ephemeral build
// =============================================================================

/// Run build using plugin system with ephemeral temp directories
pub async fn run_plugin_build(
    _config: &Config,
    path: PathBuf,
    plugin_name: String,
    release: bool,
) -> Result<BuildResult> {
    let plugin = get_plugin(&plugin_name)?;
    let project_root = resolve_project_root(&path);

    // Create ephemeral build dir
    let build_dir = std::env::temp_dir().join(format!("runts-build-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&build_dir)?;

    // Create src/ directory
    let src_dir = build_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    // Generate Cargo.toml with plugin dependencies
    let cargo_toml = generate_cargo_toml(&plugin, &project_root)?;
    fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

    // Find TS/TSX files
    let ts_files = find_ts_files(&project_root);
    info!("Found {} TypeScript files", ts_files.len());

    // Scan for routes to build route info map
    let routes_dir = project_root.join("routes");
    let route_map = scan_routes_for_plugin(&routes_dir, &routes_dir);
    info!("Found {} routes", route_map.len());

    // Generate Rust code for each file
    let mut plugin_modules: Vec<PluginModule> = Vec::new();
    for file in &ts_files {
        let source = fs::read_to_string(file)?;
        let is_tsx = file.extension().and_then(|e| e.to_str()) == Some("tsx");
        let hir_module = parse_to_hir(&source, is_tsx)?;

        // Check if this file is a route
        let rel_path = file.strip_prefix(&project_root).context("Failed to get relative path")?;
        let rel_path_str = rel_path.to_string_lossy().to_string();

        // Get route info if this is a route file
        // Strip "routes/" prefix since route_map keys are relative to routes/
        let route_key = rel_path_str.strip_prefix("routes/").unwrap_or(&rel_path_str);
        let route_info = route_map.get(route_key).cloned();

        let hir_json = serde_json::to_string(&hir_module)?;
        // Inject source_path and route_info into the HIR JSON for the plugin
        let mut hir_value: serde_json::Value = serde_json::from_str(&hir_json)?;
        if let Some(obj) = hir_value.as_object_mut() {
            obj.insert("source_path".to_string(), serde_json::json!(rel_path_str));
            if let Some(ref ri) = route_info {
                obj.insert("route_info".to_string(), serde_json::json!({
                    "path": ri.path,
                    "methods": ri.methods,
                    "file_path": ri.file_path
                }));
            }
        }
        let hir_with_plugin_data = serde_json::to_string(&hir_value)?;
        let rust_code = plugin.codegen_module(&hir_with_plugin_data)?;

        // Convert route path to file path: remove brackets, replace / with __
        let file_path = rel_path.strip_prefix("routes/").unwrap_or(rel_path).to_string_lossy()
            .replace(".tsx", "").replace(".ts", "").replace("[", "").replace("]", "").replace("/", "__");
        let out_path = src_dir.join(file_path).with_extension("rs");
        fs::create_dir_all(out_path.parent().unwrap())?;
        fs::write(&out_path, rust_code)?;

        // Create plugin module with route info
        let mut pm = PluginModule::new();
        pm.source_path = Some(rel_path_str);
        pm.route_info = route_info;
        plugin_modules.push(pm);
    }

    // Generate entry point (main.rs) with route info
    let entry_code = plugin.codegen_entry(&plugin_modules)?;
    fs::write(src_dir.join("main.rs"), entry_code)?;

    // Build with cargo
    let mut args = vec!["build"];
    if release {
        args.push("--release");
    }

    info!("Building with cargo...");
    let status = Command::new("cargo")
        .current_dir(&build_dir)
        .args(&args)
        .status()?;

    if !status.success() {
        error!("Cargo build failed");
        // Clean up on failure but keep for debugging
        let _ = fs::remove_dir_all(&build_dir);
        anyhow::bail!("Cargo build failed");
    }

    // Copy binary to target/release or current directory
    let binary_name = "runts-app";
    let target_profile = if release { "release" } else { "debug" };
    let binary_path = build_dir.join("target").join(target_profile).join(binary_name);

    // Determine output location
    let out_path = if release {
        project_root.join("target").join("release").join(binary_name)
    } else {
        project_root.join(binary_name)
    };

    // Ensure target/release exists for release builds
    if release {
        fs::create_dir_all(project_root.join("target").join("release"))?;
    }

    fs::copy(&binary_path, &out_path)?;

    // Clean up ephemeral temp dir
    let _ = fs::remove_dir_all(&build_dir);

    let binary_size = fs::metadata(&out_path).map(|m| m.len()).ok();

    Ok(BuildResult {
        generated_files: vec![],
        routes: vec![],
        islands: vec![],
        components: vec![],
        binary_path: Some(out_path),
        binary_path_size: binary_size,
    })
}

/// Generate Cargo.toml with plugin dependencies
fn generate_cargo_toml(plugin: &Box<dyn runts_plugin::Plugin>, project_root: &Path) -> Result<String> {
    let deps_str = format_dependencies(&plugin.cargo_deps());
    let runts_lib_path = find_runts_lib_path(project_root);

    Ok(format!(
        r#"[package]
name = "runts-app"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
runts-lib = {{ path = {:?} }}

[[bin]]
name = "runts-app"
path = "src/main.rs"
"#,
        deps_str,
        runts_lib_path
    ))
}

/// Format cargo dependencies into a string
fn format_dependencies(deps: &[runts_plugin::CargoDep]) -> String {
    deps.iter().filter_map(|dep| format_one_dep(dep)).collect()
}

fn format_one_dep(dep: &runts_plugin::CargoDep) -> Option<String> {
    if let Some(path) = &dep.path {
        Some(if dep.features.is_empty() {
            format!("{} = {{ path = {:?} }}\n", dep.name, path)
        } else {
            format!("{} = {{ path = {:?}, features = {:?} }}\n", dep.name, path, dep.features)
        })
    } else if let Some(version) = &dep.version {
        Some(if dep.features.is_empty() {
            format!("{} = \"{}\"\n", dep.name, version)
        } else {
            format!("{} = {{ version = \"{}\", features = {:?} }}\n", dep.name, version, dep.features)
        })
    } else {
        None
    }
}

/// Find runts-lib path - returns absolute path
fn find_runts_lib_path(project_root: &Path) -> PathBuf {
    let candidates = [
        project_root.join("crates/runts-lib"),
        PathBuf::from("/Users/admin/Code/GitHub/runie-tsx/crates/runts-lib"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
        }
    }
    // Fallback: try to find relative to current working directory
    std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.join("crates/runts-lib").exists().then_some(cwd.join("crates/runts-lib")))
        .unwrap_or_else(|| PathBuf::from("crates/runts-lib"))
}

/// Parse TypeScript source to HIR module
fn parse_to_hir(source: &str, is_tsx: bool) -> Result<hir::Module> {
    parse_source(source, is_tsx)
}

/// Scan routes directory and build a map of file paths to RouteInfo
fn scan_routes_for_plugin(routes_dir: &Path, root_dir: &Path) -> std::collections::HashMap<String, RouteInfo> {
    let mut route_map = std::collections::HashMap::new();

    if !routes_dir.exists() {
        return route_map;
    }

    // Walk routes directory
    if let Ok(entries) = std::fs::read_dir(routes_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                // Recurse into subdirectories, keeping root_dir the same
                let subdir_routes = scan_routes_for_plugin(&path, root_dir);
                route_map.extend(subdir_routes);
            } else if path.extension().and_then(|e| e.to_str()) == Some("tsx")
                || path.extension().and_then(|e| e.to_str()) == Some("ts")
            {
                // This is a route file - check for underscore prefix in filename
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if file_name.starts_with('_') {
                    continue; // Skip underscore files (middleware, layouts)
                }
                if let Some(route_info) = path_to_route_info(&path, root_dir) {
                    let rel_path = route_info.file_path.clone();
                    route_map.insert(rel_path, route_info);
                }
            }
        }
    }

    route_map
}

/// Convert a route file path to RouteInfo
fn path_to_route_info(file_path: &Path, root_dir: &Path) -> Option<RouteInfo> {
    let relative_path = get_relative_route_path(file_path, root_dir)?;
    // Filter out files starting with underscore (middleware, layouts, etc)
    if is_underscore_file(&relative_path) {
        return None;
    }
    let url_path = compute_route_url_path(&relative_path)?;
    let axum_path = url_path.replace("[", ":").replace("]", "");

    Some(RouteInfo {
        path: axum_path,
        methods: vec!["GET".to_string()],
        file_path: relative_path,
    })
}

/// Get relative path from routes directory (using root_dir for correct subdirectory handling)
fn get_relative_route_path(file_path: &Path, root_dir: &Path) -> Option<String> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let root_dir_str = root_dir.to_string_lossy().to_string();
    let relative = file_path_str.strip_prefix(&root_dir_str)?;
    Some(relative.trim_start_matches('/').to_string())
}

/// Check if filename is a middleware (normalized - no extension)
fn is_middleware(filename: &str) -> bool {
    filename == "_middleware"
}

/// Check if file (not just segment) starts with underscore - catches _middleware.ts, _layout.tsx etc
fn is_underscore_file(filename: &str) -> bool {
    filename.starts_with('_')
}

/// Check if filename is a layout (starts with _, not a catch-all)
fn is_layout(filename: &str) -> bool {
    filename.starts_with('_') && !filename.starts_with("[_")
}

/// Convert a file segment to URL segment
fn file_segment_to_url(file_name: &str) -> Option<String> {
    let name = file_name.replace(".tsx", "").replace(".ts", "");
    if name.is_empty() || is_middleware(&name) || is_layout(&name) {
        return None;
    }
    Some(name.replace("[", ":").replace("]", ""))
}

/// Compute route URL path from relative file path
fn compute_route_url_path(relative_path: &str) -> Option<String> {
    let parts: Vec<&str> = relative_path.split('/').collect();
    let mut url_path = String::new();

    for part in &parts {
        if let Some(segment) = file_segment_to_url(part) {
            if url_path.is_empty() {
                if segment == "index" {
                    url_path.push('/');
                } else {
                    url_path.push_str(&format!("/{}", segment));
                }
            } else {
                url_path.push_str(&format!("/{}", segment));
            }
        }
    }

    if url_path.is_empty() {
        url_path.push('/');
    }

    Some(url_path)
}
