//! Parallel file processing utilities
//!
//! Uses rayon for parallel transpilation of TS/TSX files.

use anyhow::Result;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;
use walkdir::WalkDir;
use crate::commands::dev::routes::HttpMethod;
use tracing::{info, error};

use crate::transpile::{Transpiler, codegen::CodeGenerator, analyzer::Analyzer};
use crate::util::to_snake_case;

use super::build::{GeneratedFile, RouteEntry, IslandEntry, ComponentEntry};

/// Parallel transpilation result
pub struct ParallelBuildResult {
    pub generated_files: Vec<GeneratedFile>,
    pub routes: Vec<RouteEntry>,
    pub islands: Vec<IslandEntry>,
    pub components: Vec<ComponentEntry>,
}

/// Process all files in parallel
pub fn process_all_files_parallel(
    project_root: &std::path::Path,
    transpiler: Arc<Transpiler>,
    code_gen: Arc<RwLock<CodeGenerator>>,
) -> Result<ParallelBuildResult> {
    let routes_dir = project_root.join("routes");
    let islands_dir = project_root.join("islands");
    let components_dir = project_root.join("components");

    // Collect all files first
    let route_files = collect_files(&routes_dir, &["tsx", "ts"]);
    let island_files = collect_files(&islands_dir, &["tsx", "ts"]);
    let component_files = collect_files(&components_dir, &["tsx", "ts"]);

    info!("Found {} route files, {} island files, {} component files",
          route_files.len(), island_files.len(), component_files.len());

    // Process routes in parallel
    let (route_results, routes) = process_routes_parallel(&route_files, &routes_dir, &transpiler, &code_gen);

    // Process islands in parallel
    let (island_results, islands) = process_islands_parallel(&island_files, &islands_dir, &transpiler, &code_gen);

    // Process components in parallel
    let (component_results, components) = process_components_parallel(&component_files, &components_dir, &transpiler, &code_gen);

    // Combine all generated files
    let mut generated_files = Vec::new();
    generated_files.extend(route_results);
    generated_files.extend(island_results);
    generated_files.extend(component_results);

    Ok(ParallelBuildResult {
        generated_files,
        routes,
        islands,
        components,
    })
}

