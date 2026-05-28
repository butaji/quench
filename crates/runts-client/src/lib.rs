//! Runts client-side runtime library
//!
//! This crate contains the TypeScript/JavaScript runtime for client-side
//! island hydration. The actual runtime code is in `src/runtime.ts` and
//! is served as static assets.

use std::path::PathBuf;

/// Get the path to the runtime.js file
pub fn runtime_js_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("dist")
        .join("runtime.js")
}

/// Get the path to the runtime.js.map file (source map)
pub fn runtime_js_map_path() -> Option<PathBuf> {
    let path = runtime_js_path().with_extension("js.map");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Get the runtime version
pub const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Whether to enable debug mode
    pub debug: bool,

    /// Default island hydration mode
    pub default_mode: String,

    /// Root path for island bundles
    pub bundles_path: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            debug: cfg!(debug_assertions),
            default_mode: "lazy".to_string(),
            bundles_path: "/_runts/islands".to_string(),
        }
    }
}

impl RuntimeConfig {
    /// Generate the inline script for runtime initialization
    pub fn init_script(&self) -> String {
        format!(
            r#"window.__RUNTS_CONFIG__ = {{
    debug: {},
    defaultMode: "{}",
    bundlesPath: "{}",
    version: "{}"
}};"#,
            self.debug, self.default_mode, self.bundles_path, RUNTIME_VERSION
        )
    }
}

/// Get the CDN URL for the runtime (if using CDN distribution)
pub fn runtime_cdn_url() -> String {
    format!(
        "https://cdn.jsdelivr.net/npm/runts-client@{}/dist/runtime.js",
        RUNTIME_VERSION
    )
}
