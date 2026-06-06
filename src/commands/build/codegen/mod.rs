//! Route-based codegen utilities

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;
use crate::transpile::hir;
use crate::transpile::parser::parse_source;
use runts_plugin::RouteInfo;

/// Scan routes directory for plugin routes
pub fn scan_routes_for_plugin(routes_dir: &Path, root_dir: &Path) -> HashMap<String, RouteInfo> {
    let mut route_map = HashMap::new();
    if routes_dir.exists() {
        for entry in walkdir::WalkDir::new(routes_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && is_route_file(path) {
                process_route_file(path, root_dir, &mut route_map);
            }
        }
    }
    route_map
}

/// Check if file is a route file
fn is_route_file(path: &Path) -> bool {
    path.extension().map(|e| e == "ts" || e == "tsx").unwrap_or(false)
        && !path.file_name().map(|n| n.to_str().map(|s| s.starts_with('.') || s.contains(".test.") || s.contains(".spec.")).unwrap_or(false)).unwrap_or(false)
}

/// Process a single route file
fn process_route_file(path: &Path, root_dir: &Path, route_map: &mut HashMap<String, RouteInfo>) {
    if let Some(info) = path_to_route_info(path, root_dir) {
        route_map.insert(info.path.clone(), info);
    }
}

/// Convert path to route info
pub fn path_to_route_info(file_path: &Path, root_dir: &Path) -> Option<RouteInfo> {
    let route_path = get_relative_route_path(file_path, root_dir)?;
    let path = PathBuf::from("/").join(&route_path);
    
    Some(RouteInfo {
        path,
        file: file_path.to_path_buf(),
        is_middleware: is_middleware(file_path.file_name()?.to_str()?),
        is_layout: is_layout(file_path.file_name()?.to_str()?),
        is_underscore: is_underscore_file(file_path.file_name()?.to_str()?),
    })
}

/// Get relative route path from file path
pub fn get_relative_route_path(file_path: &Path, root_dir: &Path) -> Option<String> {
    let stripped = file_path.strip_prefix(root_dir).ok()?;
    let parts: Vec<_> = stripped.iter().skip(1).filter_map(|p| p.to_str()).collect();
    
    if parts.is_empty() {
        return Some("/".to_string());
    }
    
    let route_parts: Vec<_> = parts.iter().map(|p| {
        if p.starts_with('[') && p.ends_with(']') {
            format!("{{{}}}", &p[1..p.len()-1])
        } else if p.starts_with('[') {
            p.replace("[...", "{").replace(']', "...}")
        } else {
            p.to_string()
        }
    }).collect();
    
    let route = format!("/{}", route_parts.join("/"));
    Some(route.replace("/index", "").replace("index", "/"))
}

/// Check if file is middleware
pub fn is_middleware(filename: &str) -> bool {
    filename == "middleware.ts" || filename == "middleware.tsx"
}

/// Check if file is underscore file
pub fn is_underscore_file(filename: &str) -> bool {
    filename.starts_with('_')
}

/// Check if file is layout
pub fn is_layout(filename: &str) -> bool {
    filename.starts_with("layout.") || filename.starts_with("layout.ts") || filename.starts_with("layout.tsx")
}

/// Parse source to HIR module
pub fn parse_to_hir(source: &str, is_tsx: bool) -> Result<hir::Module> {
    parse_source(source, is_tsx).map_err(|e| anyhow::anyhow!("parse error: {:?}", e))
}
