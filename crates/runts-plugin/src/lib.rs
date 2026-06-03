//! Plugin system for runts compiler.
//!
//! Provides traits and types for extending runts with framework-specific
//! code generation and development server support.

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

/// Errors that can occur during plugin operations.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("{plugin} codegen failed for {file}: {message}")]
    Codegen {
        plugin: String,
        file: String,
        message: String,
    },
    #[error("{plugin} dependency error: {message}")]
    Dependency { plugin: String, message: String },
    #[error("{plugin} dev error: {message}")]
    Dev { plugin: String, message: String },
    #[error("{plugin} fatal: {message}")]
    Fatal { plugin: String, message: String },
}

impl PluginError {
    pub fn new(plugin: &str, file: &str, message: &str) -> Self {
        Self::Codegen {
            plugin: plugin.to_string(),
            file: file.to_string(),
            message: message.to_string(),
        }
    }
    pub fn codegen(plugin: &str, file: &str, message: impl Into<String>) -> Self {
        Self::Codegen {
            plugin: plugin.to_string(),
            file: file.to_string(),
            message: message.into(),
        }
    }
    pub fn dependency(plugin: &str, message: impl Into<String>) -> Self {
        Self::Dependency {
            plugin: plugin.to_string(),
            message: message.into(),
        }
    }
    pub fn dev(plugin: &str, message: impl Into<String>) -> Self {
        Self::Dev {
            plugin: plugin.to_string(),
            message: message.into(),
        }
    }
    pub fn fatal(plugin: &str, message: impl Into<String>) -> Self {
        Self::Fatal {
            plugin: plugin.to_string(),
            message: message.into(),
        }
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
pub trait DevState: Send + Sync {
    /// Convert state to Any for safe downcasting.
    fn as_any(&self) -> &dyn Any;
    /// Convert state to Any for safe downcasting
    /// (mutable). Needed by `dev_reload` to mark
    /// state as dirty.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

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

    /// Plugin version string (e.g., "0.1.0").
    /// Default implementation returns "0.1.0".
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// Plugin capabilities bitfield.
    /// Default implementation returns no capabilities (basic codegen only).
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities::EMPTY
    }

    /// Generate Rust code for a single HIR module (as JSON string).
    fn codegen_module(&self, hir_str: &str) -> Result<String, PluginError>;

    /// Cargo dependencies required by this plugin.
    fn cargo_deps(&self) -> Vec<CargoDep>;

    /// Generate the entry point main.rs file.
    fn codegen_entry(&self, modules: &[hir::Module]) -> Result<String, PluginError>;

    /// Initialize development state.
    /// Default implementation returns a no-op dev state.
    fn dev_init(&self, _ctx: &mut DevContext) -> Result<Box<dyn DevState>, PluginError> {
        Ok(Box::new(DefaultDevState))
    }

    /// Run one iteration of the development loop.
    /// Default implementation returns Continue (no-op).
    fn dev_run_once(&self, _state: &mut dyn DevState) -> Result<DevAction, PluginError> {
        Ok(DevAction::Continue)
    }

    /// Handle file reload event.
    /// Default implementation does nothing.
    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Plugin capability flags for discovery and feature detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginCapabilities(u32);

impl PluginCapabilities {
    pub const EMPTY: Self = Self(0);
    pub const CODEGEN: Self = Self(1 << 0);
    pub const DEV_SERVER: Self = Self(1 << 1);
    pub const HOT_RELOAD: Self = Self(1 << 2);
    pub const TUI: Self = Self(1 << 3);
    pub const REACT: Self = Self(1 << 4);
    pub const FRESH: Self = Self(1 << 5);
    pub const RATATUI: Self = Self(1 << 6);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Default no-op dev state for plugins that don't need persistent state.
pub struct DefaultDevState;

impl DevState for DefaultDevState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Route information for plugin code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    /// URL path pattern (e.g., "/", "/blog", "/blog/[slug]")
    pub path: String,
    /// HTTP methods supported
    pub methods: Vec<String>,
    /// Relative file path from project root
    pub file_path: String,
}

impl RouteInfo {
    pub fn new(path: &str, file_path: &str) -> Self {
        Self {
            path: path.to_string(),
            methods: Vec::new(),
            file_path: file_path.to_string(),
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
        /// Raw HIR items as JSON Value (opaque to plugin trait, parsed by plugins).
        /// Named `items_json` internally but accepts `items` from core's HIR JSON.
        #[serde(alias = "items")]
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
        let msg = err.to_string();
        assert!(msg.contains("fresh"));
        assert!(msg.contains("file.tsx"));
        assert!(msg.contains("parse failed"));
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
        assert_eq!(route.file_path, "blog/[slug].tsx");
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

    #[test]
    fn test_hir_json_contract_items_alias() {
        // Core serializes HIR with "items" field, plugins expect "items_json".
        // This test verifies the alias works: JSON with "items" deserializes to items_json.
        let json_with_items = r#"{
            "source_path": "test.tsx",
            "items": [{"kind": "Import", "source": "react", "specifiers": [], "type_only": false}]
        }"#;
        let module: hir::Module = serde_json::from_str(json_with_items).unwrap();
        assert!(module.items_json.is_some(), "items_json should be populated from 'items' field");
        assert!(module.source_path.as_deref() == Some("test.tsx"));
    }

    #[test]
    fn test_hir_json_contract_round_trip() {
        // Test that serializing with items_json and deserializing works
        let items_value = serde_json::json!([
            {"kind": "Import", "source": "react", "specifiers": [], "type_only": false}
        ]);
        let module = hir::Module {
            source_path: Some("test.tsx".to_string()),
            route_info: None,
            items_json: Some(items_value.clone()),
        };
        let json = serde_json::to_string(&module).unwrap();
        let parsed: hir::Module = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.items_json, Some(items_value));
    }
}
