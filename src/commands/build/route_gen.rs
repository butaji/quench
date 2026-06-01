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
            // Skip underscore-prefixed files (middleware, layouts, etc.)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('_') {
                    continue;
                }
            }

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

    // Check for route collisions (e.g., [id].tsx and id.tsx both matching same URL)
    detect_route_collisions(&routes);

    routes
}

/// Check for route path collisions and warn/error on duplicates
fn detect_route_collisions(routes: &[RouteEntry]) {
    use std::collections::HashMap;

    let mut pattern_seen: HashMap<&str, &Path> = HashMap::new();
    for route in routes {
        if let Some(existing) = pattern_seen.get(route.pattern.as_str()) {
            anyhow::bail!(
                "Route collision: '{}' is defined by both {:?} and {:?}",
                route.pattern,
                existing,
                route.file
            );
        }
        pattern_seen.insert(&route.pattern, &route.file);
    }
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
            let replaced = s.replace('[', "{").replace(']', "}");
            pattern.push('/');
            pattern.push_str(&replaced.replace(".tsx", "").replace(".ts", ""));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bracket_route_strips_extension() {
        // [id].tsx should produce /{id}, not /{id}.tsx
        let base = Path::new("/routes");
        let file = base.join("[id].tsx");
        let pattern = file_to_pattern(base, &file).unwrap();
        assert_eq!(pattern, "/{id}");
    }

    #[test]
    fn test_bracket_route_with_nested_path() {
        let base = Path::new("/routes");
        let file = base.join("posts/[id].tsx");
        let pattern = file_to_pattern(base, &file).unwrap();
        assert_eq!(pattern, "/posts/{id}");
    }

    #[test]
    fn test_plain_route_still_works() {
        let base = Path::new("/routes");
        let file = base.join("about.tsx");
        let pattern = file_to_pattern(base, &file).unwrap();
        assert_eq!(pattern, "/about");
    }

    #[test]
    fn test_index_route() {
        let base = Path::new("/routes");
        let file = base.join("index.tsx");
        let pattern = file_to_pattern(base, &file).unwrap();
        assert_eq!(pattern, "/");
    }

    #[test]
    fn test_collision_prevention_per_component() {
        // posts/[id].tsx and posts/id.tsx should both produce /posts/{id} pattern
        // (the collision fix is in mod.rs for file_path generation, but we verify pattern gen)
        let base = Path::new("/routes");
        let file1 = base.join("posts/[id].tsx");
        let file2 = base.join("posts/id.tsx");
        let pattern1 = file_to_pattern(base, &file1).unwrap();
        let pattern2 = file_to_pattern(base, &file2).unwrap();
        // Both should produce valid patterns (extension stripped for bracket version)
        assert_eq!(pattern1, "/posts/{id}");
        assert_eq!(pattern2, "/posts/id");
    }
}
