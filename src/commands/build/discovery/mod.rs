//! File discovery utilities

use std::path::{Path, PathBuf};

/// Find all TypeScript files in project
pub fn find_ts_files(project_root: &Path) -> Vec<PathBuf> {
    WalkDir::new(project_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "ts" || ext == "tsx").unwrap_or(false))
        .filter(|e| !is_excluded_subpath(project_root, e.path()))
        .filter(|e| !is_hidden_or_test_file(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Check if path should be excluded
pub fn is_excluded_subpath(project_root: &Path, path: &Path) -> bool {
    let rel = match path.strip_prefix(project_root) {
        Ok(r) => r,
        Err(_) => return true,
    };
    let parts: Vec<_> = rel.iter().collect();
    if parts.is_empty() || parts[0] == Path::new(".runts") || parts[0] == Path::new("target") || parts[0] == Path::new("node_modules") {
        return true;
    }
    is_excluded_dir(path) || is_excluded_name(path.file_name().and_then(|n| n.to_str()).unwrap_or(""))
}

/// Check if directory should be excluded
pub fn is_excluded_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("dist" | "build" | ".next" | ".nuxt" | "__pycache__" | ".git")
    )
}

/// Check if filename pattern should be excluded
pub fn is_excluded_name(name: &str) -> bool {
    name.starts_with('.')
        || name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.tsx")
        || name.starts_with("index.test.")
        || name.starts_with("index.spec.")
}

/// Check for hidden or test files
pub fn is_hidden_or_test_file(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        name.starts_with('.') || name.contains(".test.") || name.contains(".spec.")
    } else {
        false
    }
}

/// Check if path has hidden component
pub fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|c| matches!(c, std::path::Component::Normal(s) if s.to_str().map(|s| s.starts_with('.')).unwrap_or(false)))
}
