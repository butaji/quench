//! The Node host's part of a startup snapshot (`quench_runtime::Runtime::build_snapshot`):
//! the host state a bootstrapped VM's heap is bound to. A snapshot is taken at one point of
//! `NodeHost::initialize`, before anything of the process's own (its options, its entry, its
//! IPC channel) is installed; a restored host continues from there (`Host::resume`).
//!
//! Every field of the host is either written here (the guest handles the bootstrap cached),
//! the restoring process's own (its arguments, descriptors, clocks and channels), or required
//! empty at the snapshot point (sockets, timers, pending work), so a field added later does
//! not compile until it is given one of those places.

mod entry_graph;

use quench_runtime::{HostWeakRoot, RootId, SnapshotReader, SnapshotWriter};
use std::collections::HashMap;
use std::hash::Hash;

/// A host value a snapshot writes and reads whole.
pub(crate) trait HostSnap: Sized {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String>;
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String>;
}

macro_rules! host_snap_primitive {
    ($($ty:ty: $write:ident, $read:ident;)*) => {$(
        impl HostSnap for $ty {
            fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
                writer.$write(*self);
                Ok(())
            }
            fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
                reader.$read()
            }
        }
    )*};
}

host_snap_primitive! {
    bool: bool, bool;
    u8: u8, u8;
    u32: u32, u32;
    u64: u64, u64;
    f64: f64, f64;
}

impl HostSnap for usize {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.len(*self);
        Ok(())
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        reader.len()
    }
}

impl HostSnap for String {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.str(self);
        Ok(())
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        reader.string()
    }
}

impl HostSnap for std::path::PathBuf {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.str(self.to_str().ok_or("a snapshot path is not Unicode")?);
        Ok(())
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        reader.string().map(Self::from)
    }
}

impl HostSnap for RootId {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.root(*self)
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        reader.root()
    }
}

impl HostSnap for HostWeakRoot {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.weak_root(self)
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        reader.weak_root()
    }
}

impl<T: HostSnap> HostSnap for Option<T> {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.bool(self.is_some());
        self.as_ref().map_or(Ok(()), |value| value.write(writer))
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        if reader.bool()? { T::read(reader).map(Some) } else { Ok(None) }
    }
}

impl<T: HostSnap> HostSnap for Vec<T> {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.len(self.len());
        self.iter().try_for_each(|value| value.write(writer))
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        (0..reader.len()?).map(|_| T::read(reader)).collect()
    }
}

impl<A: HostSnap, B: HostSnap> HostSnap for (A, B) {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        self.0.write(writer)?;
        self.1.write(writer)
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        Ok((A::read(reader)?, B::read(reader)?))
    }
}

impl<K: HostSnap + Eq + Hash, V: HostSnap, S: std::hash::BuildHasher + Default> HostSnap for HashMap<K, V, S> {
    fn write(&self, writer: &mut SnapshotWriter<'_>) -> Result<(), String> {
        writer.len(self.len());
        self.iter().try_for_each(|(key, value)| {
            key.write(writer)?;
            value.write(writer)
        })
    }
    fn read(reader: &mut SnapshotReader<'_, '_>) -> Result<Self, String> {
        let len = reader.len()?;
        let mut map = Self::with_capacity_and_hasher(len, S::default());
        for _ in 0..len {
            let key = K::read(reader)?;
            map.insert(key, V::read(reader)?);
        }
        Ok(map)
    }
}

/// `HostSnap` for a struct, field by field; every field must be listed.
macro_rules! host_snap_struct {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::host::startup_snapshot::HostSnap for $ty {
            fn write(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
                let Self { $($field),* } = self;
                $( $crate::host::startup_snapshot::HostSnap::write($field, writer)?; )*
                Ok(())
            }
            fn read(reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<Self, String> {
                Ok(Self { $($field: $crate::host::startup_snapshot::HostSnap::read(reader)?),* })
            }
        }
    };
}
pub(crate) use host_snap_struct;

