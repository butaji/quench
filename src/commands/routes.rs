//! Route matching and dispatch for runts dev server
//!
//! Handles:
//! - Route pattern compilation
//! - Request matching
//! - Parameter extraction
//! - Handler dispatch

use anyhow::{anyhow, Result};
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;

/// HTTP methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

impl HttpMethod {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::GET),
            "POST" => Some(Self::POST),
            "PUT" => Some(Self::PUT),
            "DELETE" => Some(Self::DELETE),
            "PATCH" => Some(Self::PATCH),
            "HEAD" => Some(Self::HEAD),
            "OPTIONS" => Some(Self::OPTIONS),
            _ => None,
        }
    }
}

/// Route handler definition
#[derive(Debug, Clone)]
pub struct RouteHandler {
    pub method: HttpMethod,
    pub file_path: PathBuf,
}

/// Route definition
#[derive(Debug, Clone)]
pub struct Route {
    /// File path pattern (e.g., "blog/[slug]")
    pub pattern: String,
    
    /// Compiled regex for matching
    pub regex: Regex,
    
    /// URL path template (e.g., "/blog/:slug")
    pub path_template: String,
    
    /// Dynamic segment names (e.g., ["slug"])
    pub segments: Vec<String>,
    
    /// Route file path
    pub file_path: PathBuf,
    
    /// HTTP methods this route responds to
    pub methods: Vec<HttpMethod>,
    
    /// Is this a catch-all route?
    pub is_catch_all: bool,
}

impl Route {
    /// Create a new route from a file path
    pub fn from_file_path(file_path: &PathBuf) -> Result<Self> {
        let pattern = Self::file_path_to_pattern(file_path)?;
        let (path_template, segments, is_catch_all) = Self::pattern_to_template(&pattern);
        let regex = Self::compile_regex(&pattern)?;
        
        Ok(Self {
            pattern: pattern.clone(),
            regex,
            path_template,
            segments,
            file_path: file_path.clone(),
            methods: vec![HttpMethod::GET],
            is_catch_all,
        })
    }
    
    /// Convert file path to route pattern
    fn file_path_to_pattern(file_path: &PathBuf) -> Result<String> {
        let path = file_path
            .to_str()
            .ok_or_else(|| anyhow!("Invalid file path"))?;
        
        let pattern = if let Some(stripped) = path.strip_prefix("routes/") {
            stripped.trim_end_matches(".tsx")
                .trim_end_matches(".ts")
        } else {
            path.trim_end_matches(".tsx")
                .trim_end_matches(".ts")
        };
        
        let pattern = if pattern.ends_with("/index") {
            pattern.trim_end_matches("/index")
        } else if pattern == "index" {
            ""
        } else {
            pattern
        };
        
        Ok(pattern.to_string())
    }
    
    /// Convert pattern to URL template and extract segments
    fn pattern_to_template(pattern: &str) -> (String, Vec<String>, bool) {
        let mut template = String::new();
        let mut segments = Vec::new();
        let mut is_catch_all = false;
        
        if pattern.is_empty() {
            return ("/".to_string(), segments, false);
        }
        
        for segment in pattern.split('/') {
            if segment.starts_with('[') && segment.ends_with(']') {
                let inner = &segment[1..segment.len()-1];
                
                if inner.starts_with("...") {
                    let name = &inner[3..];
                    template.push_str(&format!("/{{{}}}", name));
                    segments.push(inner.to_string());
                    is_catch_all = true;
                } else {
                    template.push_str(&format!("/:{}", inner));
                    segments.push(inner.to_string());
                }
            } else {
                template.push_str(&format!("/{}", segment));
            }
        }
        
        (template, segments, is_catch_all)
    }
    
    /// Compile pattern to regex
    fn compile_regex(pattern: &str) -> Result<Regex> {
        let mut regex_str = String::from("^/?");
        
        if pattern.is_empty() {
            regex_str.push_str("?$");
        } else {
            let segments: Vec<&str> = pattern.split('/').collect();
            for (i, segment) in segments.iter().enumerate() {
                // Add / between segments (but not at the start)
                if i > 0 {
                    regex_str.push('/');
                }
                
                if segment.starts_with('[') && segment.ends_with(']') {
                    let inner = &segment[1..segment.len()-1];
                    
                    if inner.starts_with("...") {
                        regex_str.push_str("(.*)");
                    } else {
                        regex_str.push_str("([^/]+)");
                    }
                } else if !segment.is_empty() {
                    regex_str.push_str(&regex::escape(segment));
                }
            }
            regex_str.push('$');
        }
        
        Regex::new(&regex_str)
            .map_err(|e| anyhow!("Invalid route pattern '{}': {}", pattern, e))
    }
    
