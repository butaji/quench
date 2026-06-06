//! Production build command
//!

pub mod cargo_gen;
pub mod island_gen;
pub mod plugin_build;
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
    let project_root = resolve_project_root(&path)?;
    let build_dir = build_dir(&project_root);

    create_build_dir(&build_dir)?;
    generate_cargo(&project_root, &build_dir)?;

    let ts_files = find_ts_files(&project_root);
    info!("Found {} TypeScript files", ts_files.len());

    let routes = route_gen::scan_routes(&project_root)?;
    let islands = island_gen::scan_islands(&project_root);
    let components = source_gen::scan_components(&project_root);
    let gen_result = source_gen::generate_all(&ts_files)?;

    write_generated_files(&build_dir, &gen_result.generated_files)?;
    write_manifests(&build_dir, &routes, &islands, &components, &ts_files, &gen_result)?;

    Ok(BuildResult {
        generated_files: gen_result.generated_files,
        routes,
        islands,
        components,
        binary_path: None,
        binary_path_size: None,
    })
}

fn resolve_project_root(path: &Path) -> anyhow::Result<PathBuf> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let cwd = std::env::current_dir()
            .context("Failed to get current working directory")?;
        cwd.join(path)
    };

    // If path is a file (not a directory), use its parent as project root
    if resolved.is_file() {
        resolved
            .parent()
            .map(|p| p.to_path_buf())
            .context("File has no parent directory")
    } else {
        Ok(resolved)
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
    for entry in WalkDir::new(project_root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if is_hidden_or_test_file(path) || is_excluded_subpath(project_root, path) {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if (ext_lower == "tsx" || ext_lower == "ts") && !has_hidden_component(path) {
                files.push(path.to_path_buf());
            }
        }
    }
    files
}

/// Return true if any path component of `path` that is *strictly below*
/// `project_root` matches the excluded list. The `project_root` itself
/// and any of its ancestors (e.g. when the user passes
/// `examples/my-blog` and `examples` happens to be in the list) are NOT
/// considered for exclusion.
fn is_excluded_subpath(project_root: &Path, path: &Path) -> bool {
    let root_comps = project_root.components().count();
    path.components().enumerate().any(|(i, c)| {
        // Skip the leading components that are part of project_root.
        if i < root_comps {
            return false;
        }
        let s = c.as_os_str().to_string_lossy();
        if s == "." || s == ".." {
            return false;
        }
        is_excluded_name(&s)
    })
}

/// Directories that should never be walked as project source. This is
/// conservative — when in doubt, skip the directory. The user is
/// expected to put their `routes/`, `islands/`, and `components/` at
/// the project root or under a non-excluded subdir.
fn is_excluded_dir(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        if s == "." || s == ".." {
            return false;
        }
        is_excluded_name(&s)
    })
}

/// Single-component name check used by the project-root-aware
/// `is_excluded_subpath` and the legacy `is_excluded_dir` (which is
/// kept for direct callers; `find_ts_files` uses `is_excluded_subpath`
/// instead).
fn is_excluded_name(name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        // build / target dirs
        "target",
        ".runts",
        "node_modules",
        // runie-tsx internal layout that is NEVER the user's project
        // (when the user is testing on examples/my-blog, the walker
        // starts at examples/my-blog and never enters these).
        "crates",
        ".git",
        "docs",
        "dist",
        // common framework / tooling output
        ".next",
        ".turbo",
        ".svelte-kit",
        "build",
        "out",
        // runie-tsx test fixtures
        "test-fixtures",
        "test-project",
    ];
    EXCLUDED.iter().any(|ex| *ex == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn is_excluded_name_basic() {
        assert!(is_excluded_name("target"));
        assert!(is_excluded_name(".runts"));
        assert!(is_excluded_name("node_modules"));
        assert!(is_excluded_name("crates"));
        assert!(!is_excluded_name("routes"));
        assert!(!is_excluded_name("islands"));
        assert!(!is_excluded_name("components"));
        assert!(!is_excluded_name("src"));
    }

    #[test]
    fn is_excluded_subpath_skips_project_root_components() {
        // The user passes `examples/my-blog`; the walker should NOT
        // exclude the project root itself just because `examples` is
        // (was) in the excluded list. The bug we're guarding against
        // was: an earlier version of this function treated the root
        // as part of the path under inspection, so the user's project
        // got skipped because one of its ancestors was in EXCLUDED.
        let root = PathBuf::from("/home/user/project/examples/my-blog");
        let file = PathBuf::from("/home/user/project/examples/my-blog/routes/index.tsx");
        assert!(!is_excluded_subpath(&root, &file));

        // But a subdir like `.runts/build/` *should* be excluded.
        let dot_runs = PathBuf::from("/home/user/project/examples/my-blog/.runts/build");
        assert!(is_excluded_subpath(&root, &dot_runs));

        // `target` deep under a user project should be excluded too.
        let target = PathBuf::from("/home/user/project/examples/my-blog/target/x.ts");
        assert!(is_excluded_subpath(&root, &target));
    }

    #[test]
    fn is_excluded_subpath_handles_dot_and_dotdot() {
        // The current working dir (.) and parent (..) components must
        // never be flagged as excluded, even though they start with a
        // dot in the sense of "is_excluded_name('.')" matching some
        // EXCLUDED entry.
        let root = PathBuf::from("/home/user/project");
        let dot = PathBuf::from("/home/user/project/.");
        let dotdot = PathBuf::from("/home/user/project/..");
        assert!(!is_excluded_subpath(&root, &dot));
        assert!(!is_excluded_subpath(&root, &dotdot));
    }
}

