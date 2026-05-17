//! # Cache Manager
//!
//! Manages the generated code cache in target/rune-cache/ (outside workspace).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Cache manager for generated code.
#[derive(Debug)]
pub struct CacheManager {
    /// Root cache directory
    root: PathBuf,
    /// Original workspace path
    workspace: PathBuf,
}

impl CacheManager {
    /// Create a new cache manager.
    ///
    /// # Errors
    /// Returns an error if the cache directory cannot be created.
    pub fn new(workspace: &Path) -> std::io::Result<Self> {
        let root = workspace.join("target/rune-cache");
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            workspace: workspace.to_path_buf(),
        })
    }

    /// Get the workspace path.
    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Get the cache root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the generated code directory.
    #[must_use]
    pub fn generated_dir(&self) -> PathBuf {
        self.root.join("src/generated")
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

    /// Get modification time of a file, if it exists.
    fn mtime(path: &Path) -> Option<SystemTime> {
        fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Check if ANY source file is newer than the cache.
    pub fn needs_regeneration(&self, sources: &[PathBuf]) -> bool {
        let cache_mtime = Self::mtime(&self.generated_cargo_toml());
        Self::cache_missing_or_stale(sources, cache_mtime)
    }

    /// Check if cache is missing or source is newer.
    fn cache_missing_or_stale(sources: &[PathBuf], cache_mtime: Option<SystemTime>) -> bool {
        let Some(cache_modified) = cache_mtime else {
            return true;
        };
        Self::any_source_newer_than(sources, cache_modified)
    }

    /// Check if any source file is newer than cache.
    fn any_source_newer_than(sources: &[PathBuf], cache_modified: SystemTime) -> bool {
        sources
            .iter()
            .any(|src| Self::source_newer(src, cache_modified))
    }

    /// Check if a single source is newer than cache.
    fn source_newer(source: &Path, cache_modified: SystemTime) -> bool {
        Self::mtime(source).is_some_and(|modified| modified > cache_modified)
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
        self.root
            .parent()
            .map_or_else(|| PathBuf::from("target/hot"), |p| p.join("hot"))
    }

    /// Get the current dylib symlink.
    #[must_use]
    pub fn current_dylib_link(&self) -> PathBuf {
        self.hot_dir().join(".current")
    }
}
