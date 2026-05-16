//! # Configuration
//!
//! Rune project configuration (rune.toml).

use std::path::Path;
use serde::{Deserialize, Serialize};

/// Rune project configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuneConfig {
    /// Project settings
    #[serde(default)]
    pub project: ProjectConfig,

    /// Build settings
    #[serde(default)]
    pub build: BuildConfig,

    /// Development settings
    #[serde(default)]
    pub dev: DevConfig,

    /// Release settings
    #[serde(default)]
    pub release: ReleaseConfig,
}

impl Default for RuneConfig {
    fn default() -> Self {
        Self {
            project: ProjectConfig::default(),
            build: BuildConfig::default(),
            dev: DevConfig::default(),
            release: ReleaseConfig::default(),
        }
    }
}

impl RuneConfig {
    /// Load configuration from a file.
    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Save configuration to a file.
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
}

/// Project settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name
    pub name: String,
    /// Entry point
    pub entry: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "rune-project".to_string(),
            entry: "src/main.r.ts".to_string(),
        }
    }
}

/// Build settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Target crate for hot reload
    pub target_crate: String,
    /// Host crate binary
    pub host_crate: String,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            target_crate: "app".to_string(),
            host_crate: "host".to_string(),
        }
    }
}

/// Development settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevConfig {
    /// Enable hot reload
    pub hot_reload: bool,
    /// Debounce milliseconds
    pub debounce: u64,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            hot_reload: true,
            debounce: 100,
        }
    }
}

/// Release settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseConfig {
    /// Static binary
    pub static_: bool,
    /// Link-time optimization
    pub lto: bool,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            static_: true,
            lto: true,
        }
    }
}

/// A target crate definition.
#[derive(Debug, Clone)]
pub struct TargetCrate {
    /// Crate name
    pub name: String,
    /// Crate path
    pub path: std::path::PathBuf,
    /// Whether this is the app crate
    pub is_app: bool,
    /// Whether this is the host crate
    pub is_host: bool,
}
