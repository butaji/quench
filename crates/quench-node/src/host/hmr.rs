//! The loaded application module graph used by the opt-in HMR path.

pub(crate) mod type_worker;

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use rustc_hash::{FxHashMap, FxHashSet};
use sha1::Digest;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// The watcher cadence used by the runtime's nonblocking HMR poll.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// A file must keep one stamp for this interval before an update is emitted.
const STABLE_INTERVAL: Duration = Duration::from_millis(30);
/// SHA-1 emits 160 bits; this digest detects byte changes when metadata is unchanged.
pub(crate) const FILE_CONTENT_DIGEST_BYTES: usize = 20;
pub(crate) type FileContentDigest = [u8; FILE_CONTENT_DIGEST_BYTES];
/// Worker runtimes publish the source versions they loaded so the main runtime can
/// watch them and choose process restart when it cannot replace that worker generation.
pub(crate) type WorkerWatchRegistry = Arc<Mutex<std::collections::HashMap<PathBuf, Option<FileStamp>>>>;
/// Bound per-file scratch space while hashing sources during the polling loop.
const FILE_STAMP_READ_BUFFER_BYTES: usize = 8 * 1024;
const SELF_ACCEPTED: &str = "\0quench:hmr:selfAccepted";
const SELF_DECLINED: &str = "\0quench:hmr:selfDeclined";
const INVALIDATED: &str = "\0quench:hmr:invalidated";
const ACCEPT_CALLBACKS: &str = "\0quench:hmr:acceptCallbacks";
const ACCEPT_DEPENDENCY_CALLBACKS: &str = "\0quench:hmr:acceptDependencyCallbacks";
const DISPOSE_CALLBACKS: &str = "\0quench:hmr:disposeCallbacks";
/// Eval workers have no file entry, but still need a graph to record loaded file modules.
const WORKER_EVAL_ENTRY: &str = "\0quench:hmr:worker-eval";

/// Cached source modules and their importer-to-dependency edges.
///
/// The forward edges are authoritative. Reverse edges and invalidation sets are derived
/// per update so cache state cannot drift from the module graph. Detached modules stay
/// tracked while their runtime cache records remain alive, preventing an edit from being
/// missed before a later reimport.
pub(crate) struct ModuleGraph {
    entry: PathBuf,
    dependencies: FxHashMap<PathBuf, FxHashSet<PathBuf>>,
    dependency_specifiers: FxHashMap<(PathBuf, PathBuf), FxHashSet<String>>,
    stamps: FxHashMap<PathBuf, Option<FileStamp>>,
    pending: FxHashMap<PathBuf, PendingStamp>,
    retry: FxHashSet<PathBuf>,
    last_poll: Option<Instant>,
}