/// Collect all .ts/.tsx files from a directory
fn collect_files(dir: &std::path::Path, extensions: &[&str]) -> Vec<PathBuf> {
    if !dir.exists() {
        return Vec::new();
    }

    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            if e.path().is_dir() {
                return false;
            }
            if let Some(ext) = e.path().extension().and_then(|e| e.to_str()) {
                extensions.contains(&ext)
            } else {
                false
            }
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Process routes in parallel
fn process_routes_parallel(
    files: &[PathBuf],
    base: &std::path::Path,
    transpiler: &Arc<Transpiler>,
    code_gen: &Arc<RwLock<CodeGenerator>>,
) -> (Vec<GeneratedFile>, Vec<RouteEntry>) {
    let base = base;
    
    let results: Vec<(Result<(GeneratedFile, Option<RouteEntry>), TranspileError>, PathBuf)> = files
        .par_iter()
        .map(|path| {
            let result = process_single_route(path, &base, transpiler, code_gen);
            (result, path.clone())
        })
        .collect();

    let mut generated_files = Vec::new();
    let mut routes = Vec::new();

    for (result, path) in results {
        match result {
            Ok((file, maybe_route)) => {
                generated_files.push(file);
                if let Some(route) = maybe_route {
                    routes.push(route);
                }
            }
            Err(e) => {
                error!("Failed to process route {:?}: {}", path, e);
            }
        }
    }

    (generated_files, routes)
}

/// Process islands in parallel
fn process_islands_parallel(
    files: &[PathBuf],
    base: &std::path::Path,
    transpiler: &Arc<Transpiler>,
    code_gen: &Arc<RwLock<CodeGenerator>>,
) -> (Vec<GeneratedFile>, Vec<IslandEntry>) {
    let base = base;
    
    let results: Vec<(Result<(GeneratedFile, IslandEntry), TranspileError>, PathBuf)> = files
        .par_iter()
        .map(|path| {
            let result = process_single_island(path, &base, transpiler, code_gen);
            (result, path.clone())
        })
        .collect();

    let mut generated_files = Vec::new();
    let mut islands = Vec::new();

    for (result, path) in results {
        match result {
            Ok((file, island)) => {
                generated_files.push(file);
                islands.push(island);
            }
            Err(e) => {
                error!("Failed to process island {:?}: {}", path, e);
            }
        }
    }

    (generated_files, islands)
}

/// Process components in parallel
fn process_components_parallel(
    files: &[PathBuf],
    components_dir: &std::path::Path,
    transpiler: &Arc<Transpiler>,
    code_gen: &Arc<RwLock<CodeGenerator>>,
) -> (Vec<GeneratedFile>, Vec<ComponentEntry>) {
    let results: Vec<Result<(GeneratedFile, ComponentEntry), TranspileError>> = files
        .par_iter()
        .map(|path| process_single_component(path, components_dir, transpiler, code_gen))
        .collect();

    let mut generated_files = Vec::new();
    let mut components = Vec::new();

    for result in results {
        match result {
            Ok((file, component)) => {
                generated_files.push(file);
                components.push(component);
            }
            Err(e) => {
                error!("Failed to process component: {}", e);
            }
        }
    }

    (generated_files, components)
}

/// Transpile error with context
#[derive(Debug)]
pub struct TranspileError {
    pub path: PathBuf,
    pub message: String,
}

impl std::fmt::Display for TranspileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for TranspileError {}

/// Process a single route file (layouts are generated but not registered as routes)
fn process_single_route(
    path: &std::path::Path,
    base: &std::path::Path,
    transpiler: &Arc<Transpiler>,
    _code_gen: &Arc<RwLock<CodeGenerator>>,
) -> Result<(GeneratedFile, Option<RouteEntry>), TranspileError> {
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // Skip middleware and error pages entirely
    if filename.starts_with('_') && !filename.contains("layout") {
        return Err(TranspileError {
            path: path.to_path_buf(),
            message: "Skipping special file".to_string(),
        });
    }

    let is_layout = filename == "_layout.tsx" || filename == "_layout.ts";

    // Read source
    let source = std::fs::read_to_string(path)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Failed to read: {}", e),
        })?;

    // Parse
    let module = transpiler.parse_source(&source)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Parse error: {}", e),
        })?;

    // Analyze
    let mut analyzer = Analyzer::new();
    if let Err(errors) = analyzer.analyze(&module) {
        return Err(TranspileError {
            path: path.to_path_buf(),
            message: format!("Analysis errors: {:?}", errors),
        });
    }

    // Generate with a fresh CodeGenerator to avoid shared-state races
    let mut cg = CodeGenerator::new();
    cg.set_generate_handlers(!is_layout);
    let rust_code = cg.generate_module(&module)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Code generation: {}", e),
        })?;

    // Extract route info
    let relative = path.strip_prefix(base)
        .unwrap_or(path);
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

    let route = if is_layout {
        None
    } else {
        let methods_str: Vec<String> = methods.iter().map(|m| format!("{:?}", m)).collect();
        Some(RouteEntry {
            pattern: pattern.clone(),
            methods: methods_str,
            file: path.to_path_buf(),
            component: component_name,
        })
    };

    // Generate output path (sanitize module names like build.rs does)
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
        .join(".runts")
        .join("build")
        .join("src")
        .join("gen")
        .join(sanitized)
        .with_extension("rs");

    Ok((GeneratedFile {
        path: output_path,
        content: rust_code,
    }, route))
}

/// Process a single island file
fn process_single_island(
    path: &std::path::Path,
    base: &std::path::Path,
    transpiler: &Arc<Transpiler>,
    _code_gen: &Arc<RwLock<CodeGenerator>>,
) -> Result<(GeneratedFile, IslandEntry), TranspileError> {
    // Read source
    let source = std::fs::read_to_string(path)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Failed to read: {}", e),
        })?;

    // Parse
    let module = transpiler.parse_source(&source)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Parse error: {}", e),
        })?;

    // Generate with a fresh CodeGenerator (islands don't need route handlers)
    let mut cg = CodeGenerator::new();
    cg.set_generate_handlers(false);
    let rust_code = cg.generate_module(&module)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Code generation: {}", e),
        })?;

    // Get island name
    let name = path.file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let relative = path.strip_prefix(base)
        .unwrap_or(path);

    let island = IslandEntry {
        name: name.clone(),
        file: path.to_path_buf(),
        props: vec![],
    };

    // Generate output path
    let output_path = base
        .parent()
        .unwrap_or(base)
        .join(".runts")
        .join("build")
        .join("src")
        .join("gen")
        .join("islands")
        .join(format!("{}.rs", to_snake_case(&name)));

    Ok((GeneratedFile {
        path: output_path,
        content: rust_code,
    }, island))
}

