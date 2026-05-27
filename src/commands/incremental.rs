//! Incremental build cache system
//!
//! Caches transpilation results (TS/TSX → Rust source) between builds.
//! Only re-transpiles files whose content hash has changed.
//!
//! Cache location: `.runts/cache/build_cache.json`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::commands::build::{ComponentEntry, GeneratedFile, IslandEntry, RouteEntry};

/// Current cache format version. Bump when serialization format changes.
const CACHE_VERSION: &str = "2";

/// Cache key that includes runts version and codegen logic fingerprint.
/// Changing this invalidates all cached entries.
fn cache_environment_key() -> String {
    // In a real release build this would include the crate version.
    format!("runts-v0.5.0-{}", CACHE_VERSION)
}

/// Build cache entry for a single source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// SHA-256 hash of the source file content.
    pub content_hash: String,

    /// Relative path from project root to the generated Rust file.
    pub generated_path: PathBuf,

    /// Generated Rust source code.
    pub generated_content: String,

    /// Optional route metadata (for files in routes/).
    pub route: Option<RouteEntry>,

    /// Optional island metadata (for files in islands/).
    pub island: Option<IslandEntry>,

    /// Optional component metadata (for files in components/).
    pub component: Option<ComponentEntry>,

    /// Unix timestamp of when this entry was created.
    pub timestamp: u64,
}

/// On-disk cache representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildCache {
    /// Environment key; if mismatched, the entire cache is discarded.
    pub env_key: String,

    /// Map from relative source path → cache entry.
    pub entries: HashMap<String, CacheEntry>,
}

impl BuildCache {
    /// Create an empty cache for the current environment.
    pub fn new() -> Self {
        Self {
            env_key: cache_environment_key(),
            entries: HashMap::new(),
        }
    }

    /// Load cache from disk, or return a fresh cache if invalid/missing.
    pub fn load(project_root: &Path) -> Self {
        let path = cache_path(project_root);
        if !path.exists() {
            debug!("No incremental cache found at {:?}", path);
            return Self::new();
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read cache: {}. Starting fresh.", e);
                return Self::new();
            }
        };

        let cache: BuildCache = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                warn!("Cache JSON corrupted: {}. Starting fresh.", e);
                return Self::new();
            }
        };

        let expected = cache_environment_key();
        if cache.env_key != expected {
            info!(
                "Cache environment mismatch ({} vs {}). Rebuilding from scratch.",
                cache.env_key, expected
            );
            return Self::new();
        }

        info!("Loaded incremental cache with {} entries", cache.entries.len());
        cache
    }

    /// Persist cache to disk.
    pub fn save(&self, project_root: &Path) -> Result<()> {
        let path = cache_path(project_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache directory {:?}", parent))?;
        }

        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize build cache")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write cache to {:?}", path))?;

        debug!("Saved incremental cache to {:?}", path);
        Ok(())
    }

    /// Check whether a source file (by relative path and current hash) is cached.
    pub fn is_fresh(&self, rel_path: &str, current_hash: &str) -> bool {
        self.entries
            .get(rel_path)
            .map(|e| e.content_hash == current_hash)
            .unwrap_or(false)
    }

    /// Get a cached entry.
    pub fn get(&self, rel_path: &str) -> Option<&CacheEntry> {
        self.entries.get(rel_path)
    }

    /// Insert or update a cache entry.
    pub fn insert(&mut self, rel_path: String, entry: CacheEntry) {
        self.entries.insert(rel_path, entry);
    }

    /// Remove stale entries whose source files no longer exist.
    pub fn prune_missing(&mut self, existing_files: &[String]) {
        let existing: std::collections::HashSet<_> = existing_files.iter().cloned().collect();
        self.entries.retain(|k, _| existing.contains(k));
    }
}

