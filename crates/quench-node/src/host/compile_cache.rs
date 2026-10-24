//! A cache of compiled host programs across runs: Node's library modules and the host's own
//! scripts. Every process otherwise compiles them from their embedded text: in the lodash npm
//! scenario that is 60 library modules (about 140 ms of a 450 ms run) and 126 host scripts
//! (about 60 ms, x86_64 VM, 2026-10-19), while decoding a compiled program takes about a tenth
//! of its compilation. The runtime encodes a compiled program in a compact form that names
//! atoms by text and leaves out the source, and decodes it under the loading VM's atoms
//! (`Host::kept_compiled_program`); this module only keys and stores those bytes.
//!
//! An entry is keyed by the build that wrote it (the executable's path, size and modification
//! time, and the runtime ABI), the module's name and the exact text compiled. An entry that
//! does not decode, or matches nothing, only costs the compilation it would have saved. Writes
//! go to a temporary file renamed into place, so a concurrent reader sees a whole entry or none.
//! Each build's entries share a directory under the cache's; the first store of a build removes
//! all but the `KEPT_BUILDS` most recent builds' directories, which no process of the new build
//! reads (they had grown to 243K files, 6.6 GB, over a day of rebuilds).

use std::hash::{BuildHasher, Hash, Hasher};
use std::path::PathBuf;

/// Names the cache directory; an empty value turns the cache off.
pub const COMPILE_CACHE_ENV: &str = "QUENCH_COMPILE_CACHE";
/// The cache's directory under the user's cache home (`XDG_CACHE_HOME`, or `~/.cache`).
const CACHE_SUBDIRECTORY: &str = "quench/compile";
/// The cache home under `HOME` when `XDG_CACHE_HOME` is unset.
const DEFAULT_CACHE_HOME: &str = ".cache";
const ENTRY_EXTENSION: &str = "qbc";
/// The seeds of the two hashes that name an entry (`CompileCache::entry`): any two distinct
/// values give independent hashes.
const KEY_SEEDS: [u64; 2] = [0, 1];
/// The builds whose compiled programs and startup snapshots a cache keeps: the running build
/// and the two before it, which an older executable still installed may run.
pub(crate) const KEPT_BUILDS: usize = 3;

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

    /// The cache's directory, when it has one.
    pub(crate) fn directory(&self) -> Option<&std::path::Path> {
        self.directory.as_deref()
    }

    /// The identity of the build that keys the cache (`build_identity`).
    pub(crate) fn build(&self) -> u64 {
        self.build
    }

    /// The entry for compiling `parts` (the module's text, in order) under `name`.
    pub(crate) fn entry(&self, name: &str, parts: &[&str]) -> Option<CacheEntry> {
        let directory = self.directory.as_ref()?;
        // Two independently seeded hashes name the entry, so distinct inputs collide only if
        // both 64-bit hashes do. The inputs are the user's own library and package files, which
        // need no collision resistance against an adversary; SipHash had cost 2% of a small
        // npm scenario, hashing each module's text twice.
        let hash = |seed: u64| {
            let mut hasher = foldhash::quality::FixedState::with_seed(seed).build_hasher();
            (self.build, name).hash(&mut hasher);
            for part in parts {
                part.hash(&mut hasher);
            }
            hasher.finish()
        };
        let build = directory.join(format!("{:016x}", self.build));
        Some(CacheEntry { path: build.join(format!("{:016x}{:016x}.{ENTRY_EXTENSION}", hash(KEY_SEEDS[0]), hash(KEY_SEEDS[1]))) })
    }
}

/// One module's place in the cache.
pub(crate) struct CacheEntry {
    path: PathBuf,
}

impl CacheEntry {
    /// The entry's bytes, mapped from its file: a program decoded from them reads each
    /// function's record on its first call, so the pages of records never called stay out of
    /// memory. Entries are replaced by rename, never written in place, so the mapped file
    /// keeps its contents.
    pub(crate) fn load(&self) -> Option<quench_runtime::KeptBytes> {
        map_file(&self.path)
    }

