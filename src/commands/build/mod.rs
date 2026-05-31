//! Production build command

pub mod cargo_gen;
pub mod island_gen;
pub mod route_gen;
pub mod source_gen;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tracing::info;
use walkdir::WalkDir;

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
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
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
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.to_lowercase() == "tsx" || ext.to_lowercase() == "ts" {
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
        validate_output_path(build_dir, &out_path, &file.path)?;
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, &file.content)?;
    }
    Ok(())
}

/// Security: validate output path doesn't escape build directory
fn validate_output_path(build_dir: &Path, out_path: &Path, original: &Path) -> Result<()> {
    // Reject absolute paths from GeneratedFile - they could escape build_dir
    if original.is_absolute() {
        anyhow::bail!("Absolute path rejected: {:?}", original);
    }
    // Reject any path components with ".."
    for component in out_path.components() {
        if component == std::path::Component::ParentDir {
            anyhow::bail!("Path traversal attempt detected: {:?}", original);
        }
    }
    // Ensure output path is within build directory
    if !out_path.starts_with(build_dir) {
        anyhow::bail!("Path escapes build directory: {:?}", original);
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
    let output = Command::new("cargo")
        .current_dir(build_dir)
        .args(&args)
        .output()
        .context("cargo not found in PATH — is Rust installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Cargo build failed:\n{}", stderr);
    }
    Ok(())
}

fn find_binary(_project_root: &Path, build_dir: &Path, release: bool) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    // Binary name is "runts-app" per Cargo.toml [[bin]] section
    let binary_name = format!("runts-app{}", std::env::consts::EXE_SUFFIX);
    let binary = build_dir.join("target").join(profile).join(&binary_name);
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

    // Create ephemeral build dir using TempDir for secure cleanup
    let temp_dir = TempDir::new()?;
    let build_dir = temp_dir.path().to_path_buf();

    // Create src/ directory
    let src_dir = build_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    // Generate Cargo.toml with plugin dependencies
    let cargo_toml = generate_cargo_toml(&*plugin, &project_root)?;
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
        let is_tsx = file.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) == Some("tsx".to_string());
        let hir_module = parse_to_hir(&source, is_tsx)?;

        // Check if this file is a route
        let rel_path = file.strip_prefix(&project_root)
            .with_context(|| format!("Failed to get relative path for {:?}", file))?;
        let rel_path_str = rel_path.to_string_lossy().to_string();

        // Get route info if this is a route file
        // Strip "routes/" prefix since route_map keys are relative to routes/
        let route_key = rel_path.strip_prefix(Path::new("routes"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| rel_path_str.clone());
        let route_info = route_map.get(&route_key).cloned();

        let hir_json = serde_json::to_string(&hir_module)
            .with_context(|| format!("Failed to serialize HIR for {:?}", file))?;
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

        // Convert route path to file path: remove brackets, replace path separators with __
        let rel_path_stripped = rel_path.strip_prefix(Path::new("routes/")).unwrap_or(rel_path);
        let file_path: String = rel_path_stripped.components()
            .map(|c| c.as_os_str().to_string_lossy().replace("[", "").replace("]", ""))
            .collect::<Vec<_>>()
            .join("__")
            .replace(".tsx", "")
            .replace(".ts", "");
        let out_path = src_dir.join(file_path).with_extension("rs");
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, rust_code)?;

        // Create plugin module with route info and HIR items
        // Note: HIR data is passed to codegen_module via JSON string (line 318).
        // codegen_entry receives modules with route metadata only - this is sufficient
        // for entry point generation (route table building). Per-module code gen
        // happens in codegen_module which receives the full HIR as JSON.
        let items_json = hir_value.get("items").cloned();
        let pm = PluginModule::new()
            .with_source_path(rel_path_str)
            .with_route_info(route_info.clone())
            .with_items_json(items_json);
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
    let output = Command::new("cargo")
        .current_dir(&build_dir)
        .args(&args)
        .output()
        .context("cargo not found in PATH — is Rust installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Cargo build failed:\n{}", stderr);
    }

    // Copy binary to target/release or current directory
    let binary_name = format!("runts-app{}", std::env::consts::EXE_SUFFIX);
    let target_profile = if release { "release" } else { "debug" };
    let binary_path = build_dir.join("target").join(target_profile).join(&binary_name);

    // Determine output location
    let out_path = if release {
        project_root.join("target").join("release").join(&binary_name)
    } else {
        project_root.join(&binary_name)
    };

    // Ensure target/release exists for release builds
    if release {
        fs::create_dir_all(project_root.join("target").join("release"))?;
    }

    if !binary_path.exists() {
        anyhow::bail!(
            "Build succeeded but binary not found at {}. Check Cargo.toml package name matches directory name.",
            binary_path.display()
        );
    }
    fs::copy(&binary_path, &out_path)?;

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
fn generate_cargo_toml(plugin: &dyn runts_plugin::Plugin, project_root: &Path) -> Result<String> {
    let deps_str = format_dependencies(&plugin.cargo_deps());
    let runts_lib_path = find_runts_lib_path(project_root);

    Ok(format!(
        r#"[package]
name = "runts-app"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
runts-lib = {{ path = "{}" }}

[[bin]]
name = "runts-app"
path = "src/main.rs"
"#,
        deps_str,
        runts_lib_path.display()
    ))
}