/// Refuses a snapshot unless `quiescent`: the named state must be empty at the snapshot point.
pub(crate) fn require_quiescent(quiescent: bool, what: &str) -> Result<(), String> {
    if quiescent { Ok(()) } else { Err(format!("startup snapshot taken with {what} in use")) }
}

/// Snapshot methods for a state each of whose fields must be empty at the snapshot point:
/// the snapshot writes nothing for it, and a restored process keeps its own, fresh one.
macro_rules! host_snap_empty {
    ($ty:ty { $($field:ident),* $(,)? } $what:literal) => {
        impl $ty {
            pub(crate) fn write_snapshot(&self, _: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
                let Self { $($field),* } = self;
                $crate::host::startup_snapshot::require_quiescent(true $(&& $field.is_empty())*, $what)
            }
            pub(crate) fn read_snapshot(&mut self, _: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
                Ok(())
            }
        }
    };
}
pub(crate) use host_snap_empty;

/// Snapshot methods for a state written whole, each field by its `HostSnap`.
macro_rules! host_snap_state {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        $crate::host::startup_snapshot::host_snap_struct!($ty { $($field),* });
        impl $ty {
            pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
                $crate::host::startup_snapshot::HostSnap::write(self, writer)
            }
            pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
                *self = $crate::host::startup_snapshot::HostSnap::read(reader)?;
                Ok(())
            }
        }
    };
}
pub(crate) use host_snap_state;

/// Environment variables that give the process a channel of its own (an IPC descriptor): a
/// process that sets any boots without a snapshot.
const REFUSED_VARIABLES: &[&str] = &["NODE_CHANNEL_FD", "NODE_CHANNEL_SERIALIZATION_MODE"];
/// Environment variables the bootstrap reads before the snapshot point: they key the
/// snapshot, which a builder started with this process's environment bootstraps under.
const KEYED_VARIABLES: &[&str] = &[
    "HOME",
    "NODE_OPTIONS",
    "NODE_PENDING_DEPRECATION",
    "NODE_DEBUG",
    "NODE_PATH",
    "QUENCH_PLATFORM",
];
/// How eligible processes use a snapshot (`SnapshotUse`): `on` always, `trace` always and
/// reporting why one was not used, `off` never; otherwise by what the entry loads.
pub(crate) const SNAPSHOT_ENV: &str = "QUENCH_STARTUP_SNAPSHOT";
const SNAPSHOT_ON: &str = "on";
const SNAPSHOT_TRACE: &str = "trace";
const SNAPSHOT_OFF: &str = "off";

/// Whether an eligible process boots from a snapshot (`SNAPSHOT_ENV`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapshotUse {
    /// When its entry loads modules (`EntryLoading::Modules`). A program that loads modules
    /// spends most of its start loading the builtins the snapshot holds (npm scenarios, x86_64
    /// 4 vCPU, pinned, median of 7, 2026-10-25, without -> with: lodash 67 -> 30 ms, express
    /// 123 -> 80, zod 85 -> 53, axios 132 -> 87, playwright 428 -> 382). A script that names no
    /// module boots without one: a fresh boot loads only what it uses, while a restored
    /// `Preloaded` realm keeps every preloaded builtin resident (V8-v7 max RSS, without ->
    /// with: RegExp 12.3 -> 17.1 MB, Splay 32.6 -> 39.0, EarleyBoyer 12.3 -> 15.9; a one-line
    /// `console.log` 12.9M -> 23.6M instructions).
    Auto,
    Always,
    Never,
}

impl SnapshotUse {
    fn of_environment() -> Self {
        match std::env::var_os(SNAPSHOT_ENV) {
            Some(value) if value == SNAPSHOT_ON || value == SNAPSHOT_TRACE => Self::Always,
            Some(value) if value == SNAPSHOT_OFF => Self::Never,
            _ => Self::Auto,
        }
    }