impl Default for BuildCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a SHA-256 hex digest of file contents.
pub fn compute_file_hash(path: &Path) -> Result<String> {
    use sha2::Digest;
    use std::io::Read;

    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open {:?} for hashing", path))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let n = file.read(&mut buffer)
            .with_context(|| format!("Failed to read {:?}", path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute hash from an in-memory string (used by parallel builder).
pub fn compute_hash(source: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(source.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Return the path to the on-disk cache file.
pub fn cache_path(project_root: &Path) -> PathBuf {
    project_root.join(".runts").join("cache").join("build_cache.json")
}

/// Result of an incremental file processing step.
pub enum FileProcessResult {
    /// File was unchanged; cached artifacts are valid.
    Cached,
    /// File was changed (or new); transpilation was performed.
    Fresh {
        generated: GeneratedFile,
        route: Option<RouteEntry>,
        island: Option<IslandEntry>,
        component: Option<ComponentEntry>,
    },
}

/// Statistics about an incremental build.
#[derive(Debug, Default, Clone)]
pub struct IncrementalStats {
    pub files_total: usize,
    pub files_cached: usize,
    pub files_changed: usize,
    pub routes_cached: usize,
    pub routes_changed: usize,
    pub islands_cached: usize,
    pub islands_changed: usize,
    pub components_cached: usize,
    pub components_changed: usize,
}

impl IncrementalStats {
    pub fn summary(&self) -> String {
        format!(
            "Incremental build: {} total, {} cached, {} changed (routes: {}/{}, islands: {}/{}, components: {}/{})",
            self.files_total,
            self.files_cached,
            self.files_changed,
            self.routes_cached,
            self.routes_changed,
            self.islands_cached,
            self.islands_changed,
            self.components_cached,
            self.components_changed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_compute_hash() {
        let h1 = compute_hash("hello world");
        let h2 = compute_hash("hello world");
        let h3 = compute_hash("hello world!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_compute_file_hash() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        {
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(b"incremental test").unwrap();
        }
        let h = compute_file_hash(&path).unwrap();
        assert_eq!(h.len(), 64);

        // Same content → same hash
        let h2 = compute_file_hash(&path).unwrap();
        assert_eq!(h, h2);
    }

    #[test]
    fn test_cache_save_load() {
        let dir = TempDir::new().unwrap();
        let mut cache = BuildCache::new();
        cache.insert(
            "routes/index.tsx".to_string(),
            CacheEntry {
                content_hash: "abc123".to_string(),
                generated_path: PathBuf::from("src/gen/routes/index.rs"),
                generated_content: "pub fn handler() {}".to_string(),
                route: Some(RouteEntry {
                    pattern: "/".to_string(),
                    path: PathBuf::from("routes/index.tsx"),
                    file: "index.tsx".to_string(),
                    params: vec![],
                    methods: vec![super::super::build::HttpMethod::GET],
                    component_name: Some("Home".to_string()),
                }),
                island: None,
                component: None,
                timestamp: 1234567890,
            },
        );

        cache.save(dir.path()).unwrap();
        let loaded = BuildCache::load(dir.path());
        assert_eq!(loaded.entries.len(), 1);
        assert!(loaded.is_fresh("routes/index.tsx", "abc123"));
        assert!(!loaded.is_fresh("routes/index.tsx", "different"));
    }

    #[test]
    fn test_cache_env_mismatch_clears() {
        let dir = TempDir::new().unwrap();
        let mut cache = BuildCache::new();
        cache.env_key = "old-version".to_string();
        cache.save(dir.path()).unwrap();

        let loaded = BuildCache::load(dir.path());
        assert!(loaded.entries.is_empty());
        assert_eq!(loaded.env_key, cache_environment_key());
    }

    #[test]
    fn test_cache_prune_missing() {
        let mut cache = BuildCache::new();
        cache.insert("a.tsx".to_string(), dummy_entry("a"));
        cache.insert("b.tsx".to_string(), dummy_entry("b"));
        cache.insert("c.tsx".to_string(), dummy_entry("c"));

        cache.prune_missing(&["a.tsx".to_string(), "c.tsx".to_string()]);
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.contains_key("a.tsx"));
        assert!(!cache.entries.contains_key("b.tsx"));
        assert!(cache.entries.contains_key("c.tsx"));
    }

    fn dummy_entry(hash: &str) -> CacheEntry {
        CacheEntry {
            content_hash: hash.to_string(),
            generated_path: PathBuf::from("src/gen/dummy.rs"),
            generated_content: String::new(),
            route: None,
            island: None,
            component: None,
            timestamp: 0,
        }
    }
}
