//! Plugin system for runts compiler.
//!
//! Provides traits and types for extending runts with framework-specific
//! code generation and development server support.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A Rust dependency to include in generated projects.
#[derive(Debug, Clone)]
pub struct CargoDep {
    pub name: String,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub features: Vec<String>,
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
    fn codegen_module(&self, hir_str: &str) -> anyhow::Result<String>;

    /// Cargo dependencies required by this plugin.
    fn cargo_deps(&self) -> Vec<CargoDep>;

    /// Generate the entry point main.rs file.
    fn codegen_entry(&self, modules: &[hir::Module]) -> anyhow::Result<String>;

    /// Initialize development state.
    fn dev_init(&self, ctx: &mut DevContext) -> anyhow::Result<Box<dyn DevState>>;

    /// Run one iteration of the development loop.
    fn dev_run_once(&self, state: &mut dyn DevState) -> anyhow::Result<DevAction>;

    /// Handle file reload event.
    fn dev_reload(&self, ctx: &mut DevContext, state: &mut dyn DevState) -> anyhow::Result<()>;
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

/// Extended HIR module type with route metadata
pub mod hir {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Module {
        /// Source file path (for route discovery)
        pub source_path: Option<String>,
        /// Route info if this module is a route
        pub route_info: Option<super::RouteInfo>,
    }

    impl Module {
        pub fn new() -> Self {
            Self {
                source_path: None,
                route_info: None,
            }
        }

        pub fn with_source_path(mut self, path: String) -> Self {
            self.source_path = Some(path);
            self
        }

        pub fn with_route_info(mut self, info: super::RouteInfo) -> Self {
            self.route_info = Some(info);
            self
        }
    }

    impl Default for Module {
        fn default() -> Self {
            Self::new()
        }
    }
}