    fn restores(self, loading: entry_graph::EntryLoading) -> bool {
        match self {
            Self::Auto => loading == entry_graph::EntryLoading::Modules,
            Self::Always => true,
            Self::Never => false,
        }
    }
}
/// Set in the environment of the process that builds a snapshot, to the file it writes.
pub(crate) const BUILD_ENV: &str = "QUENCH_BUILD_STARTUP_SNAPSHOT";
/// The snapshots' directory beside the compile cache's (`CompileCache::directory`).
const SNAPSHOT_SUBDIRECTORY: &str = "snapshot";
const SNAPSHOT_EXTENSION: &str = "qss";
const LOCK_EXTENSION: &str = "lock";
/// Builds whose snapshots are kept: a few builds in use side by side (an installed one and a
/// development one) keep theirs, while older ones go.
/// A build lock older than this belongs to a builder that died, or to a build that failed and
/// kept it so that eligible processes do not each start a failing build: another may start.
const STALE_BUILD: std::time::Duration = std::time::Duration::from_secs(60);
/// The Node builtins a `Preloaded` snapshot loads, as Node's own startup snapshot does: the
/// modules nearly every program that loads modules loads, whose loading is most of its run.
/// `zlib` costs a restore that loads none of it 0.35 ms (11.61 -> 11.95 ms, 31 interleaved
/// runs, x86_64 VM, 2026-10-23) and saves each package that loads it more: npm scenarios
/// node-fetch 50.0 -> 44.1 ms, axios 108.2 -> 96.9, minizlib 34.0 -> 30.4.
const PRELOADED_BUILTINS: &[&str] = &[
    "events", "buffer", "util", "path", "fs", "url", "timers", "querystring", "async_hooks",
    "diagnostics_channel", "stream", "module", "child_process", "http", "zlib",
];

/// What a snapshot holds beyond the bootstrap. A restore decodes everything a snapshot holds
/// (heap, programs, shapes, atoms), so the builtins cost every process that restores them
/// whether it loads them or not: a process whose entry can load none restores without them
/// (`entry_graph::snapshot_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotKind {
    /// The bootstrap alone.
    Bootstrap,
    /// The bootstrap with `PRELOADED_BUILTINS` loaded.
    Preloaded,
}

impl SnapshotKind {
    const ALL: [Self; 2] = [Self::Bootstrap, Self::Preloaded];

    /// The kind's part of its snapshot's file name, from which its builder reads it back.
    fn name(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Preloaded => "preloaded",
        }
    }

    fn builtins(self) -> &'static [&'static str] {
        match self {
            Self::Bootstrap => &[],
            Self::Preloaded => PRELOADED_BUILTINS,
        }
    }

    /// The kind of the snapshot `snapshot_path` named `path`.
    fn of_path(path: &std::path::Path) -> Option<Self> {
        let name = std::path::Path::new(path.file_stem()?).extension()?;
        Self::ALL.into_iter().find(|kind| name == kind.name())
    }
}

/// Refuses a read of the process's own state while a snapshot is built: what the snapshot
/// keeps must not depend on the process that built it.
pub(crate) fn refuse_process_state(
    context: &mut quench_runtime::NativeContext<'_, crate::host::NodeHost>,
    what: &str,
) -> Result<(), quench_runtime::RootedError> {
    if context.host_mut().building_snapshot.is_some() {
        return Err(quench_runtime::RootedError::host(format!("a startup snapshot cannot keep {what}")));
    }
    Ok(())
}

