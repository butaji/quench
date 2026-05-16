//! # Cache Manager
//!
//! Manages the generated code cache in target/rune-cache/.

use std::path::{Path, PathBuf};
use std::fs;

/// Cache manager for generated code.
#[derive(Debug)]
pub struct CacheManager {
    /// Root cache directory
    root: PathBuf,
}

impl CacheManager {
    /// Create a new cache manager.
    ///
    /// # Errors
    /// Returns an error if the cache directory cannot be created.
    pub fn new(workspace: &Path) -> std::io::Result<Self> {
        let root = workspace.join("target/rune-cache");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Get the generated code directory.
    #[must_use]
    pub fn generated_dir(&self) -> PathBuf {
        self.root.join("generated")
    }

    /// Get the path for a generated module.
    #[must_use]
    pub fn module_path(&self, module_name: &str) -> PathBuf {
        self.generated_dir().join(format!("{module_name}.rs"))
    }

    /// Get the path to the generated Cargo.toml.
    #[must_use]
    pub fn generated_cargo_toml(&self) -> PathBuf {
        self.root.join("Cargo.toml")
    }

    /// Check if a source file needs regeneration.
    #[must_use]
    pub fn needs_regeneration(&self, source: &Path) -> bool {
        let source_mtime = fs::metadata(source)
            .and_then(|m| m.modified())
            .ok();

        let cache_mtime = fs::metadata(self.generated_cargo_toml())
            .and_then(|m| m.modified())
            .ok();

        match (source_mtime, cache_mtime) {
            (Some(st), Some(ct)) => st > ct,
            _ => true,
        }
    }

    /// Clean the cache.
    ///
    /// # Errors
    /// Returns an error if the cache cannot be cleaned.
    pub fn clean(&self) -> std::io::Result<()> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
            fs::create_dir_all(&self.root)?;
        }
        Ok(())
    }

    /// Get the hot reload directory.
    #[must_use]
    pub fn hot_dir(&self) -> PathBuf {
        self.root.parent()
            .unwrap_or(&self.root)
            .join("hot")
    }

    /// Get the current dylib symlink.
    #[must_use]
    pub fn current_dylib_link(&self) -> PathBuf {
        self.hot_dir().join(".current")
    }
}
