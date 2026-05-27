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
use tracing::{info, error, warn};

use serde::{Serialize, Deserialize};
use crate::config::Config;
use crate::transpile::{Transpiler, hir::Module, codegen::CodeGenerator, analyzer::Analyzer};
use crate::commands::parallel;
use crate::commands::incremental::{BuildCache, CacheEntry, compute_file_hash, IncrementalStats};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    pub pattern: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub file: String,
    pub params: Vec<String>,
    pub methods: Vec<HttpMethod>,
    /// Name of the default-export component (for SSR wrapper)
    pub component_name: Option<String>,
}

/// HTTP method
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandEntry {
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub file: String,
    pub props_type: Option<String>,
}

/// Component entry
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    // Use incremental builds when enabled
    if config.build.incremental {
        info!("Incremental build enabled");
        return run_incremental_build(config, &project_root).await;
    }

    // Collect files to transpile
    let mut routes = Vec::new();
    let mut islands = Vec::new();
    let mut components = Vec::new();
    let mut generated_files = Vec::new();

    if config.build.parallel {
        info!("Using parallel transpilation...");
        use std::sync::Arc;
        use parking_lot::RwLock;

        let transpiler = Arc::new(Transpiler::new(config));
        let code_gen = Arc::new(RwLock::new(CodeGenerator::new()));

        let parallel_result = parallel::process_all_files_parallel(
            &project_root,
            transpiler,
            code_gen,
        )?;

        generated_files = parallel_result.generated_files;
        routes = parallel_result.routes;
        islands = parallel_result.islands;
        components = parallel_result.components;
    } else {
        // Create transpiler and code generator
        let mut transpiler = Transpiler::new(config);
        let mut code_gen = CodeGenerator::new();

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

/// Run an incremental build using the cache.
///
/// Algorithm:
/// 1. Load existing cache.
/// 2. Walk routes/islands/components directories.
/// 3. For each file, check hash against cache.
/// 4. Cache hit → reuse generated file + metadata.
/// 5. Cache miss → transpile, generate, and update cache.
/// 6. Prune stale entries, save cache.
/// 7. Regenerate aggregate files (routes.rs, islands.rs, etc.).
async fn run_incremental_build(config: &Config, project_root: &Path) -> Result<BuildResult> {
    let mut cache = BuildCache::load(project_root);
    let mut stats = IncrementalStats::default();

    let mut routes: Vec<RouteEntry> = Vec::new();
    let mut islands: Vec<IslandEntry> = Vec::new();
    let mut components: Vec<ComponentEntry> = Vec::new();
    let mut generated_files: Vec<GeneratedFile> = Vec::new();

    let mut existing_files: Vec<String> = Vec::new();

    // ── Routes ──────────────────────────────────────────────────────────────
    let routes_dir = project_root.join("routes");
    if routes_dir.exists() {
        info!("Processing routes (incremental)...");
        for entry in WalkDir::new(&routes_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "ts" && ext != "tsx" {
                continue;
            }
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if filename.starts_with('_') {
                continue; // skip special files
            }

            let rel = pathdiff::diff_paths(path, project_root)
                .unwrap_or_else(|| path.to_path_buf());
            let rel_str = rel.to_string_lossy().to_string().replace('\\', "/");
            existing_files.push(rel_str.clone());
            stats.files_total += 1;

            let hash = match compute_file_hash(path) {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to hash {:?}: {}", path, e);
                    continue;
                }
            };

            if cache.is_fresh(&rel_str, &hash) {
                if let Some(entry) = cache.get(&rel_str) {
                    generated_files.push(GeneratedFile {
                        path: project_root.join(&entry.generated_path),
                        content: entry.generated_content.clone(),
                    });
                    if let Some(r) = &entry.route {
                        routes.push(r.clone());
                    }
                    stats.files_cached += 1;
                    if entry.route.is_some() {
                        stats.routes_cached += 1;
                    }
                    continue;
                }
            }

            // Cache miss — transpile
            stats.files_changed += 1;
            let mut transpiler = Transpiler::new(config);
            let path_buf = path.to_path_buf();
            let module = match transpiler.parse_file(&path_buf) {
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

            let mut code_gen = CodeGenerator::new();
            code_gen.set_generate_handlers(true);
            let rust_code = match code_gen.generate_module(&module) {
                Ok(c) => c,
                Err(e) => {
                    error!("Code generation failed for {:?}: {}", path, e);
                    continue;
                }
            };

            let relative = path.strip_prefix(&routes_dir).unwrap_or(path);
            let pattern = extract_route_pattern(relative);
            let params = extract_params(&pattern);
            let methods = extract_http_methods(&module);
            let component_name = module.items.iter().find_map(|item| {
                if let crate::transpile::hir::ModuleItem::Export(crate::transpile::hir::Export::Default { expr }) = item {
                    if let crate::transpile::hir::Expr::Function { decl } = expr {
                        return Some(decl.name.clone());
                    }
                }
                None
            });

            let route = RouteEntry {
                pattern: pattern.clone(),
                path: path.to_path_buf(),
                file: relative.to_string_lossy().to_string(),
                params,
                methods,
                component_name: component_name.clone(),
            };
            routes.push(route.clone());
            stats.routes_changed += 1;

            let output_path = compute_output_path(project_root, &routes_dir, relative, "routes");
            generated_files.push(GeneratedFile {
                path: output_path.clone(),
                content: rust_code.clone(),
            });

            cache.insert(rel_str, CacheEntry {
                content_hash: hash,
                generated_path: PathBuf::from(
                    pathdiff::diff_paths(&output_path, project_root)
                        .unwrap_or(output_path)
                        .to_string_lossy()
                        .to_string()
                        .replace('\\', "/")
                ),
                generated_content: rust_code,
                route: Some(route),
                island: None,
                component: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
    }

    // ── Islands ─────────────────────────────────────────────────────────────
    let islands_dir = project_root.join("islands");
    if islands_dir.exists() {
        info!("Processing islands (incremental)...");
        for entry in WalkDir::new(&islands_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "ts" && ext != "tsx" {
                continue;
            }

            let rel = pathdiff::diff_paths(path, project_root)
                .unwrap_or_else(|| path.to_path_buf());
            let rel_str = rel.to_string_lossy().to_string().replace('\\', "/");
            existing_files.push(rel_str.clone());
            stats.files_total += 1;

            let hash = match compute_file_hash(path) {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to hash {:?}: {}", path, e);
                    continue;
                }
            };

            if cache.is_fresh(&rel_str, &hash) {
                if let Some(entry) = cache.get(&rel_str) {
                    generated_files.push(GeneratedFile {
                        path: project_root.join(&entry.generated_path),
                        content: entry.generated_content.clone(),
                    });
                    if let Some(i) = &entry.island {
                        islands.push(i.clone());
                    }
                    stats.files_cached += 1;
                    if entry.island.is_some() {
                        stats.islands_cached += 1;
                    }
                    continue;
                }
            }

            stats.files_changed += 1;
            let mut transpiler = Transpiler::new(config);
            let path_buf = path.to_path_buf();
            let module = match transpiler.parse_file(&path_buf) {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to parse {:?}: {}", path, e);
                    continue;
                }
            };
            let mut code_gen = CodeGenerator::new();
            code_gen.set_generate_handlers(false);
            let rust_code = match code_gen.generate_module(&module) {
                Ok(c) => c,
                Err(e) => {
                    error!("Code generation failed for {:?}: {}", path, e);
                    continue;
                }
            };

            let name = path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            let relative = path.strip_prefix(&islands_dir).unwrap_or(path);

            let island = IslandEntry {
                name: name.clone(),
                path: path.to_path_buf(),
                file: relative.to_string_lossy().to_string(),
                props_type: None,
            };
            islands.push(island.clone());
            stats.islands_changed += 1;

            let output_path = islands_dir
                .parent()
                .unwrap_or(&islands_dir)
                .join("src")
                .join("gen")
                .join("islands")
                .join(format!("{}.rs", to_snake_case(&name)));

            generated_files.push(GeneratedFile {
                path: output_path.clone(),
                content: rust_code.clone(),
            });

            cache.insert(rel_str, CacheEntry {
                content_hash: hash,
                generated_path: PathBuf::from(
                    pathdiff::diff_paths(&output_path, project_root)
                        .unwrap_or(output_path.clone())
                        .to_string_lossy()
                        .to_string()
                        .replace('\\', "/")
                ),
                generated_content: rust_code,
                route: None,
                island: Some(island),
                component: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
    }

    // ── Components ──────────────────────────────────────────────────────────
    let components_dir = project_root.join("components");
    if components_dir.exists() {
        info!("Processing components (incremental)...");
        for entry in WalkDir::new(&components_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "ts" && ext != "tsx" {
                continue;
            }

            let rel = pathdiff::diff_paths(path, project_root)
                .unwrap_or_else(|| path.to_path_buf());
            let rel_str = rel.to_string_lossy().to_string().replace('\\', "/");
            existing_files.push(rel_str.clone());
            stats.files_total += 1;

            let hash = match compute_file_hash(path) {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to hash {:?}: {}", path, e);
                    continue;
                }
            };

            if cache.is_fresh(&rel_str, &hash) {
                if let Some(entry) = cache.get(&rel_str) {
                    generated_files.push(GeneratedFile {
                        path: project_root.join(&entry.generated_path),
                        content: entry.generated_content.clone(),
                    });
                    if let Some(c) = &entry.component {
                        components.push(c.clone());
                    }
                    stats.files_cached += 1;
                    if entry.component.is_some() {
                        stats.components_cached += 1;
                    }
                    continue;
                }
            }

            stats.files_changed += 1;
            let mut transpiler = Transpiler::new(config);
            let path_buf = path.to_path_buf();
            let module = match transpiler.parse_file(&path_buf) {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to parse {:?}: {}", path, e);
                    continue;
                }
            };
            let mut code_gen = CodeGenerator::new();
            code_gen.set_generate_handlers(false);
            let rust_code = match code_gen.generate_module(&module) {
                Ok(c) => c,
                Err(e) => {
                    error!("Code generation failed for {:?}: {}", path, e);
                    continue;
                }
            };

            let name = path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();

            let component = ComponentEntry {
                name: name.clone(),
                path: path.to_path_buf(),
                file: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
            };
            components.push(component.clone());
            stats.components_changed += 1;

            let output_path = components_dir
                .parent()
                .unwrap_or(&components_dir)
                .join("src")
                .join("gen")
                .join("components")
                .join(format!("{}.rs", to_snake_case(&name)));

            generated_files.push(GeneratedFile {
                path: output_path.clone(),
                content: rust_code.clone(),
            });

            cache.insert(rel_str, CacheEntry {
                content_hash: hash,
                generated_path: PathBuf::from(
                    pathdiff::diff_paths(&output_path, project_root)
                        .unwrap_or(output_path.clone())
                        .to_string_lossy()
                        .to_string()
                        .replace('\\', "/")
                ),
                generated_content: rust_code,
                route: None,
                island: None,
                component: Some(component),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
    }

    // Prune deleted files from cache
    cache.prune_missing(&existing_files);

    // Save updated cache
    if let Err(e) = cache.save(project_root) {
        warn!("Failed to save incremental cache: {}", e);
    }

    // ── Aggregate files (always regenerated) ────────────────────────────────
    let route_table = generate_route_table(&routes);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/routes.rs"),
        content: route_table,
    });

    let islands_manifest = generate_islands_manifest(&islands);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/islands.rs"),
        content: islands_manifest,
    });

    let components_module = generate_components_module(&components);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/components.rs"),
        content: components_module,
    });

    let lib_content = generate_lib(&routes, &islands, &components);
    generated_files.push(GeneratedFile {
        path: project_root.join("src/lib.rs"),
        content: lib_content,
    });

    let main_path = project_root.join("src/main.rs");
    if !main_path.exists() {
        let main_content = generate_main(project_root);
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
    }

    let file_tuples: Vec<(PathBuf, String)> = generated_files.iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect();
    generate_mod_files(project_root, &file_tuples);

    info!("{}", stats.summary());
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

/// Compute output path for a generated Rust file.
fn compute_output_path(
    _project_root: &Path,
    base_dir: &Path,
    relative: &Path,
    _kind: &str,
) -> PathBuf {
    let relative_without_ext = relative.with_extension("");
    let sanitized: PathBuf = relative_without_ext
        .components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy().to_string();
            sanitize_module_name(&s)
        })
        .collect();
    base_dir
        .parent()
        .unwrap_or(base_dir)
        .join("src")
        .join("gen")
        .join(sanitized)
        .with_extension("rs")
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

        // Extract default-export component name (if any)
        let component_name = module.items.iter().find_map(|item| {
            if let crate::transpile::hir::ModuleItem::Export(crate::transpile::hir::Export::Default { expr }) = item {
                if let crate::transpile::hir::Expr::Function { decl } = expr {
                    return Some(decl.name.clone());
                }
            }
            None
        });

        routes.push(RouteEntry {
            pattern: pattern.clone(),
            path: path.clone(),
            file: relative.to_string_lossy().to_string(),
            params,
            methods,
            component_name,
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

    let mut used_methods = std::collections::HashSet::new();
    for route in routes {
        for method in &route.methods {
            used_methods.insert(method);
        }
    }
    used_methods.insert(&HttpMethod::GET);

    let mut imports = Vec::new();
    if used_methods.contains(&HttpMethod::GET) { imports.push("get"); }
    if used_methods.contains(&HttpMethod::POST) { imports.push("post"); }
    if used_methods.contains(&HttpMethod::PUT) { imports.push("put"); }
    if used_methods.contains(&HttpMethod::DELETE) { imports.push("delete"); }
    let methods_import = imports.join(", ");
    code.push_str(&format!("use axum::{{Router, routing::{{{methods_import}}}}};
"));
    // Body import removed — not needed in generated wrappers
    code.push_str("use axum::response::IntoResponse;\n");
    code.push_str("use axum::extract::Request;\n");
    code.push_str("use runts_lib::runtime::prelude::*;\n\n");

    let mut module_imports: Vec<String> = Vec::new();
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
        module_imports.push(format!("use {} as {};", path, alias));
    }
    module_imports.sort();
    module_imports.dedup();
    for imp in module_imports {
        code.push_str(&imp);
        code.push('\n');
    }
    code.push('\n');

    for route in routes {
        if let Some(comp_name) = &route.component_name {
            let alias = to_snake_case(
                &route.file
                    .replace(".tsx", "")
                    .replace(".ts", "")
                    .replace('/', "_")
                    .replace('[', "")
                    .replace(']', "")
            );
            let wrapper_name = format!("wrap_{}_{}", alias, to_snake_case(comp_name));
            let render_fn = format!("{}_render_with_data", to_snake_case(comp_name));
            code.push_str(&format!(
                "async fn {}(ctx: HandlerContext, req: Request) -> impl IntoResponse {{\n",
                wrapper_name
            ));
            code.push_str("    let resp = ");
            code.push_str(&format!("{}::handle_get(ctx.clone(), req).await;\n", alias));
            code.push_str("    if resp.headers().get(\"X-Runts-Render\").is_some() {\n");
            code.push_str("        let (_parts, body) = resp.into_parts();\n");
            code.push_str("        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();\n");
            code.push_str("        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();\n");
            code.push_str(&format!(
                "        {}::{}(ctx.params, ctx.url, json).into_response()\n",
                alias, render_fn
            ));
            code.push_str("    } else {\n");
            code.push_str("        resp\n");
            code.push_str("    }\n");
            code.push_str("}\n\n");
        }
    }

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

            let handler = if method_name == "get" && route.component_name.is_some() {
                format!("wrap_{}_{}", alias, to_snake_case(route.component_name.as_ref().unwrap()))
            } else {
                format!("{}::handle_{}", alias, method_name)
            };

            code.push_str(&format!(
                "    router = router.route(\"{}\", {}({}));\n",
                route.pattern, method_name, handler
            ));
        }
        if route.methods.is_empty() {
            let handler = if route.component_name.is_some() {
                format!("wrap_{}_{}", alias, to_snake_case(route.component_name.as_ref().unwrap()))
            } else {
                format!("{}::handle_get", alias)
            };
            code.push_str(&format!(
                "    router = router.route(\"{}\", get({}));\n",
                route.pattern, handler
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
    code.push_str("pub async fn render_island(name: &str, props: serde_json::Value) -> String {\n");
    code.push_str("    let props_json = serde_json::to_string(&props).unwrap_or_default();\n");
    code.push_str("    let props_escaped = props_json.replace('&', \"&amp;\").replace('\"', \"&quot;\");\n");
    code.push_str("    format!(r#\"<div data-island=\"{}\" data-props=\"{}\"></div>\"#, name, props_escaped)\n");
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
fn generate_lib(_routes: &[RouteEntry], islands: &[IslandEntry], components: &[ComponentEntry]) -> String {
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
    code.push_str("pub use runts_lib::runtime::prelude::*;\n\n");

    // Generate component registration for SSR
    code.push_str("/// Register all components for SSR rendering.\n");
    code.push_str("/// Call this before starting the server.\n");
    code.push_str("pub fn register_components() {\n");
    for island in islands {
        let module = to_snake_case(&island.name);
        let struct_name = pascal_case(&island.name);
        let fn_name = to_snake_case(&island.name);
        code.push_str(&format!(
            "    runts_lib::runtime::vdom::register_island(\"{}\", |props, _children| {{\n",
            struct_name
        ));
        code.push_str(&format!(
            "        let parsed = serde_json::from_value(serde_json::json!(props)).unwrap_or_default();\n"
        ));
        code.push_str(&format!(
            "        Some(crate::gen::islands::{}::{}(parsed))\n",
            module, fn_name
        ));
        code.push_str("    });\n");
    }
    for comp in components {
        let module = to_snake_case(&comp.name);
        let struct_name = pascal_case(&comp.name);
        let fn_name = to_snake_case(&comp.name);
        code.push_str(&format!(
            "    runts_lib::runtime::vdom::register_component(\"{}\", |props, _children| {{\n",
            struct_name
        ));
        code.push_str(&format!(
            "        let parsed = serde_json::from_value(serde_json::json!(props)).unwrap_or_default();\n"
        ));
        code.push_str(&format!(
            "        Some(crate::gen::components::{}::{}(parsed))\n",
            module, fn_name
        ));
        code.push_str("    });\n");
    }
    code.push_str("}\n");

    code
}

fn pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Generate main.rs
fn generate_main(project_root: &Path) -> String {
    let canonical = project_root.canonicalize().unwrap_or_else(|_| project_root.to_path_buf());
    let app_name = canonical.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app");
    let crate_name = app_name.replace('-', "_");

    format!(r#"//! {app_name} - Generated by runts

use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{{layer::SubscriberExt, util::SubscriberInitExt}};

#[tokio::main]
async fn main() {{
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "{crate_name}".into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Register components for SSR
    {crate_name}::register_components();

    // Build router
    let app = {crate_name}::build_router()
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
