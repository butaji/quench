//! A cache of compiled Node library modules across runs. Every process otherwise compiles the
//! library modules it loads from their embedded text: in the lodash npm scenario that is 60
//! modules, about 140 ms of a 450 ms run (x86_64 VM, 2026-10-19), while decoding a compiled
//! program takes about a tenth of its compilation. The runtime encodes a compiled program in
//! a compact form that names atoms by text and leaves out the source, and decodes it under the
//! loading VM's atoms (`Context::evaluate_specialized_static_body_cached_rooted`); this module
//! only keys and stores those bytes.
//!
//! An entry is keyed by the build that wrote it (the executable's path, size and modification
//! time, and the runtime ABI), the module's name and the exact text compiled. An entry that
//! does not decode, or matches nothing, only costs the compilation it would have saved. Writes
//! go to a temporary file renamed into place, so a concurrent reader sees a whole entry or none.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Names the cache directory; an empty value turns the cache off.
pub const COMPILE_CACHE_ENV: &str = "QUENCH_COMPILE_CACHE";
/// The cache's directory under the user's cache home (`XDG_CACHE_HOME`, or `~/.cache`).
const CACHE_SUBDIRECTORY: &str = "quench/compile";
/// The cache home under `HOME` when `XDG_CACHE_HOME` is unset.
const DEFAULT_CACHE_HOME: &str = ".cache";
const ENTRY_EXTENSION: &str = "qbc";

/// Where this process keeps compiled library modules, and the build identity that keys them.
pub(crate) struct CompileCache {
    directory: Option<PathBuf>,
    build: u64,
}

impl CompileCache {
    pub(crate) fn from_environment() -> Self {
        let directory = match std::env::var_os(COMPILE_CACHE_ENV) {
            Some(directory) if directory.is_empty() => None,
            Some(directory) => Some(PathBuf::from(directory)),
            None => std::env::var_os("XDG_CACHE_HOME")
                .filter(|home| !home.is_empty())
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(DEFAULT_CACHE_HOME)))
                .map(|home| home.join(CACHE_SUBDIRECTORY)),
        };
        let build = build_identity();
        Self { directory: build.and(directory), build: build.unwrap_or_default() }
    }

    /// The entry for compiling `parts` (the module's text, in order) under `name`.
    pub(crate) fn entry(&self, name: &str, parts: &[&str]) -> Option<CacheEntry> {
        let directory = self.directory.as_ref()?;
        // Two independently seeded hashes name the entry, so distinct inputs collide only if
        // both 64-bit hashes do.
        let hash = |seed: u8| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (seed, self.build, name).hash(&mut hasher);
            for part in parts {
                part.hash(&mut hasher);
            }
            hasher.finish()
        };
        Some(CacheEntry { path: directory.join(format!("{:016x}{:016x}.{ENTRY_EXTENSION}", hash(0), hash(1))) })
    }
}

/// One module's place in the cache.
pub(crate) struct CacheEntry {
    path: PathBuf,
}

impl CacheEntry {
    pub(crate) fn load(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.path).ok()
    }

    /// Stores `bytes`; a failure (a read-only or full disk) leaves the cache as it was.
    pub(crate) fn store(&self, bytes: &[u8]) {
        let Some(directory) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(directory).is_err() {
            return;
        }
        let temporary = self.path.with_extension(format!("{ENTRY_EXTENSION}.{}", std::process::id()));
        if std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, &self.path).is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

/// The running executable's path, size and modification time with the runtime ABI: a rebuild
/// changes it, so no build reads another's programs. None when the executable cannot be read.
fn build_identity() -> Option<u64> {
    let executable = std::env::current_exe().ok()?;
    let metadata = std::fs::metadata(&executable).ok()?;
    let modified = metadata.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (executable, metadata.len(), modified, quench_runtime::ResidualProgram::RUNTIME_ABI_FINGERPRINT).hash(&mut hasher);
    Some(hasher.finish())
}
