//! Layout composition for runts dev server
//!
//! Handles:
//! - Layout hierarchy detection
//! - Layout composition (outermost to innermost)
//! - _app.tsx global wrapper
//! - Layout context passing

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Layout information
#[derive(Debug, Clone)]
pub struct Layout {
    /// Layout file path
    pub file_path: PathBuf,
    
    /// Layout pattern (e.g., "blog" for routes/blog/_layout.tsx)
    pub pattern: String,
    
    /// Layout level (higher = more outermost)
    pub level: usize,
}

/// Layout manager for composing layouts
#[derive(Debug, Clone, Default)]
pub struct LayoutManager {
    layouts: Vec<Layout>,
    app_wrapper: Option<PathBuf>,
}

impl LayoutManager {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Build layout manager from routes directory
    pub fn from_routes_dir(routes_dir: &PathBuf) -> Result<Self> {
        let mut manager = Self::new();
        
        if !routes_dir.exists() {
            return Ok(manager);
        }
        
        for entry in walkdir::WalkDir::new(routes_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            
            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            
            if filename == "_layout.tsx" || filename == "_layout.ts" {
                let pattern = Self::extract_layout_pattern(path, routes_dir);
                let level = pattern.split('/').count();
                
                manager.layouts.push(Layout {
                    file_path: path.to_path_buf(),
                    pattern,
                    level,
                });
            }
            
            if filename == "_app.tsx" || filename == "_app.ts" {
                manager.app_wrapper = Some(path.to_path_buf());
            }
        }
        
        manager.layouts.sort_by(|a, b| b.level.cmp(&a.level));
        
        Ok(manager)
    }
    
    fn extract_layout_pattern(path: &Path, routes_dir: &Path) -> String {
        let path_str = path.to_str().unwrap_or("");
        let routes_str = routes_dir.to_str().unwrap_or("");
        
        let relative = path_str.strip_prefix(routes_str)
            .unwrap_or(path_str);
        
        let pattern = relative
            .trim_start_matches('/')
            .trim_end_matches("/_layout.tsx")
            .trim_end_matches("/_layout.ts")
            .trim_end_matches("_layout.tsx")
            .trim_end_matches("_layout.ts");
        
        if pattern.is_empty() {
            "/".to_string()
        } else {
            pattern.to_string()
        }
    }
    
    /// Find layouts that apply to a given route path
    pub fn find_layouts_for_path(&self, route_path: &str) -> Vec<Layout> {
        let route_segments: Vec<&str> = if route_path == "/" {
            vec![]
        } else {
            route_path.trim_start_matches('/').split('/').collect()
        };
        
        self.layouts
            .iter()
            .filter(|layout| {
                if layout.pattern == "/" {
                    return true;
                }
                
                let layout_segments: Vec<&str> = layout.pattern.split('/').collect();
                route_segments.starts_with(&layout_segments[..])
            })
            .cloned()
            .collect()
    }
    
    pub fn get_app_wrapper(&self) -> Option<&PathBuf> {
        self.app_wrapper.as_ref()
    }
    
    pub fn all_layouts(&self) -> &[Layout] {
        &self.layouts
    }
}

/// Layout context passed during rendering
#[derive(Debug, Clone, Default)]
pub struct LayoutContext {
    /// Shared state from middleware
    pub state: HashMap<String, serde_json::Value>,
    
    /// Layout-specific data
    pub layout_data: HashMap<String, serde_json::Value>,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn set_state(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.state.insert(key.into(), value);
    }
    
    pub fn get_state(&self, key: &str) -> Option<&serde_json::Value> {
        self.state.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_layout_pattern_extraction() {
        let routes_dir = PathBuf::from("/project/routes");
        
        let pattern = LayoutManager::extract_layout_pattern(
            &PathBuf::from("/project/routes/_layout.tsx"),
            &routes_dir
        );
        assert_eq!(pattern, "/");
        
        let pattern = LayoutManager::extract_layout_pattern(
            &PathBuf::from("/project/routes/blog/_layout.tsx"),
            &routes_dir
        );
        assert_eq!(pattern, "blog");
    }
    
    #[test]
    fn test_find_layouts_for_path() {
        let mut manager = LayoutManager::new();
        
        manager.layouts.push(Layout {
            file_path: PathBuf::from("routes/_layout.tsx"),
            pattern: "/".to_string(),
            level: 0,
        });
        
        manager.layouts.push(Layout {
            file_path: PathBuf::from("routes/blog/_layout.tsx"),
            pattern: "blog".to_string(),
            level: 1,
        });
        
        let layouts = manager.find_layouts_for_path("/blog");
        assert_eq!(layouts.len(), 2);
    }
}