/// Changed source files and the loaded importer closure that must be updated or restarted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HmrChangeSet {
    pub changed: Vec<PathBuf>,
    pub invalidated: Vec<PathBuf>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FileStamp {
    kind: FileStampKind,
    length: u64,
    modified: Option<SystemTime>,
    content_digest: FileContentDigest,
    #[cfg(unix)]
    identity: Option<(u64, u64)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileStampKind {
    RegularFile,
    Symlink,
}

struct PendingStamp {
    stamp: Option<FileStamp>,
    first_seen: Instant,
}

impl ModuleGraph {
    pub(crate) fn new_worker() -> Self {
        Self::new_with_stamp(PathBuf::from(WORKER_EVAL_ENTRY), None)
    }

    pub(crate) fn new_with_stamp(entry: PathBuf, stamp: Option<FileStamp>) -> Self {
        let entry = normalize(entry);
        let mut dependencies = FxHashMap::default();
        dependencies.insert(entry.clone(), FxHashSet::default());
        let mut stamps = FxHashMap::default();
        stamps.insert(entry.clone(), stamp);
        Self {
            entry,
            dependencies,
            dependency_specifiers: FxHashMap::default(),
            stamps,
            pending: FxHashMap::default(),
            retry: FxHashSet::default(),
            last_poll: None,
        }
    }

    pub(crate) fn record_module(&mut self, module: PathBuf) {
        let module = normalize(module);
        self.dependencies.entry(module.clone()).or_default();
        self.stamps
            .entry(module.clone())
            .or_insert_with(|| file_stamp(&module));
    }

    /// Adds a loaded module to the graph without making its source a watched input.
    /// This keeps importers such as `node_modules` packages visible to acceptance
    /// checks while leaving watch policy with the host.
    pub(crate) fn record_graph_module(&mut self, module: PathBuf) {
        self.dependencies.entry(normalize(module)).or_default();
    }

    pub(crate) fn record_module_with_stamp(&mut self, module: PathBuf, stamp: Option<FileStamp>) {
        let module = normalize(module);
        self.dependencies.entry(module.clone()).or_default();
        self.stamps.entry(module).or_insert(stamp);
    }

    pub(crate) fn stamp(&self, path: &Path) -> Option<Option<FileStamp>> {
        self.stamps.get(path).cloned()
    }

    pub(crate) fn record_dependency(
        &mut self,
        importer: PathBuf,
        dependency: PathBuf,
        specifier: Option<&str>,
    ) {
        let importer = normalize(importer);
        let dependency = normalize(dependency);
        self.dependencies
            .entry(importer.clone())
            .or_default()
            .insert(dependency.clone());
        self.dependencies.entry(dependency.clone()).or_default();
        if let Some(specifier) = specifier {
            self.dependency_specifiers
                .entry((importer, dependency))
                .or_default()
                .insert(specifier.to_owned());
        }
    }

    /// Checks tracked files at most once per `POLL_INTERVAL`, returning changed paths.
    pub(crate) fn changed_files(&mut self, now: Instant) -> Vec<PathBuf> {
        if self
            .last_poll
            .is_some_and(|last| now.saturating_duration_since(last) < POLL_INTERVAL)
        {
            return Vec::new();
        }
        self.last_poll = Some(now);
        let mut changed = Vec::new();
        for path in std::mem::take(&mut self.retry) {
            let Some(previous) = self.stamps.get(&path) else {
                continue;
            };
            let current = file_stamp(&path);
            if current == *previous {
                changed.push(path);
            } else {
                self.pending.insert(
                    path,
                    PendingStamp {
                        stamp: current,
                        first_seen: now,
                    },
                );
            }
        }
        for (path, previous) in &mut self.stamps {
            let current = file_stamp(path);
            if current == *previous {
                self.pending.remove(path);
                continue;
            }
            match self.pending.get(path) {
                Some(pending)
                    if pending.stamp == current
                        && now.saturating_duration_since(pending.first_seen) >= STABLE_INTERVAL =>
                {
                    *previous = current;
                    self.pending.remove(path);
                    changed.push(path.clone());
                }
                Some(pending) if pending.stamp == current => {}
                _ => {
                    self.pending.insert(
                        path.clone(),
                        PendingStamp {
                            stamp: current,
                            first_seen: now,
                        },
                    );
                }
            }
        }
        changed.sort();
        changed.dedup();
        changed
    }

    /// Schedule already-stable changes for another attempt after a sibling edit failed
    /// source preparation. Their stamps are rechecked before retrying.
    pub(crate) fn retry_changes(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.retry.extend(
            paths
                .into_iter()
                .map(normalize)
                .filter(|path| self.dependencies.contains_key(path)),
        );
    }

    /// Confirms preparation used the exact stable file version emitted by the watcher.
    pub(crate) fn change_matches_stamp(
        &self,
        path: &Path,
        prepared_digest: Option<&FileContentDigest>,
    ) -> bool {
        let path = normalize(path.to_path_buf());
        let Some(expected) = self.stamps.get(&path) else {
            return false;
        };
        let current = file_stamp(&path);
        match expected {
            Some(expected) => {
                prepared_digest.is_none_or(|digest| &expected.content_digest == digest)
                    && current.as_ref() == Some(expected)
            }
            None => prepared_digest.is_none() && current.is_none(),
        }
    }

    /// Returns changed modules and all loaded importers that must be reconsidered.
    pub(crate) fn invalidated_by(
        &self,
        changed: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<PathBuf> {
        let mut importers: FxHashMap<&Path, Vec<&Path>> = FxHashMap::default();
        for (importer, dependencies) in &self.dependencies {
            for dependency in dependencies {
                importers.entry(dependency).or_default().push(importer);
            }
        }

        let mut invalidated = FxHashSet::default();
        let mut pending = Vec::new();
        for path in changed.into_iter().map(normalize) {
            if self.dependencies.contains_key(&path) && invalidated.insert(path.clone()) {
                pending.push(path);
            }
        }

        while let Some(module) = pending.pop() {
            if let Some(parents) = importers.get(module.as_path()) {
                for parent in parents {
                    let parent = PathBuf::from(parent);
                    if invalidated.insert(parent.clone()) {
                        pending.push(parent);
                    }
                }
            }
        }

        let mut modules = invalidated.into_iter().collect::<Vec<_>>();
        modules.sort();
        modules
    }

    pub(crate) fn entry(&self) -> &Path {
        &self.entry
    }

    pub(crate) fn direct_importers(&self, dependency: &Path) -> Vec<(PathBuf, Vec<String>)> {
        let dependency = normalize(dependency.to_path_buf());
        let mut importers = self
            .dependencies
            .iter()
            .filter(|(_, dependencies)| dependencies.contains(&dependency))
            .map(|(importer, _)| {
                let mut specifiers = self
                    .dependency_specifiers
                    .get(&(importer.clone(), dependency.clone()))
                    .into_iter()
                    .flat_map(|specifiers| specifiers.iter().cloned())
                    .collect::<Vec<_>>();
                specifiers.sort();
                (importer.clone(), specifiers)
            })
            .collect::<Vec<_>>();
        importers.sort_by(|left, right| left.0.cmp(&right.0));
        importers
    }

    pub(crate) fn take_dependencies(&mut self, importer: &Path) -> Vec<(PathBuf, Vec<String>)> {
        let importer = normalize(importer.to_path_buf());
        let dependencies = self
            .dependencies
            .get_mut(&importer)
            .map(std::mem::take)
            .unwrap_or_default();
        dependencies
            .into_iter()
            .map(|dependency| {
                let specifiers = self
                    .dependency_specifiers
                    .remove(&(importer.clone(), dependency.clone()))
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                (dependency, specifiers)
            })
            .collect()
    }

    pub(crate) fn restore_dependencies(
        &mut self,
        importer: &Path,
        dependencies: Vec<(PathBuf, Vec<String>)>,
    ) {
        let importer = normalize(importer.to_path_buf());
        self.dependencies
            .entry(importer.clone())
            .or_default()
            .clear();
        self.dependency_specifiers
            .retain(|(parent, _), _| parent != &importer);
        for (dependency, specifiers) in dependencies {
            if specifiers.is_empty() {
                self.record_dependency(importer.clone(), dependency, None);
            } else {
                for specifier in specifiers {
                    self.record_dependency(importer.clone(), dependency.clone(), Some(&specifier));
                }
            }
        }
    }
}

pub(crate) fn file_stamp(path: &Path) -> Option<FileStamp> {
    let link_metadata = std::fs::symlink_metadata(path).ok()?;
    if link_metadata.file_type().is_symlink() {
        let target = std::fs::read_link(path).ok()?;
        let content_digest = sha1::Sha1::digest(path_bytes(&target)).into();
        return Some(file_stamp_from_metadata(
            link_metadata,
            content_digest,
            FileStampKind::Symlink,
        ));
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = sha1::Sha1::new();
    let mut buffer = [0; FILE_STAMP_READ_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let metadata = file.metadata().ok()?;
    let content_digest = hasher.finalize().into();
    Some(file_stamp_from_metadata(
        metadata,
        content_digest,
        FileStampKind::RegularFile,
    ))
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

/// Stamp the exact source string already read for initial ESM compilation. Metadata is
/// sampled separately; if the file changed between its read and this call, the source
/// digest still makes the watcher detect that mismatch on its first poll.
pub(crate) fn file_stamp_for_source(path: &Path, source: &str) -> FileStamp {
    let metadata = std::fs::metadata(path).ok();
    let content_digest = sha1::Sha1::digest(source.as_bytes()).into();
    FileStamp {
        kind: FileStampKind::RegularFile,
        length: source.len() as u64,
        modified: metadata.as_ref().and_then(|metadata| metadata.modified().ok()),
        content_digest,
        #[cfg(unix)]
        identity: metadata.as_ref().map(|metadata| {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        }),
    }
}

fn file_stamp_from_metadata(
    metadata: std::fs::Metadata,
    content_digest: FileContentDigest,
    kind: FileStampKind,
) -> FileStamp {
    FileStamp {
        kind,
        length: metadata.len(),
        modified: metadata.modified().ok(),
        content_digest,
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            Some((metadata.dev(), metadata.ino()))
        },
    }
}

fn normalize(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// Replaces the opt-in CLI process with the same invocation after a conservative HMR restart.
pub(crate) fn restart_cli_process() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut command = std::process::Command::new(executable);
    command.args(arguments);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().to_string())
    }
    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|error| error.to_string())?;
        std::process::exit(status.code().unwrap_or(1))
    }
}