    /// Stores `bytes`; a failure (a read-only or full disk) leaves the cache as it was.
    pub(crate) fn store(&self, bytes: &[u8]) {
        let Some(directory) = self.path.parent() else {
            return;
        };
        let new_build = !directory.exists();
        if std::fs::create_dir_all(directory).is_err() {
            return;
        }
        if new_build {
            if let Some(builds) = directory.parent() {
                remove_other_builds(builds);
            }
        }
        let temporary = self.path.with_extension(format!("{ENTRY_EXTENSION}.{}", std::process::id()));
        if std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, &self.path).is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

/// Removes from `builds`, a cache directory of per-build directories, all but the `KEPT_BUILDS`
/// most recently modified builds, and any entry kept outside a build's directory, as the cache
/// stored them before it kept builds apart.
pub(crate) fn remove_other_builds(builds: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(builds) else {
        return;
    };
    // Entries are classified by their own type, never a link's target, and a build directory
    // is removed with `remove_dir_all`, which removes links without following them: nothing
    // outside `builds` is removed. An entry another process removes first is skipped.
    let mut kept = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) {
                kept.push((modified, entry.path()));
            }
        } else {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    kept.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, build) in kept.into_iter().skip(KEPT_BUILDS) {
        let _ = std::fs::remove_dir_all(build);
    }
}

/// The file at `path`, mapped where the platform maps files; files the cache and snapshots
/// keep are replaced by rename, never written in place, so a mapping keeps its contents.
#[cfg(unix)]
pub(crate) fn map_file(path: &std::path::Path) -> Option<quench_runtime::KeptBytes> {
    MappedFile::open(path).map(|file| std::rc::Rc::new(file) as quench_runtime::KeptBytes)
}

#[cfg(not(unix))]
pub(crate) fn map_file(path: &std::path::Path) -> Option<quench_runtime::KeptBytes> {
    std::fs::read(path).ok().map(|bytes| std::rc::Rc::new(bytes) as quench_runtime::KeptBytes)
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

/// A whole file mapped read-only and private, unmapped when dropped.
#[cfg(unix)]
struct MappedFile {
    address: std::ptr::NonNull<u8>,
    length: usize,
}

#[cfg(unix)]
impl MappedFile {
    fn open(path: &std::path::Path) -> Option<Self> {
        use std::os::fd::AsRawFd;
        let file = std::fs::File::open(path).ok()?;
        let length = usize::try_from(file.metadata().ok()?.len()).ok().filter(|length| *length > 0)?;
        // The descriptor may close once the mapping exists.
        let address = crate::host::resident_pages::map_read_only_file(file.as_raw_fd(), length)?;
        Some(Self { address: std::ptr::NonNull::new(address as *mut u8)?, length })
    }
}

#[cfg(unix)]
impl AsRef<[u8]> for MappedFile {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: the mapping covers `length` readable bytes for as long as `self` lives.
        unsafe { std::slice::from_raw_parts(self.address.as_ptr(), self.length) }
    }
}

#[cfg(unix)]
impl Drop for MappedFile {
    fn drop(&mut self) {
        // Nothing borrows the mapping past `self`.
        crate::host::resident_pages::unmap_tracked_mapping(self.address.as_ptr() as usize, self.length);
    }
}

#[cfg(test)]
mod tests {
    use super::{remove_other_builds, KEPT_BUILDS};

    #[test]
    fn a_new_build_keeps_the_most_recent_builds_only() {
        let root = std::env::temp_dir().join(format!("quench-compile-builds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builds = (0..KEPT_BUILDS + 2).map(|build| root.join(format!("{build:016x}"))).collect::<Vec<_>>();
        for build in &builds {
            std::fs::create_dir_all(build).unwrap();
            std::fs::write(build.join("entry.qbc"), b"").unwrap();
            // Directory times order the builds; filesystems keep them at least this finely.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::write(root.join("legacy.qbc"), b"").unwrap();
        // A link out of the cache, to a directory and to a file, loses only the link.
        let outside = std::env::temp_dir().join(format!("quench-compile-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked-directory")).unwrap();
        std::os::unix::fs::symlink(outside.join("keep"), root.join("linked-file")).unwrap();
        remove_other_builds(&root);
        assert!(outside.join("keep").exists());
        assert!(std::fs::symlink_metadata(root.join("linked-directory")).is_err());
        std::fs::remove_dir_all(&outside).unwrap();
        let kept = builds.iter().filter(|build| build.exists()).collect::<Vec<_>>();
        assert_eq!(kept, builds.iter().rev().take(KEPT_BUILDS).rev().collect::<Vec<_>>());
        assert!(!root.join("legacy.qbc").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
