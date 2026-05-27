//! Configuration for runts projects

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Runts configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    /// Server configuration
    pub server: ServerConfig,

    /// Build configuration
    pub build: BuildConfig,

    /// Islands configuration
    pub islands: IslandsConfig,

    /// Dev server configuration
    pub dev: DevConfig,
    
    /// File watching configuration
    pub watch: WatchConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Patterns to ignore
    pub ignored: Vec<String>,
    
    /// Patterns to include
    #[serde(default)]
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub optimization: OptimizationConfig,
    #[serde(default = "default_true")]
    pub parallel: bool,
    #[serde(default = "default_true")]
    pub incremental: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    pub lto: bool,
    pub opt_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandsConfig {
    pub hydration: String, // "eager" or "lazy"
    pub serializer: String, // "json" or "msgpack"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevConfig {
    pub port: u16,
    pub open: bool,
    pub hmr: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                port: 8000,
                host: "127.0.0.1".to_string(),
            },
            build: BuildConfig {
                target: None,
                optimization: OptimizationConfig {
                    lto: true,
                    opt_level: "z".to_string(),
                },
                parallel: true,
                incremental: true,
            },
            islands: IslandsConfig {
                hydration: "eager".to_string(),
                serializer: "json".to_string(),
            },
            dev: DevConfig {
                port: 8000,
                open: true,
                hmr: true,
            },
            watch: WatchConfig {
                ignored: vec![
                    "**/node_modules/**".to_string(),
                    "**/target/**".to_string(),
                    "**/.git/**".to_string(),
                ],
                include: vec![
                    "routes/**".to_string(),
                    "islands/**".to_string(),
                    "components/**".to_string(),
                ],
            },
        }
    }
}

impl Config {
    /// Load configuration from a path (runts.config.ts or runts.config.json)
    #[allow(dead_code)]
    pub fn load(path: &PathBuf) -> Result<Self> {
        let config_path = Self::find_config(path)?;

        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).context("Failed to parse config")
        } else {
            Ok(Config::default())
        }
    }

    /// Load configuration from path, defaulting if not found
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        // Check for existing config
        let config_path = Self::find_config(path)?;
        
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            serde_json::from_str(&content).context("Failed to parse config")
        } else {
            // Return default config
            Ok(Config::default())
        }
    }

    /// Find configuration file path
    fn find_config(path: &PathBuf) -> Result<PathBuf> {
        // Try runts.config.json first
        let json_config = path.join("runts.config.json");
        if json_config.exists() {
            return Ok(json_config);
        }

        // Try runts.config.yaml
        let yaml_config = path.join("runts.config.yaml");
        if yaml_config.exists() {
            return Ok(yaml_config);
        }

        // Try runts.config.ts (TypeScript - would need parsing)
        let ts_config = path.join("runts.config.ts");
        if ts_config.exists() {
            // For now, return a fallback - TS config requires separate parsing
            // In a full implementation, we'd parse the TS file
            return Ok(json_config);
        }

        // Return default path
        Ok(path.join("runts.config.json"))
    }
}
