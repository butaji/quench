//! # Cache Manager
//!
//! Manages the rune-cache directory for generated code.

use std::path::PathBuf;

/// Manages the cache directory.
pub struct CacheManager {
    /// Base workspace path
    workspace: PathBuf,
}

impl CacheManager {
    /// Create a new cache manager.
    pub fn new(workspace: &std::path::Path) -> std::io::Result<Self> {
        let cache = Self {
            workspace: workspace.to_path_buf(),
        };
        cache.ensure_directories()?;
        Ok(cache)
    }

    /// Get the cache root directory.
    pub fn cache_dir(&self) -> PathBuf {
        self.workspace.join("target/rune-cache")
    }

    /// Get the generated code directory.
    pub fn generated_dir(&self) -> PathBuf {
        self.cache_dir().join("generated")
    }

    /// Get the generated Cargo.toml path.
    pub fn generated_cargo_toml(&self) -> PathBuf {
        self.cache_dir().join("Cargo.toml")
    }

    /// Get the hot reload directory.
    pub fn hot_dir(&self) -> PathBuf {
        self.workspace.join("target/hot")
    }

    /// Get the current dylib symlink path.
    pub fn current_dylib(&self) -> PathBuf {
        self.hot_dir().join(".current")
    }

    /// Ensure all cache directories exist.
    fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.cache_dir())?;
        std::fs::create_dir_all(self.generated_dir())?;
        std::fs::create_dir_all(self.hot_dir())?;
        Ok(())
    }

    /// Clean the cache.
    pub fn clean(&self) -> std::io::Result<()> {
        if self.cache_dir().exists() {
            std::fs::remove_dir_all(self.cache_dir())?;
        }
        self.ensure_directories()?;
        Ok(())
    }

    /// Get the path to a specific generated module.
    pub fn module_path(&self, module_name: &str) -> PathBuf {
        self.generated_dir().join(format!("{}.rs", module_name))
    }

    /// Check if cache is valid for given sources.
    pub fn is_valid(&self, sources: &[PathBuf]) -> bool {
        if !self.generated_cargo_toml().exists() {
            return false;
        }

        for source in sources {
            if !source.exists() {
                continue;
            }

            let source_mtime = match std::fs::metadata(source) {
                Ok(m) => m.modified().ok(),
                Err(_) => None,
            };

            let cache_mtime = match std::fs::metadata(self.generated_cargo_toml()) {
                Ok(m) => m.modified().ok(),
                Err(_) => None,
            };

            if let (Some(src), Some(cache)) = (source_mtime, cache_mtime) {
                if src > cache {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}