type Context<'a> = NativeContext<'a, NodeHost>;

enum AcceptCallbackArguments {
    Module,
    Modules {
        dependency_count: usize,
        updated_indices: Vec<usize>,
    },
}

pub(crate) struct AcceptCallback {
    pub(crate) callback: RootId,
    arguments: AcceptCallbackArguments,
}

impl AcceptCallback {
    pub(crate) fn runtime_callback(&self) -> quench_runtime::ModuleAcceptCallback {
        let arguments = match &self.arguments {
            AcceptCallbackArguments::Module => quench_runtime::ModuleAcceptArguments::Namespace,
            AcceptCallbackArguments::Modules {
                dependency_count,
                updated_indices,
            } => quench_runtime::ModuleAcceptArguments::NamespaceArray {
                length: *dependency_count,
                updated_indices: updated_indices.clone(),
            },
        };
        quench_runtime::ModuleAcceptCallback {
            callback: self.callback,
            arguments,
        }
    }
}

/// Installs the opt-in CommonJS `module.hot` registration surface.
pub(crate) fn create_hot_api(
    context: &mut Context<'_>,
    source_name: &str,
) -> Result<RootId, RootedError> {
    let hot = context.object_rooted()?;
    let data = context.object_rooted()?;
    let accept_callbacks = context.array_rooted(&[])?;
    let accept_dependency_callbacks = context.array_rooted(&[])?;
    let dispose_callbacks = context.array_rooted(&[])?;
    let id = context.string_rooted(source_name);
    set(context, hot, "data", data)?;
    set(context, hot, "id", id)?;
    set_bool(context, hot, "active", true)?;
    set_internal_bool(context, hot, SELF_ACCEPTED, false)?;
    set_internal_bool(context, hot, SELF_DECLINED, false)?;
    set_internal_bool(context, hot, INVALIDATED, false)?;
    set_internal(context, hot, ACCEPT_CALLBACKS, accept_callbacks)?;
    set_internal(
        context,
        hot,
        ACCEPT_DEPENDENCY_CALLBACKS,
        accept_dependency_callbacks,
    )?;
    set_internal(context, hot, DISPOSE_CALLBACKS, dispose_callbacks)?;

    for (name, operation) in [
        ("accept", "hmrHotAccept"),
        ("dispose", "hmrHotDispose"),
        ("decline", "hmrHotDecline"),
        ("invalidate", "hmrHotInvalidate"),
    ] {
        let function = context.host_function_named_with_data(
            crate::host::shared_vm::operation(operation),
            name,
            hot,
        )?;
        set(context, hot, name, function)?;
    }
    Ok(hot)
}