fn is_hidden_or_test_file(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        return name.ends_with(".test.ts")
            || name.ends_with(".test.tsx")
            || name.ends_with(".spec.ts")
            || name.ends_with(".spec.tsx")
            || name.ends_with("_test.ts")
            || name.ends_with("_test.tsx");
    }
    false
}

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s.starts_with('.') || s == "node_modules"
    })
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
    ts_files: &[PathBuf],
    gen_result: &source_gen::SourceGenResult,
) -> Result<()> {
    // Check if this is a single-file script build
    let is_single = source_gen::is_single_file_build(ts_files, routes, islands, components);

    if is_single {
        // For single-file builds, generate lib.rs and main.rs from HIR
        let source_file = &ts_files[0];
        if let Some(ref stmts) = gen_result.single_file_stmts {
            let lib_gen_result = source_gen::generate_lib_from_hir(stmts, source_file);
            fs::write(build_dir.join("src/lib.rs"), lib_gen_result.lib_content)?;
            fs::write(build_dir.join("src/main.rs"), source_gen::generate_main(Some(source_file), &lib_gen_result.exec_stmts, &lib_gen_result.fn_defs))?;
        } else {
            // Fallback: use the old method if no HIR statements
            let rust_code = gen_result.generated_files
                .iter()
                .find(|f| f.path.to_string_lossy().contains("gen/"))
                .map(|f| f.content.as_str());
            fs::write(
                build_dir.join("src/lib.rs"),
                source_gen::generate_lib(routes, islands, components, Some(source_file.as_path()), rust_code),
            )?;
            fs::write(build_dir.join("src/main.rs"), source_gen::generate_main(Some(source_file.as_path()), &[], &[]))?;
        }
    } else {
        // Multi-file build: standard structure
        let rust_code = gen_result.generated_files
            .iter()
            .find(|f| f.path.to_string_lossy().contains("gen/"))
            .map(|f| f.content.as_str());
        fs::write(
            build_dir.join("src/lib.rs"),
            source_gen::generate_lib(routes, islands, components, None, rust_code),
        )?;
        fs::write(build_dir.join("src/main.rs"), source_gen::generate_main(None, &[], &[]))?;
    }

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
    let project_root = resolve_project_root(&path)?;
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

    info!("Compiling (timeout: 5 minutes)...");
    let mut child = Command::new("cargo")
        .current_dir(build_dir)
        .args(&args)
        .spawn()
        .context("cargo not found in PATH — is Rust installed?")?;

    wait_for_child(&mut child)
}

fn wait_for_child(child: &mut std::process::Child) -> Result<()> {
    let timeout = std::time::Duration::from_secs(300);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    anyhow::bail!("Cargo build failed: exit status {}", status);
                }
                return Ok(());
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("Cargo build timed out after 5 minutes");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                anyhow::bail!("Cargo build wait failed: {}", e);
            }
        }
    }
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

pub use plugin_build::run_plugin_build;
