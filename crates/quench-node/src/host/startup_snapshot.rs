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
/// `on` boots an eligible process from a snapshot; `trace` does, and reports why one was not
/// used. Otherwise processes boot without one: a restored `Preloaded` realm keeps every
/// preloaded builtin resident, which a script that loads none of them would pay for (V8-v7
/// max RSS, bolted static x86_64, 2026-10-21, before `SnapshotKind`: Richards 2200 -> 7040
/// KiB, RegExp 6160 -> 10484).
pub(crate) const SNAPSHOT_ENV: &str = "QUENCH_STARTUP_SNAPSHOT";
const SNAPSHOT_ON: &str = "on";
const SNAPSHOT_TRACE: &str = "trace";
/// Set in the environment of the process that builds a snapshot, to the file it writes.
pub(crate) const BUILD_ENV: &str = "QUENCH_BUILD_STARTUP_SNAPSHOT";
/// The snapshots' directory beside the compile cache's (`CompileCache::directory`).
const SNAPSHOT_SUBDIRECTORY: &str = "snapshot";
const SNAPSHOT_EXTENSION: &str = "qss";
const LOCK_EXTENSION: &str = "lock";
/// Builds whose snapshots are kept: a few builds in use side by side (an installed one and a
/// development one) keep theirs, while older ones go.
const KEPT_BUILDS: usize = 3;
/// A build lock older than this belongs to a builder that died: another may start.
const STALE_BUILD: std::time::Duration = std::time::Duration::from_secs(60);
/// The Node builtins a `Preloaded` snapshot loads, as Node's own startup snapshot does: the
/// modules nearly every program that loads modules loads, whose loading is most of its run.
const PRELOADED_BUILTINS: &[&str] = &[
    "events", "buffer", "util", "path", "fs", "url", "timers", "querystring", "async_hooks",
    "diagnostics_channel", "stream", "module", "child_process", "http",
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
    if REFUSED_VARIABLES.iter().any(|name| std::env::var_os(name).is_some())
        || !std::env::var_os(SNAPSHOT_ENV).is_some_and(|value| value == SNAPSHOT_ON || value == SNAPSHOT_TRACE)
    {
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
    let kind = match entry {
        SnapshotEntry::File(path) => entry_graph::snapshot_kind(path),
        SnapshotEntry::Inline(source) => entry_graph::inline_snapshot_kind(source),
    };
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
        let temporary = path.with_extension(format!("{SNAPSHOT_EXTENSION}.{}", std::process::id()));
        std::fs::write(&temporary, &bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        remove_other_builds(path);
        Ok(())
    })();
    let _ = std::fs::remove_file(path.with_extension(LOCK_EXTENSION));
    result
}

/// Removes the snapshots of all but the `KEPT_BUILDS` most recently built builds, the one that
/// wrote `path` among them: a rebuild leaves its predecessor's behind, which no process of the
/// new build reads.
fn remove_other_builds(path: &std::path::Path) {
    let Some(builds) = path.parent().and_then(std::path::Path::parent) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(builds) else {
        return;
    };
    let mut builds = entries
        .flatten()
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect::<Vec<_>>();
    builds.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, build) in builds.into_iter().skip(KEPT_BUILDS) {
        let _ = std::fs::remove_dir_all(build);
    }
}