/// Format cargo dependencies into a string
fn format_dependencies(deps: &[runts_plugin::CargoDep]) -> String {
    deps.iter().filter_map(|dep| format_one_dep(dep)).collect()
}

fn format_one_dep(dep: &runts_plugin::CargoDep) -> Option<String> {
    if let Some(path) = &dep.path {
        Some(if dep.features.is_empty() {
            format!("{} = {{ path = \"{}\" }}\n", dep.name, path.display())
        } else {
            format!("{} = {{ path = \"{}\", features = {:?} }}\n", dep.name, path.display(), dep.features)
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
    // Try relative to current exe first
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join("crates/runts-lib");
            if candidate.exists() {
                return candidate.canonicalize().unwrap_or(candidate);
            }
            // Also try parent directories (in case exe is in target/debug/ or target/release/)
            for ancestor in exe_dir.ancestors() {
                let candidate = ancestor.join("crates/runts-lib");
                if candidate.exists() {
                    return candidate.canonicalize().unwrap_or(candidate);
                }
            }
        }
    }

    // Fallback: relative to project root
    let candidate = project_root.join("crates/runts-lib");
    if candidate.exists() {
        return candidate.canonicalize().unwrap_or(candidate);
    }

    // Fallback: try CARGO_MANIFEST_DIR at compile time via env!
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = PathBuf::from(manifest_dir);
    let runts_lib_from_manifest = manifest_path.parent()
        .map(|p| p.join("runts-lib"))
        .unwrap_or_else(|| manifest_path.join("runts-lib"));
    if runts_lib_from_manifest.exists() {
        return runts_lib_from_manifest.canonicalize()
            .unwrap_or(runts_lib_from_manifest);
    }

    // Last resort: return project_root relative path
    PathBuf::from("crates/runts-lib")
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

    if let Ok(entries) = std::fs::read_dir(routes_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(file_type) = entry.file_type().ok() {
                if file_type.is_dir() {
                    // Recurse into real directories only (not symlinks)
                    let subdir_routes = scan_routes_for_plugin(&path, root_dir);
                    route_map.extend(subdir_routes);
                } else if file_type.is_file() {
                    // Process real files only (not symlinks)
                    process_route_file(&path, root_dir, &mut route_map);
                }
            }
        }
    }

    route_map
}

/// Process a single route file if it meets criteria
fn process_route_file(path: &Path, root_dir: &Path, route_map: &mut std::collections::HashMap<String, RouteInfo>) {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase());
    if ext != Some("tsx".to_string()) && ext != Some("ts".to_string()) {
        return;
    }

    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if file_name.starts_with('_') {
        return;
    }

    if let Some(route_info) = path_to_route_info(path, root_dir) {
        route_map.insert(route_info.file_path.clone(), route_info);
    }
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
    let relative = file_path.strip_prefix(root_dir).ok()?;
    Some(relative.to_string_lossy().into_owned())
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
    let path = Path::new(relative_path);
    let parts: Vec<_> = path.components().filter_map(|c| c.as_os_str().to_str()).collect();
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
                // If segment is "index" and we already have a path, stop here
                // This makes /blog/index.tsx → /blog instead of /blog/index
                if segment == "index" {
                    break;
                }
                url_path.push_str(&format!("/{}", segment));
            }
        }
    }

    if url_path.is_empty() {
        url_path.push('/');
    }

    Some(url_path)
}