pub(crate) fn hot_accept(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let hot = context.host_function_data()?;
    match args.first().copied() {
        None => set_internal_bool(context, hot, SELF_ACCEPTED, true)?,
        Some(first) if context.is_callable_rooted(first)? => {
            set_internal_bool(context, hot, SELF_ACCEPTED, true)?;
            append(context, hot, ACCEPT_CALLBACKS, first)?;
        }
        Some(first) => {
            let (dependencies, expects_array) = if context.is_array_rooted(first)? {
                (context.array_like_values_rooted(first)?, true)
            } else if context.string_text(first)?.is_some() {
                (vec![first], false)
            } else {
                return Err(throw_type_error(
                    context,
                    "module.hot.accept expects a callback, a dependency, or an array of dependencies",
                )?);
            };
            for dependency in &dependencies {
                if context.string_text(*dependency)?.is_none() {
                    return Err(throw_type_error(
                        context,
                        "module.hot.accept dependencies must be strings",
                    )?);
                }
            }
            let callback = args.get(1).copied().unwrap_or_else(|| context.undefined());
            if !context
                .rooted_value(callback)
                .is_some_and(|value| value.is_undefined())
                && !context.is_callable_rooted(callback)?
            {
                return Err(throw_type_error(
                    context,
                    "module.hot.accept callback must be a function",
                )?);
            }
            let registration = context.object_rooted()?;
            let dependencies = context.array_rooted(&dependencies)?;
            set_internal(context, registration, "dependencies", dependencies)?;
            set_internal(context, registration, "callback", callback)?;
            set_internal_bool(context, registration, "expectsArray", expects_array)?;
            append(context, hot, ACCEPT_DEPENDENCY_CALLBACKS, registration)?;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn hot_dispose(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        let error = context.type_error_rooted("module.hot.dispose expects a callback")?;
        return Err(context.throw(error));
    };
    if !context.is_callable_rooted(callback)? {
        return Err(throw_type_error(
            context,
            "module.hot.dispose expects a callback",
        )?);
    }
    let hot = context.host_function_data()?;
    append(context, hot, DISPOSE_CALLBACKS, callback)?;
    Ok(context.undefined())
}

pub(crate) fn hot_decline(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let hot = context.host_function_data()?;
    set_internal_bool(context, hot, SELF_DECLINED, true)?;
    Ok(context.undefined())
}

pub(crate) fn hot_invalidate(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let hot = context.host_function_data()?;
    set_internal_bool(context, hot, INVALIDATED, true)?;
    Ok(context.undefined())
}

pub(crate) fn self_accepted(context: &mut Context<'_>, hot: RootId) -> Result<bool, RootedError> {
    let value = context.get_named_property_rooted(hot, SELF_ACCEPTED)?;
    context.truthy_rooted(value)
}

pub(crate) fn invalidated(context: &mut Context<'_>, hot: RootId) -> Result<bool, RootedError> {
    let value = context.get_named_property_rooted(hot, INVALIDATED)?;
    context.truthy_rooted(value)
}

pub(crate) fn self_declined(context: &mut Context<'_>, hot: RootId) -> Result<bool, RootedError> {
    let value = context.get_named_property_rooted(hot, SELF_DECLINED)?;
    context.truthy_rooted(value)
}

/// A lifecycle callback can reject the update currently being applied by either declining
/// the module or invalidating its importers. Callers check this both before replacement and
/// after dispose/accept callbacks have run.
pub(crate) fn update_rejected(context: &mut Context<'_>, hot: RootId) -> Result<bool, RootedError> {
    Ok(self_declined(context, hot)? || invalidated(context, hot)?)
}

pub(crate) fn data(context: &mut Context<'_>, hot: RootId) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(hot, "data")
}

pub(crate) fn module_id(context: &mut Context<'_>, hot: RootId) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(hot, "id")
}

pub(crate) fn set_data(
    context: &mut Context<'_>,
    hot: RootId,
    data: RootId,
) -> Result<(), RootedError> {
    set(context, hot, "data", data)
}

pub(crate) fn deactivate(context: &mut Context<'_>, hot: RootId) -> Result<(), RootedError> {
    set_bool(context, hot, "active", false)
}

pub(crate) fn activate(context: &mut Context<'_>, hot: RootId) -> Result<(), RootedError> {
    set_bool(context, hot, "active", true)
}

pub(crate) fn dispose(
    context: &mut Context<'_>,
    hot: RootId,
    data: RootId,
) -> Result<(), RootedError> {
    invoke_callbacks(context, hot, DISPOSE_CALLBACKS, &[data])
}

pub(crate) fn invoke_accept_callbacks(
    context: &mut Context<'_>,
    callbacks: &[AcceptCallback],
    updated_module: RootId,
) -> Result<(), RootedError> {
    let receiver = context.undefined();
    let result = (|| {
        for callback in callbacks {
            let arguments = match &callback.arguments {
                AcceptCallbackArguments::Module => vec![updated_module],
                AcceptCallbackArguments::Modules {
                    dependency_count,
                    updated_indices,
                } => {
                    let undefined = context.undefined();
                    let mut modules = vec![undefined; *dependency_count];
                    for &index in updated_indices {
                        modules[index] = updated_module;
                    }
                    let array = context.array_rooted(&modules);
                    context.release_root(undefined);
                    vec![array?]
                }
            };
            let is_array = matches!(callback.arguments, AcceptCallbackArguments::Modules { .. });
            let result = context.call_rooted(callback.callback, receiver, &arguments);
            if is_array {
                context.release_root(arguments[0]);
            }
            context.release_root(result?);
        }
        Ok(())
    })();
    context.release_root(receiver);
    result
}

pub(crate) fn accept_callbacks(
    context: &mut Context<'_>,
    hot: RootId,
) -> Result<Vec<AcceptCallback>, RootedError> {
    let callbacks = context.get_named_property_rooted(hot, ACCEPT_CALLBACKS)?;
    context
        .array_like_values_rooted(callbacks)?
        .into_iter()
        .map(|callback| {
            Ok(AcceptCallback {
                callback,
                arguments: AcceptCallbackArguments::Module,
            })
        })
        .collect()
}

pub(crate) fn dependency_accept_callbacks(
    context: &mut Context<'_>,
    hot: RootId,
    specifiers: &[String],
) -> Result<Option<Vec<AcceptCallback>>, RootedError> {
    if specifiers.is_empty() {
        return Ok(None);
    }
    let registrations = context.get_named_property_rooted(hot, ACCEPT_DEPENDENCY_CALLBACKS)?;
    let registrations = context.array_like_values_rooted(registrations)?;
    let mut callbacks = Vec::new();
    let mut accepted = FxHashSet::default();
    for registration in registrations {
        let dependencies = context.get_named_property_rooted(registration, "dependencies")?;
        let dependencies = context.array_like_values_rooted(dependencies)?;
        let callback = context.get_named_property_rooted(registration, "callback")?;
        let expects_array_value =
            context.get_named_property_rooted(registration, "expectsArray")?;
        let expects_array = context
            .rooted_value(expects_array_value)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let has_callback = context.is_callable_rooted(callback)?;
        if !has_callback
            && !context
                .rooted_value(callback)
                .is_some_and(|value| value.is_undefined())
        {
            continue;
        }
        let mut updated_indices = Vec::new();
        for (index, dependency) in dependencies.iter().copied().enumerate() {
            let Some(dependency) = context.string_text(dependency)? else {
                continue;
            };
            if specifiers.iter().any(|specifier| specifier == &dependency) {
                accepted.insert(dependency);
                updated_indices.push(index);
            }
        }
        if !updated_indices.is_empty() {
            if has_callback {
                callbacks.push(AcceptCallback {
                    callback,
                    arguments: if expects_array {
                        AcceptCallbackArguments::Modules {
                            dependency_count: dependencies.len(),
                            updated_indices,
                        }
                    } else {
                        AcceptCallbackArguments::Module
                    },
                });
            }
        }
    }
    if specifiers
        .iter()
        .all(|specifier| accepted.contains(specifier))
    {
        Ok(Some(callbacks))
    } else {
        Ok(None)
    }
}

fn invoke_callbacks(
    context: &mut Context<'_>,
    hot: RootId,
    property: &str,
    args: &[RootId],
) -> Result<(), RootedError> {
    let callbacks = context.get_named_property_rooted(hot, property)?;
    let callbacks = context.array_like_values_rooted(callbacks)?;
    let undefined = context.undefined();
    let result = (|| {
        for callback in callbacks {
            let result = context.call_rooted(callback, undefined, args)?;
            context.release_root(result);
        }
        Ok(())
    })();
    context.release_root(undefined);
    result
}

fn append(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let current = context.get_named_property_rooted(object, property)?;
    let mut values = context.array_like_values_rooted(current)?;
    values.push(value);
    let replacement = context.array_rooted(&values)?;
    set(context, object, property, replacement)
}

fn set(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(property);
    context
        .define_data_property_rooted(object, key, value, true, true, true)
        .map(|_| ())
}

fn set_bool(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
    value: bool,
) -> Result<(), RootedError> {
    let value = context.boolean(value);
    set(context, object, property, value)
}

fn set_internal(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(property);
    context
        .define_data_property_rooted(object, key, value, true, false, true)
        .map(|_| ())
}

fn set_internal_bool(
    context: &mut Context<'_>,
    object: RootId,
    property: &str,
    value: bool,
) -> Result<(), RootedError> {
    let value = context.boolean(value);
    set_internal(context, object, property, value)
}

fn throw_type_error(context: &mut Context<'_>, message: &str) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    Ok(context.throw(error))
}