/// Loads the builtins a snapshot of `kind` keeps, at the snapshot point.
pub(crate) fn preload(
    context: &mut quench_runtime::NativeContext<'_, crate::host::NodeHost>,
    kind: SnapshotKind,
) -> Result<(), quench_runtime::RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted(crate::host::shared_vm::commonjs::BOOTSTRAP_REQUIRE);
    let require = context.get_property_rooted(global, key)?;
    let undefined = context.undefined();
    for name in kind.builtins() {
        context.scoped(|context| {
            let specifier = context.string_rooted(name);
            context.call_rooted(require, undefined, &[specifier]).map(drop)
        })?;
    }
    // Node's `fs` loads its option policy and defines its members with it; Quench's at their
    // first use.
    if kind.builtins().contains(&"fs") {
        context.scoped(crate::modules::fs_policy_shared_vm::capture)?;
        context.scoped(crate::modules::fs_shared_vm::materialize)?;
    }
    Ok(())
}

/// The entry a process restores a snapshot for: a file, or the inline source of `-e`/`-p`.
pub(crate) enum SnapshotEntry<'a> {
    File(&'a std::path::Path),
    Inline(&'a str),
}

/// Where a process with these inputs finds its snapshot, when it may use one: the main
/// thread of a file or inline entry run without options beyond the inline source's own
/// (`shared_run::names_only_inline_source`), which only the part after the snapshot point
/// reads (`commonjs::finish_initialize` installs the eval globals, `refresh_identity` the
/// argv), under an environment that changes nothing the bootstrap reads. An inline entry had
/// booted in full: 27 ms for `-e 'console.log(42)'` against 5.5 ms for the same file, which
/// every child process the execa and tinyexec npm scenarios spawn paid.
pub(crate) fn snapshot_path(
    host: &crate::host::NodeHost,
    entry: Option<SnapshotEntry<'_>>,
    worker: bool,
) -> Option<std::path::PathBuf> {
    let entry = entry?;
    if worker || !crate::shared_run::names_only_inline_source(&host.shared_state.borrow().exec_argv) {
        return None;
    }
    let snapshot_use = SnapshotUse::of_environment();
    if REFUSED_VARIABLES.iter().any(|name| std::env::var_os(name).is_some()) || snapshot_use == SnapshotUse::Never {
        return None;
    }
    let loading = match entry {
        SnapshotEntry::File(path) => entry_graph::entry_loading(path),
        SnapshotEntry::Inline(source) => entry_graph::inline_entry_loading(source),
    };
    if !snapshot_use.restores(loading) {
        return None;
    }
    // Each build's snapshots share a directory, which a builder of another build removes.
    let directory = host
        .compile_cache
        .directory()?
        .parent()?
        .join(SNAPSHOT_SUBDIRECTORY)
        .join(format!("{:016x}", host.compile_cache.build()));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&host.node_library_root, &mut hasher);
    for name in KEYED_VARIABLES {
        std::hash::Hash::hash(&std::env::var_os(name), &mut hasher);
    }
    let kind = loading.snapshot_kind();
    Some(directory.join(format!(
        "{:016x}.{}.{SNAPSHOT_EXTENSION}",
        std::hash::Hasher::finish(&hasher),
        kind.name()
    )))
}

/// The snapshot at `path`, mapped; when there is none, starts a process that builds it for
/// the next run, unless one is building it already.
pub(crate) fn load_or_schedule(path: &std::path::Path) -> Option<quench_runtime::KeptBytes> {
    if let Some(bytes) = crate::host::compile_cache::map_file(path) {
        return Some(bytes);
    }
    let lock = path.with_extension(LOCK_EXTENSION);
    let stale = std::fs::metadata(&lock)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > STALE_BUILD));
    if stale {
        remove_unfinished_writes(path);
        let _ = std::fs::remove_file(&lock);
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).ok()?;
    let started = std::env::current_exe().ok().and_then(|executable| {
        let mut builder = std::process::Command::new(executable);
        builder
            .env(BUILD_ENV, path)
            .current_dir("/")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // Its own process group, so a signal to this process's group (a terminal's interrupt,
        // a supervisor ending the run) does not end the build it outlives.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut builder, 0);
        builder.spawn().ok()
    });
    if started.is_none() {
        let _ = std::fs::remove_file(&lock);
    }
    None
}

