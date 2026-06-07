//! Plugin-based ephemeral build

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use tempfile::TempDir;
use tracing::info;

use crate::config::Config;
use crate::plugin::get_plugin;
use runts_plugin::RouteInfo;

use super::{find_ts_files, resolve_project_root, BuildResult};

/// Run build using plugin system with ephemeral temp directories
pub async fn run_plugin_build(
    _config: &Config,
    path: PathBuf,
    plugin_name: String,
    release: bool,
) -> Result<BuildResult> {
    let plugin = get_plugin(&plugin_name)?;
    let project_root = resolve_project_root(&path)?;

    let (build_dir, _temp_guard) = if std::env::var_os("RUNTS_KEEP_BUILD").is_some() {
        let persistent = project_root.join(".runts").join("build");
        if persistent.exists() {
            fs::remove_dir_all(&persistent).ok();
        }
        fs::create_dir_all(&persistent).context("create persistent build dir")?;
        (persistent, None)
    } else {
        let temp = TempDir::new()?;
        (temp.path().to_path_buf(), Some(temp))
    };

    let src_dir = build_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    let cargo_toml = generate_cargo_toml(&*plugin, &project_root)?;
    fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

    let ts_files = find_ts_files(&project_root);
    info!("Found {} TypeScript files", ts_files.len());

    let routes_dir = project_root.join("routes");
    let route_map = scan_routes_for_plugin(&routes_dir, &routes_dir);
    info!("Found {} routes", route_map.len());

    let mut parsed: Vec<(PathBuf, runts_hir::Module)> = {
        use crate::transpile::parallel::FileToParse;
        let inputs: Vec<FileToParse> = ts_files
            .iter()
            .map(|p| {
                let is_tsx = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    == Some("tsx".to_string());
                FileToParse {
                    path: p.clone(),
                    source: fs::read_to_string(p).unwrap_or_default(),
                    is_tsx,
                }
            })
            .collect();
        let results = crate::transpile::parallel::parse_files_parallel(inputs);
        let mut out = Vec::with_capacity(ts_files.len());
        for (path, result) in ts_files.iter().cloned().zip(results.into_iter()) {
            let mut module = result.with_context(|| format!("failed to parse {path:?}"))?;
            let rel_path = path
                .strip_prefix(&project_root)
                .with_context(|| format!("Failed to get relative path for {:?}", path))?;
            let rel_path_str = rel_path.to_string_lossy().to_string();

            let route_key = rel_path
                .strip_prefix(Path::new("routes"))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| rel_path_str.clone());
            let route_info = route_map.get(&route_key).cloned();

            module.source_path = Some(rel_path_str);
            module.route_info = route_info;
            out.push((path, module));
        }
        out
    };

    let mut plugin_modules: Vec<runts_hir::Module> = Vec::new();
    for (file, module) in &mut parsed {
        let rust_code = plugin.codegen_module(module)?;

        let rel_path = file
            .strip_prefix(&project_root)
            .with_context(|| format!("Failed to get relative path for {:?}", file))?;
        let rel_path_stripped = rel_path.strip_prefix(Path::new("routes/")).unwrap_or(rel_path);
        let file_path: String = rel_path_stripped
            .components()
            .map(|c| {
                // Use same naming scheme as routes.rs: module_name_from_path
                let s = c.as_os_str().to_string_lossy();
                // Remove extension first (tsx before ts to avoid leaving 'x')
                let s = s.replace(".tsx", "").replace(".ts", "");
                // Extract content from brackets
                let mut result = String::new();
                let mut capturing = false;
                for ch in s.chars() {
                    match ch {
                        '[' => capturing = true,
                        ']' => capturing = false,
                        '_' | 'a'..='z' | 'A'..='Z' | '0'..='9' if !capturing => result.push(ch),
                        'a'..='z' | 'A'..='Z' | '0'..='9' => result.push(ch),
                        _ => {}
                    }
                }
                result
            })
            .collect::<Vec<_>>()
            .join("_");
        let out_path = src_dir.join(file_path).with_extension("rs");
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, rust_code)?;

        plugin_modules.push(std::mem::take(module));
    }

    let entry_code = plugin.codegen_entry(&plugin_modules)?;
    fs::write(src_dir.join("main.rs"), entry_code)?;

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

    let binary_name = format!("runts-app{}", std::env::consts::EXE_SUFFIX);
    let target_profile = if release { "release" } else { "debug" };
    let binary_path = build_dir.join("target").join(target_profile).join(&binary_name);

    let out_path = if release {
        project_root.join("target").join("release").join(&binary_name)
    } else {
        project_root.join(&binary_name)
    };

    if release {
        fs::create_dir_all(project_root.join("target").join("release"))?;
    }

    fs::copy(&binary_path, &out_path).with_context(|| {
        format!(
            "Failed to copy binary from {} to {}",
            binary_path.display(),
            out_path.display()
        )
    })?;

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

[workspace]

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
            format!(
                "{} = {{ path = \"{}\", features = {:?} }}\n",
                dep.name,
                path.display(),
                dep.features
            )
        })
    } else if let Some(version) = &dep.version {
        Some(if dep.features.is_empty() {
            format!("{} = \"{}\"\n", dep.name, version)
        } else {
            format!(
                "{} = {{ version = \"{}\", features = {:?} }}\n",
                dep.name,
                version,
                dep.features
            )
        })
    } else {
        None
    }
}

/// Find runts-lib path - returns absolute path
fn find_runts_lib_path(_project_root: &Path) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("crates/runts-lib")
}

/// Scan routes directory and build a map of file paths to RouteInfo
fn scan_routes_for_plugin(
    routes_dir: &Path,
    root_dir: &Path,
) -> std::collections::HashMap<String, RouteInfo> {
    let mut route_map = std::collections::HashMap::new();

    if !routes_dir.exists() {
        return route_map;
    }

    if let Ok(entries) = std::fs::read_dir(routes_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(file_type) = entry.file_type().ok() {
                if file_type.is_dir() {
                    let subdir_routes = scan_routes_for_plugin(&path, root_dir);
                    route_map.extend(subdir_routes);
                } else if file_type.is_file() {
                    process_route_file(&path, root_dir, &mut route_map);
                }
            }
        }
    }

    route_map
}

/// Process a single route file if it meets criteria
fn process_route_file(
    path: &Path,
    root_dir: &Path,
    route_map: &mut std::collections::HashMap<String, RouteInfo>,
) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
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

/// Get relative path from routes directory
fn get_relative_route_path(file_path: &Path, root_dir: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(root_dir).ok()?;
    Some(relative.to_string_lossy().into_owned())
}

/// Check if filename is a middleware
fn is_middleware(filename: &str) -> bool {
    filename == "_middleware"
}

/// Check if file starts with underscore
fn is_underscore_file(filename: &str) -> bool {
    filename.starts_with('_')
}

/// Check if filename is a layout
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
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
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