    /// Match a URL path against this route
    pub fn match_path(&self, path: &str) -> Option<HashMap<String, String>> {
        let path = path.trim_start_matches('/');
        
        let caps = self.regex.captures(path)?;
        let mut params = HashMap::new();
        
        for (i, name) in self.segments.iter().enumerate() {
            if let Some(cap) = caps.get(i + 1) {
                params.insert(name.clone(), cap.as_str().to_string());
            }
        }
        
        Some(params)
    }
    
    /// Check if this route matches a method
    pub fn matches_method(&self, method: HttpMethod) -> bool {
        self.methods.contains(&method)
    }
}

/// Route table for managing all routes
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    routes: Vec<Route>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }
    
    /// Add a route to the table
    pub fn add_route(&mut self, route: Route) {
        let insert_pos = if route.is_catch_all {
            self.routes.len()
        } else {
            self.routes.iter()
                .position(|r| r.is_catch_all)
                .unwrap_or(self.routes.len())
        };
        self.routes.insert(insert_pos, route);
    }
    
    /// Find a matching route for a path and method
    pub fn find_route(&self, path: &str, method: HttpMethod) -> Option<(&Route, HashMap<String, String>)> {
        for route in &self.routes {
            if let Some(params) = route.match_path(path) {
                if route.matches_method(method) {
                    return Some((route, params));
                }
            }
        }
        None
    }
    
    /// Get all routes
    pub fn all_routes(&self) -> &[Route] {
        &self.routes
    }
    
    /// Build route table from routes directory
    pub fn from_routes_dir(routes_dir: &PathBuf) -> Result<Self> {
        let mut table = Self::new();
        
        if !routes_dir.exists() {
            return Ok(table);
        }
        
        for entry in walkdir::WalkDir::new(routes_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            
            if let Some(ext) = path.extension() {
                if ext == "tsx" || ext == "ts" {
                    let filename = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    
                    if filename.starts_with('_') {
                        continue;
                    }
                    
                    let route = Route::from_file_path(&path.to_path_buf())?;
                    table.add_route(route);
                }
            }
        }
        
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    
    #[test]
    fn test_route_pattern_basic() {
        let route = Route::from_file_path(&PathBuf::from("routes/blog/index.tsx")).unwrap();
        assert_eq!(route.path_template, "/blog");
    }
    
    #[test]
    fn test_route_pattern_with_param() {
        let route = Route::from_file_path(&PathBuf::from("routes/blog/[slug].tsx")).unwrap();
        assert_eq!(route.path_template, "/blog/:slug");
        assert_eq!(route.segments, vec!["slug"]);
        
        let params = route.match_path("/blog/hello-world");
        assert!(params.is_some());
        let params = params.unwrap();
        assert_eq!(params.get("slug"), Some(&"hello-world".to_string()));
    }
    
    #[test]
    fn test_route_pattern_catch_all() {
        let route = Route::from_file_path(&PathBuf::from("routes/[...path].tsx")).unwrap();
        assert!(route.is_catch_all);
        
        let params = route.match_path("/api/users/123");
        assert!(params.is_some());
    }
    
    #[test]
    fn test_route_table_match() {
        let mut table = RouteTable::new();
        
        table.add_route(Route::from_file_path(&PathBuf::from("routes/index.tsx")).unwrap());
        table.add_route(Route::from_file_path(&PathBuf::from("routes/blog/[slug].tsx")).unwrap());
        table.add_route(Route::from_file_path(&PathBuf::from("routes/[...path].tsx")).unwrap());
        
        let (route, params) = table.find_route("/", HttpMethod::GET).unwrap();
        assert_eq!(route.pattern, "");
        
        let (route, params) = table.find_route("/blog/hello", HttpMethod::GET).unwrap();
        assert_eq!(route.pattern, "blog/[slug]");
        assert_eq!(params.get("slug"), Some(&"hello".to_string()));
        
        let (route, params) = table.find_route("/api/users/123", HttpMethod::GET).unwrap();
        assert!(route.is_catch_all);
    }
}