/// Reports whether a snapshot was restored, and why not, when `SNAPSHOT_ENV` asks for it.
pub(crate) fn report(path: Option<&std::path::Path>, outcome: Result<(), &str>) {
    if std::env::var_os(SNAPSHOT_ENV).is_some_and(|value| value == SNAPSHOT_TRACE) {
        let kind = path.and_then(SnapshotKind::of_path).map_or("", SnapshotKind::name);
        match outcome {
            Ok(()) => eprintln!("quench: startup snapshot restored ({kind})"),
            Err(error) => eprintln!("quench: startup snapshot ({kind}) not restored: {error}"),
        }
    }
}

/// Builds the snapshot a process with this one's inputs restores, into `path`: runs the
/// bootstrap to the snapshot point with an empty entry and writes the VM and host state.
pub(crate) fn build(path: &std::path::Path) -> Result<(), String> {
    let result = (|| {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let kind = SnapshotKind::of_path(path).ok_or_else(|| format!("{} names no snapshot kind", path.display()))?;
        let mut host = crate::host::NodeHost::new(vec![executable.to_string_lossy().into_owned()]);
        host.building_snapshot = Some(kind);
        let mut runtime = quench_runtime::Runtime::new(host);
        let program = runtime
            .compile_entry(quench_runtime::ExecutionRequest {
                source: "",
                name: "[startup snapshot]",
                kind: quench_runtime::SourceKind::Script,
            })
            .map_err(|error| format!("{error:?}"))?;
        let bytes = runtime.build_snapshot(&program, |host, writer| host.write_snapshot(writer))?;
        let temporary = unfinished_write(path, std::process::id());
        let written = std::fs::write(&temporary, &bytes).and_then(|()| std::fs::rename(&temporary, path));
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        if let Some(builds) = path.parent().and_then(std::path::Path::parent) {
            crate::host::compile_cache::remove_other_builds(builds);
        }
        Ok(())
    })();
    // A failed build keeps its lock until it is stale (`STALE_BUILD`), so the processes that
    // find no snapshot meanwhile boot without one rather than each starting the build again.
    if result.is_ok() {
        let _ = std::fs::remove_file(path.with_extension(LOCK_EXTENSION));
    }
    result
}

/// The file builder `pid` writes `path`'s snapshot into before renaming it to `path`, so no
/// process maps a partly written snapshot.
fn unfinished_write(path: &std::path::Path, pid: u32) -> std::path::PathBuf {
    path.with_extension(format!("{SNAPSHOT_EXTENSION}.{pid}"))
}

/// Removes the unfinished writes of `path`'s builders, which only a builder that died leaves.
fn remove_unfinished_writes(path: &std::path::Path) {
    let (Some(directory), Some(prefix)) = (path.parent(), unfinished_write(path, 0).file_name().map(|name| {
        let name = name.to_string_lossy();
        name[..name.len() - 1].to_owned()
    })) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.strip_prefix(&prefix).is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_build_keeps_its_lock_and_leaves_no_unfinished_write() {
        let directory = std::env::temp_dir().join(format!("quench-snapshot-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        // A file name naming no snapshot kind fails the build before it writes anything.
        let path = directory.join("0123456789abcdef.unknown.qss");
        let lock = path.with_extension(LOCK_EXTENSION);
        std::fs::write(&lock, b"").unwrap();
        assert!(build(&path).is_err());
        assert!(lock.exists(), "a failed build backs off by keeping its lock");
        assert!(!path.exists());
        // A builder that died left an unfinished write; clearing its stale lock removes it,
        // and nothing else.
        let unfinished = unfinished_write(&path, 4242);
        let other = directory.join("0123456789abcdef.unknown.qss.notes");
        std::fs::write(&unfinished, b"partial").unwrap();
        std::fs::write(&other, b"").unwrap();
        remove_unfinished_writes(&path);
        assert!(!unfinished.exists());
        assert!(other.exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
