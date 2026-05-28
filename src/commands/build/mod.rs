//! Production build command

pub mod cargo_gen;
pub mod route_gen;
pub mod island_gen;
pub mod source_gen;

use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use walkdir::WalkDir;
use tracing::{info, error};

use serde::{Serialize, Deserialize};
use crate::config::Config;
use crate::commands::incremental::BuildCache;

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
pub struct GeneratedFile { pub path: PathBuf, pub content: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry { pub pattern: String, pub methods: Vec<String>, pub file: PathBuf, pub component: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandEntry { pub name: String, pub file: PathBuf, pub props: Vec<PropEntry> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentEntry { pub name: String, pub file: PathBuf }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropEntry { pub name: String, pub type_: String }

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

    Ok(BuildResult { generated_files, routes, islands, components, binary_path: None, binary_path_size: None })
}

fn resolve_project_root(path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap().join(path) }
}

fn create_build_dir(build_dir: &Path) -> Result<()> {
    fs::create_dir_all(build_dir).context("Failed to create build directory")
}

fn generate_cargo(project_root: &Path, build_dir: &Path) -> Result<()> {
    generate_build_cargo_toml(project_root, build_dir)
}

fn find_ts_files(project_root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(project_root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "tsx" || ext == "ts" {
                let components: Vec<_> = path.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
                if !components.iter().any(|c| c.starts_with('.') || c == "node_modules") {
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

fn write_manifests(build_dir: &Path, routes: &[RouteEntry], islands: &[IslandEntry], components: &[ComponentEntry]) -> Result<()> {
    fs::write(build_dir.join("src/lib.rs"), source_gen::generate_lib(routes, islands, components))?;
    fs::write(build_dir.join("src/main.rs"), source_gen::generate_main())?;
    fs::write(build_dir.join("src/routes.rs"), route_gen::generate_route_table(routes))?;
    fs::write(build_dir.join("src/islands.rs"), island_gen::generate_manifest(islands))?;
    source_gen::generate_mod_files(build_dir)
}

/// Run incremental build
pub async fn run_incremental_build(_config: &Config, path: PathBuf, _cache: &mut BuildCache) -> Result<BuildResult> {
    run_build(_config, path).await
}

/// Run full build including compilation
pub async fn run_full_build(config: &Config, path: PathBuf, release: bool) -> Result<BuildResult> {
    let project_root = resolve_project_root(&path);
    let result = run_build(config, path).await?;
    let build_dir = build_dir(&project_root);

    compile_project(&build_dir, release)?;

    let binary = find_binary(&project_root, &build_dir, release);
    let binary_size = binary.as_ref().and_then(|p| fs::metadata(p).ok()).map(|m| m.len());

    Ok(BuildResult { binary_path: binary, binary_path_size: binary_size, ..result })
}

fn compile_project(build_dir: &Path, release: bool) -> Result<()> {
    let mut args = vec!["build"];
    if release { args.push("--release"); }

    info!("Compiling...");
    let status = Command::new("cargo").current_dir(build_dir).args(&args).status()?;

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
    if binary.exists() { Some(binary) } else { None }
}