/// Process a single component file
fn process_single_component(
    path: &std::path::Path,
    components_dir: &std::path::Path,
    transpiler: &Arc<Transpiler>,
    _code_gen: &Arc<RwLock<CodeGenerator>>,
) -> Result<(GeneratedFile, ComponentEntry), TranspileError> {
    // Read source
    let source = std::fs::read_to_string(path)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Failed to read: {}", e),
        })?;

    // Parse
    let module = transpiler.parse_source(&source)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Parse error: {}", e),
        })?;

    // Generate with a fresh CodeGenerator (components don't need route handlers)
    let mut cg = CodeGenerator::new();
    cg.set_generate_handlers(false);
    let rust_code = cg.generate_module(&module)
        .map_err(|e| TranspileError {
            path: path.to_path_buf(),
            message: format!("Code generation: {}", e),
        })?;

    let name = path.file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    // Components are in <project_root>/components/; generated code goes to <project_root>/.runts/build/src/gen/components/
    let output_path = components_dir
        .parent()
        .unwrap_or(components_dir)
        .join(".runts")
        .join("build")
        .join("src")
        .join("gen")
        .join("components")
        .join(format!("{}.rs", to_snake_case(&name)));

    let component = ComponentEntry {
        name: name.clone(),
        file: path.to_path_buf(),
    };

    Ok((GeneratedFile {
        path: output_path,
        content: rust_code,
    }, component))
}

/// Extract route pattern from file path
fn extract_route_pattern(relative: &Path) -> String {
    let parts: Vec<String> = relative.iter()
        .filter_map(|p| p.to_str())
        .filter(|p| !["routes", "islands", "components"].contains(p))
        .filter(|p| *p != "index" && *p != "index.tsx" && *p != "index.ts")
        .map(|p| {
            let p = p.trim_end_matches(".tsx").trim_end_matches(".ts");
            if p.starts_with("[...") && p.ends_with(']') {
                // Catch-all route: [...slug] -> :slug
                format!(":{}", &p[4..p.len() - 1])
            } else if p.starts_with('[') && p.ends_with(']') {
                // Dynamic route: [slug] -> :slug
                format!(":{}", &p[1..p.len() - 1])
            } else {
                p.to_string()
            }
        })
        .collect();

    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Extract parameter names from route pattern
fn extract_params(pattern: &str) -> Vec<String> {
    let re = regex::Regex::new(r":(\w+)").unwrap();
    re.captures_iter(pattern)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// Extract HTTP methods from route handlers
fn extract_http_methods(module: &crate::transpile::hir::Module) -> Vec<HttpMethod> {
    let mut methods = vec![HttpMethod::GET];

    for item in &module.items {
        if let crate::transpile::hir::ModuleItem::Export(export) = item {
            if let crate::transpile::hir::Export::Named { name } = export {
                let name_lower = name.to_lowercase();
                add_method_from_name(&mut methods, &name_lower);
            }
        }
    }

    methods
}

fn add_method_from_name(methods: &mut Vec<HttpMethod>, name_lower: &str) {
    if name_lower.contains("get") && !name_lower.contains("post") {
        add_unique_method(methods, HttpMethod::GET);
    }
    if name_lower.contains("post") {
        add_unique_method(methods, HttpMethod::POST);
    }
    if name_lower.contains("put") {
        add_unique_method(methods, HttpMethod::PUT);
    }
    if name_lower.contains("delete") {
        add_unique_method(methods, HttpMethod::DELETE);
    }
}

fn add_unique_method(methods: &mut Vec<HttpMethod>, method: HttpMethod) {
    if !methods.contains(&method) {
        methods.push(method);
    }
}

/// Sanitize a file stem into a valid Rust module name.
fn sanitize_module_name(stem: &str) -> String {
    let mut s = stem
        .replace('[', "")
        .replace(']', "")
        .replace('-', "_")
        .replace('.', "_");
    if let Some(first) = s.chars().next() {
        if first.is_ascii_digit() {
            s = format!("_{}", s);
        }
    }
    to_snake_case(&s)
}

/// Extend Transpiler with parse_source method
impl Transpiler {
    /// Parse source string directly
    pub fn parse_source(&self, source: &str) -> Result<crate::transpile::hir::Module, anyhow::Error> {
        let mut parser = crate::transpile::Parser::new();
        parser.parse_source(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_route_pattern() {
        assert_eq!(extract_route_pattern(Path::new("index.tsx")), "/");
        assert_eq!(extract_route_pattern(Path::new("blog/index.tsx")), "/blog");
        assert_eq!(extract_route_pattern(Path::new("blog/[slug].tsx")), "/blog/:slug");
        assert_eq!(extract_route_pattern(Path::new("[...path].tsx")), "/:path");
    }

    #[test]
    fn test_extract_params() {
        assert_eq!(extract_params("/blog/:slug"), vec!["slug"]);
        assert_eq!(extract_params("/:year/:month"), vec!["year", "month"]);
        assert_eq!(extract_params("/"), Vec::<String>::new());
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("Counter"), "counter");
        assert_eq!(to_snake_case("TodoList"), "todo_list");
        assert_eq!(to_snake_case("myComponent"), "my_component");
    }
}
