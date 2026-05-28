//! Route generation

use std::path::Path;
use walkdir::WalkDir;

use crate::commands::build::RouteEntry;

/// Scan routes directory for route files
pub fn scan_routes(project_root: &Path) -> Vec<RouteEntry> {
    let routes_dir = project_root.join("routes");
    let mut routes = Vec::new();

    if !routes_dir.exists() {
        return routes;
    }

    for entry in WalkDir::new(&routes_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "tsx" || ext == "ts" {
                if let Some(pattern) = file_to_pattern(&routes_dir, path) {
                    routes.push(RouteEntry {
                        pattern,
                        methods: vec!["GET".to_string()],
                        file: path.to_path_buf(),
                        component: None,
                    });
                }
            }
        }
    }

    routes
}

/// Convert file path to route pattern
fn file_to_pattern(base: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(base).ok()?;
    let parts: Vec<_> = rel.components().collect();
    if parts.is_empty() {
        return None;
    }

    let mut pattern = String::new();
    for part in parts.iter() {
        let s = part.as_os_str().to_string_lossy();
        if s.starts_with('[') {
            pattern.push_str(&s.replace('[', "{").replace(']', "}"));
        } else if s != "index.tsx" && s != "index.ts" && s != "routes" {
            pattern.push('/');
            pattern.push_str(&s.replace(".tsx", "").replace(".ts", ""));
        }
    }
    if pattern.is_empty() {
        pattern = "/".to_string();
    }
    Some(pattern)
}

/// Generate route table
pub fn generate_route_table(routes: &[RouteEntry]) -> String {
    let mut output = String::new();
    output.push_str("//! Auto-generated route table\n\n");

    for route in routes {
        let methods_str = route
            .methods
            .iter()
            .map(|m| format!("\"{}\"", m))
            .collect::<Vec<_>>()
            .join(", ");

        output.push_str(&format!(
            r#"// Route: {} [{}]
"#,
            route.pattern, methods_str
        ));
    }

    output
}
