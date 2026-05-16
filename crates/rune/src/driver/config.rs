//! # Rune Configuration
//!
//! Project configuration for the Rune compiler.

use std::path::Path;
use serde::{Deserialize, Serialize};

/// Project metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name
    pub name: String,
    /// Entry point
    #[serde(default)]
    pub entry: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "rune-project".to_string(),
            entry: None,
        }
    }
}

/// Build configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Target crate for hot reload
    #[serde(default = "default_target_crate")]
    pub target_crate: String,
    /// Host crate binary
    #[serde(default = "default_host_crate")]
    pub host_crate: String,
}

fn default_target_crate() -> String {
    "app".to_string()
}

fn default_host_crate() -> String {
    "host".to_string()
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            target_crate: default_target_crate(),
            host_crate: default_host_crate(),
        }
    }
}

/// Development configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevConfig {
    /// Enable hot reload
    #[serde(default)]
    pub hot_reload: bool,
    /// Debounce milliseconds
    #[serde(default = "default_debounce")]
    pub debounce: u64,
}

fn default_debounce() -> u64 {
    100
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            hot_reload: true,
            debounce: default_debounce(),
        }
    }
}

/// Release configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseConfig {
    /// Static binary mode
    #[serde(default)]
    pub static_binary: bool,
    /// Link-time optimization
    #[serde(default)]
    pub lto: bool,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            static_binary: true,
            lto: true,
        }
    }
}

/// Full Rune configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuneConfig {
    /// Project settings
    pub project: ProjectConfig,
    /// Build settings
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
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Save configuration to a file.
    ///
    /// # Errors
    /// Returns an error if the file cannot be written.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
}
