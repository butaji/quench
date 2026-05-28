//! Route table management

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use regex::Regex;
use walkdir::WalkDir;

/// HTTP method
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpMethod {
    GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS
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

/// Route information
#[derive(Debug, Clone)]
pub struct Route {
    pub pattern: String,
    pub regex: Regex,
    pub file_path: PathBuf,
    pub methods: Vec<HttpMethod>,
}

/// Route table for fast lookup
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    routes: Vec<Route>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    pub fn from_routes_dir(routes_dir: &PathBuf) -> Result<Self> {
        let mut table = Self::new();
        if routes_dir.exists() {
            Self::scan_dir(routes_dir, routes_dir, &mut table)?;
        }
        Ok(table)
    }

    fn scan_dir(base: &Path, current: &Path, table: &mut Self) -> Result<()> {
        for entry in WalkDir::new(current).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                Self::scan_dir(base, path, table)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("tsx") ||
                      path.extension().and_then(|e| e.to_str()) == Some("ts") {
                if let Some(pattern) = Self::file_to_pattern(base, path) {
                    table.add_route(pattern, path.to_path_buf());
                }
            }
        }
        Ok(())
    }

    fn file_to_pattern(base: &Path, file: &Path) -> Option<String> {
        let rel = file.strip_prefix(base).ok()?;
        let parts: Vec<_> = rel.components().collect();
        if parts.len() < 2 || parts[0].as_os_str() != "routes" {
            return None;
        }

        let mut pattern = String::new();
        for part in parts.iter().skip(1) {
            let s = part.as_os_str().to_string_lossy();
            if s.starts_with('[') {
                pattern.push_str(&s.replace('[', "{").replace(']', "}"));
            } else if s != "index.tsx" && s != "index.ts" {
                pattern.push('/');
                pattern.push_str(&s.replace(".tsx", "").replace(".ts", ""));
            }
        }
        if pattern.is_empty() {
            pattern = "/".to_string();
        }
        Some(pattern)
    }

    pub fn add_route(&mut self, pattern: String, file_path: PathBuf) {
        let regex_pattern = pattern
            .replace("{slug}", r"(?P<slug>[^/]+)")
            .replace("{id}", r"(?P<id>[^/]+)")
            .replace("/", "\\/");

        let regex = Regex::new(&format!("^{}$", regex_pattern)).ok();
        if let Some(regex) = regex {
            self.routes.push(Route {
                pattern,
                regex,
                file_path,
                methods: vec![HttpMethod::GET],
            });
        }
    }

    pub fn find_route(&self, path: &str) -> Option<&Route> {
        self.routes.iter().find(|r| r.regex.is_match(path))
    }

    pub fn routes(&self) -> &[Route] {
        &self.routes
    }
}
