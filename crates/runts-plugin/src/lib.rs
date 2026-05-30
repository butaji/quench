//! Plugin system for runts compiler.
//!
//! Provides traits and types for extending runts with framework-specific
//! code generation and development server support.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Errors that can occur during plugin operations.
#[derive(Debug)]
pub struct PluginError {
    pub plugin: String,
    pub file: Option<String>,
    pub message: String,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{} codegen failed for {}: {}", self.plugin, file, self.message),
            None => write!(f, "{} codegen failed: {}", self.plugin, self.message),
        }
    }
}

impl std::error::Error for PluginError {}

impl PluginError {
    pub fn new(plugin: &str, file: &str, message: &str) -> Self {
        Self { plugin: plugin.to_string(), file: Some(file.to_string()), message: message.to_string() }
    }
    pub fn codegen(plugin: &str, file: &str, message: impl Into<String>) -> Self {
        Self { plugin: plugin.to_string(), file: Some(file.to_string()), message: message.into() }
    }
    pub fn dependency(plugin: &str, message: impl Into<String>) -> Self {
        Self { plugin: plugin.to_string(), file: None, message: message.into() }
    }
    pub fn dev(plugin: &str, message: impl Into<String>) -> Self {
        Self { plugin: plugin.to_string(), file: None, message: message.into() }
    }
    pub fn fatal(plugin: &str, message: impl Into<String>) -> Self {
        Self { plugin: plugin.to_string(), file: None, message: message.into() }
    }
}

/// A Rust dependency to include in generated projects.
#[derive(Debug, Clone)]
pub struct CargoDep {
    pub name: String,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub features: Vec<String>,
}

impl CargoDep {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: Some(version.to_string()),
            path: None,
            features: Vec::new(),
        }
    }
}

/// Context passed to plugin dev commands.
#[derive(Debug)]
pub struct DevContext {
    pub root: PathBuf,
    pub modules: Vec<String>,
}

/// Trait for framework-specific plugin state during development.
pub trait DevState: Send + Sync {}

/// Action returned by dev_run_once.
#[derive(Debug)]
pub enum DevAction {
    Continue,
    Stop,
    Error(String),
}

/// Plugin trait for framework integrations.
///
/// Each plugin provides:
/// - Code generation for modules
/// - Entry point generation
/// - Development server hooks
pub trait Plugin {
    /// Plugin identifier name.
    fn name(&self) -> &str;

    /// Help text describing the plugin.
    fn help_text(&self) -> &str;

    /// Generate Rust code for a single HIR module (as JSON string).
    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError>;

    /// Cargo dependencies required by this plugin.
    fn cargo_deps(&self) -> Vec<CargoDep>;

    /// Generate the entry point main.rs file.
    fn codegen_entry(&self, modules: &[hir::Module]) -> Result<String, PluginError>;

    /// Initialize development state.
    fn dev_init(&self, ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError>;

    /// Run one iteration of the development loop.
    fn dev_run_once(&self, state: &mut dyn DevState) -> Result<DevAction, PluginError>;

    /// Handle file reload event.
    fn dev_reload(&self, ctx: &mut DevContext, state: &mut dyn DevState) -> Result<(), PluginError>;
}

/// Route information for plugin code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    /// URL path pattern (e.g., "/", "/blog", "/blog/[slug]")
    pub path: String,
    /// HTTP methods supported
    pub methods: Vec<String>,
    /// Relative file path from project root
    pub file: String,
}

impl RouteInfo {
    pub fn new(path: &str, file: &str) -> Self {
        Self {
            path: path.to_string(),
            methods: Vec::new(),
            file: file.to_string(),
        }
    }
}

/// Extended HIR module type with route metadata
pub mod hir {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Module {
        /// Source file path (for route discovery)
        pub source_path: Option<String>,
        /// Route info if this module is a route
        pub route_info: Option<super::RouteInfo>,
        /// Raw HIR items as JSON Value (opaque to plugin trait, parsed by plugins)
        pub items_json: Option<serde_json::Value>,
    }

    impl Module {
        pub fn new() -> Self {
            Self {
                source_path: None,
                route_info: None,
                items_json: None,
            }
        }

        pub fn with_source_path(mut self, path: String) -> Self {
            self.source_path = Some(path);
            self
        }

        pub fn with_route_info(mut self, info: Option<super::RouteInfo>) -> Self {
            self.route_info = info;
            self
        }

        pub fn with_items_json(mut self, items: Option<serde_json::Value>) -> Self {
            self.items_json = items;
            self
        }
    }

    impl Default for Module {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_error_new() {
        let err = PluginError::new("fresh", "file.tsx", "parse failed");
        assert_eq!(err.plugin, "fresh");
        assert_eq!(err.file, Some("file.tsx".to_string()));
        assert!(err.message.contains("parse failed"));
    }

    #[test]
    fn test_plugin_error_display() {
        let err = PluginError::new("fresh", "file.tsx", "parse failed");
        let msg = format!("{}", err);
        assert!(msg.contains("fresh"));
        assert!(msg.contains("file.tsx"));
        assert!(msg.contains("parse failed"));
    }

    #[test]
    fn test_cargo_dep_new() {
        let dep = CargoDep::new("serde", "1.0");
        assert_eq!(dep.name, "serde");
        assert_eq!(dep.version, Some("1.0".to_string()));
    }

    #[test]
    fn test_route_info_new() {
        let route = RouteInfo::new("/blog/:slug", "blog/[slug].tsx");
        assert_eq!(route.path, "/blog/:slug");
        assert_eq!(route.file, "blog/[slug].tsx");
    }

    #[test]
    fn test_module_serialization() {
        let module = hir::Module {
            source_path: Some("test.tsx".to_string()),
            route_info: Some(RouteInfo::new("/", "index.tsx")),
            items_json: None,
        };
        let json = serde_json::to_string(&module).unwrap();
        assert!(json.contains("test.tsx"));
        assert!(json.contains("/"));
    }
}
