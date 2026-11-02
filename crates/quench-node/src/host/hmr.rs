//! Opt-in hot module replacement (`--hmr`).
//!
//! The host records the application's loaded module graph, polls its source files, and on
//! a stable edit replaces each changed module in place when the module or every importer
//! accepted it (`module.hot` / `import.meta.hot`). An edit nobody accepted, a removed file,
//! or package metadata restarts the process with the same arguments. Modules load through
//! the ordinary CommonJS and ES module paths, so TypeScript reloads with the same type
//! stripping as a first load.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use rustc_hash::{FxHashMap, FxHashSet};
use sha1::Digest;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The CLI switch that enables this module.
pub(crate) const FLAG: &str = "--hmr";
/// The watcher's cadence: polls closer together than this find nothing new.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// An edit is applied once its file has kept one stamp this long (editors write in steps).
const STABLE_INTERVAL: Duration = Duration::from_millis(30);
/// SHA-1's digest size: a content digest tells a rewrite of the same bytes from an edit.
const DIGEST_BYTES: usize = 20;

// The hot object's registrations, as non-enumerable properties the guest cannot name.
const SELF_ACCEPTED: &str = "\0quench:hmr:selfAccepted";
const DECLINED: &str = "\0quench:hmr:declined";
const INVALIDATED: &str = "\0quench:hmr:invalidated";
const ACCEPT_CALLBACKS: &str = "\0quench:hmr:acceptCallbacks";
const DEPENDENCY_ACCEPTS: &str = "\0quench:hmr:dependencyAccepts";
const DISPOSE_CALLBACKS: &str = "\0quench:hmr:disposeCallbacks";
const REGISTRATION_DEPENDENCIES: &str = "dependencies";
const REGISTRATION_CALLBACK: &str = "callback";
const REGISTRATION_EXPECTS_ARRAY: &str = "expectsArray";

/// The hot object's methods and the host operations behind them.
const HOT_METHODS: [(&str, &str); 4] = [
    ("accept", "hmrHotAccept"),
    ("dispose", "hmrHotDispose"),
    ("decline", "hmrHotDecline"),
    ("invalidate", "hmrHotInvalidate"),
];

type Context<'a> = NativeContext<'a, NodeHost>;

/// Whether `exec_argv` opts into HMR.
pub(crate) fn requested(exec_argv: &[String]) -> bool {
    exec_argv.iter().any(|argument| argument == FLAG)
}

/// A file's identity and size at one poll; equal metadata means an unchanged file.
#[derive(Clone, PartialEq, Eq)]
struct FileMetadata {
    length: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}

fn file_metadata(path: &Path) -> Option<FileMetadata> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(FileMetadata {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        },
    })
}

/// A watched file's version: its metadata, and the digest of the bytes it then held.
#[derive(Clone)]
struct FileStamp {
    metadata: FileMetadata,
    digest: [u8; DIGEST_BYTES],
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let bytes = std::fs::read(path).ok()?;
    Some(FileStamp { metadata: file_metadata(path)?, digest: sha1::Sha1::digest(&bytes).into() })
}

/// A watched file whose metadata moved, waiting to hold still for `STABLE_INTERVAL`.
struct PendingEdit {
    metadata: Option<FileMetadata>,
    since: Instant,
}

/// How a changed file is handled.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WatchRole {
    /// Replaced in place when accepted.
    Source,
    /// Read when a module loads (package scope); any change restarts.
    Metadata,
}

struct Watched {
    role: WatchRole,
    stamp: Option<FileStamp>,
    pending: Option<PendingEdit>,
}

/// What a polled edit asks of the process.
pub(crate) enum Update {
    /// Replace these source modules, in order.
    Replace(Vec<PathBuf>),
    /// Metadata changed or a source vanished: only a new process sees it consistently.
    Restart,
}

/// The loaded application graph and its watcher. Forward edges (importer to dependency,
/// with the specifiers that requested it) are authoritative; importers are derived when an
/// update needs them.
pub(crate) struct HmrState {
    entry: PathBuf,
    dependencies: FxHashMap<PathBuf, FxHashMap<PathBuf, FxHashSet<String>>>,
    watched: FxHashMap<PathBuf, Watched>,
    /// Each ES module file's compiled names; one file under several names restarts.
    source_names: FxHashMap<PathBuf, FxHashSet<String>>,
    /// The live `import.meta.hot` of each ES module.
    esm_hot: FxHashMap<PathBuf, RootId>,
    /// `import.meta.hot.data` waiting for the replacement's `import.meta`.
    carried_data: FxHashMap<PathBuf, RootId>,
    last_poll: Option<Instant>,
}

impl HmrState {
    pub(crate) fn new(entry: PathBuf) -> Self {
        let mut state = Self {
            entry: entry.clone(),
            dependencies: FxHashMap::default(),
            watched: FxHashMap::default(),
            source_names: FxHashMap::default(),
            esm_hot: FxHashMap::default(),
            carried_data: FxHashMap::default(),
            last_poll: None,
        };
        state.record_module(&entry);
        state
    }

    pub(crate) fn entry(&self) -> &Path {
        &self.entry
    }

    /// Application files are watched; packages are part of the graph (their importers
    /// still decide acceptance) but are not sources this process edits.
    pub(crate) fn watches(path: &Path) -> bool {
        path.is_absolute() && !path.components().any(|component| component.as_os_str() == "node_modules")
    }

    pub(crate) fn record_module(&mut self, path: &Path) {
        if !path.is_absolute() || self.dependencies.contains_key(path) {
            return;
        }
        self.dependencies.insert(path.to_path_buf(), FxHashMap::default());
        if !Self::watches(path) {
            return;
        }
        self.watch(path, WatchRole::Source);
        for package in crate::host::shared_vm::commonjs::package_scope_files(path) {
            self.watch(&package, WatchRole::Metadata);
        }
    }

    fn watch(&mut self, path: &Path, role: WatchRole) {
        self.watched.entry(path.to_path_buf()).or_insert_with(|| Watched { role, stamp: file_stamp(path), pending: None });
    }

    pub(crate) fn record_dependency(&mut self, importer: &Path, dependency: &Path, specifier: Option<&str>) {
        if importer == dependency || !importer.is_absolute() || !dependency.is_absolute() {
            return;
        }
        self.record_module(importer);
        self.record_module(dependency);
        let specifiers = self
            .dependencies
            .get_mut(importer)
            .expect("the importer was just recorded")
            .entry(dependency.to_path_buf())
            .or_default();
        specifiers.extend(specifier.map(str::to_owned));
    }

    pub(crate) fn record_source_name(&mut self, path: &Path, name: &str) {
        if Self::watches(path) {
            self.source_names.entry(path.to_path_buf()).or_default().insert(name.to_owned());
        }
    }

    /// The one name `path` compiled under as an ES module.
    pub(crate) fn source_name(&self, path: &Path) -> Option<&str> {
        let names = self.source_names.get(path)?;
        (names.len() == 1).then(|| names.iter().next().map(String::as_str)).flatten()
    }

    /// The loaded modules that import `dependency`, with the specifiers each used.
    pub(crate) fn importers(&self, dependency: &Path) -> Vec<(PathBuf, Vec<String>)> {
        let mut importers = self
            .dependencies
            .iter()
            .filter_map(|(importer, dependencies)| {
                let mut specifiers = dependencies.get(dependency)?.iter().cloned().collect::<Vec<_>>();
                specifiers.sort();
                Some((importer.clone(), specifiers))
            })
            .collect::<Vec<_>>();
        importers.sort();
        importers
    }

    /// A reloading module records its imports afresh; `restore_dependencies` undoes this.
    pub(crate) fn take_dependencies(&mut self, module: &Path) -> FxHashMap<PathBuf, FxHashSet<String>> {
        self.dependencies.get_mut(module).map(std::mem::take).unwrap_or_default()
    }

    pub(crate) fn restore_dependencies(&mut self, module: &Path, dependencies: FxHashMap<PathBuf, FxHashSet<String>>) {
        self.dependencies.insert(module.to_path_buf(), dependencies);
    }

    pub(crate) fn esm_hot(&self, path: &Path) -> Option<RootId> {
        self.esm_hot.get(path).copied()
    }

    /// Install `hot` as `path`'s live `import.meta.hot`, returning the one it replaces.
    pub(crate) fn replace_esm_hot(&mut self, path: &Path, hot: Option<RootId>) -> Option<RootId> {
        match hot {
            Some(hot) => self.esm_hot.insert(path.to_path_buf(), hot),
            None => self.esm_hot.remove(path),
        }
    }

    pub(crate) fn replace_carried_data(&mut self, path: &Path, data: Option<RootId>) -> Option<RootId> {
        match data {
            Some(data) => self.carried_data.insert(path.to_path_buf(), data),
            None => self.carried_data.remove(path),
        }
    }

    /// The edits that held still since the last poll, at most once per `POLL_INTERVAL`.
    /// Metadata is compared each poll; a file is read only once its new metadata is stable.
    pub(crate) fn poll(&mut self, now: Instant) -> Option<Update> {
        if self.last_poll.is_some_and(|last| now.saturating_duration_since(last) < POLL_INTERVAL) {
            return None;
        }
        self.last_poll = Some(now);
        let mut changed = Vec::new();
        let mut restart = false;
        for (path, watched) in &mut self.watched {
            let metadata = file_metadata(path);
            if metadata == watched.stamp.as_ref().map(|stamp| stamp.metadata.clone()) {
                watched.pending = None;
                continue;
            }
            let stable = watched.pending.as_ref().is_some_and(|pending| {
                pending.metadata == metadata && now.saturating_duration_since(pending.since) >= STABLE_INTERVAL
            });
            if !stable {
                if watched.pending.as_ref().is_none_or(|pending| pending.metadata != metadata) {
                    watched.pending = Some(PendingEdit { metadata, since: now });
                }
                continue;
            }
            watched.pending = None;
            let stamp = file_stamp(path);
            let edited = match (&watched.stamp, &stamp) {
                (Some(before), Some(after)) => before.digest != after.digest,
                (None, None) => false,
                _ => true,
            };
            restart |= edited && (watched.role == WatchRole::Metadata || stamp.is_none());
            if edited {
                changed.push(path.clone());
            }
            watched.stamp = stamp;
        }
        if restart {
            return Some(Update::Restart);
        }
        (!changed.is_empty()).then(|| {
            changed.sort();
            Update::Replace(changed)
        })
    }
}

/// Replace this process with the same invocation, for an edit it cannot apply in place.
pub(crate) fn restart_process() -> String {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return error.to_string(),
    };
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.exec().to_string()
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error.to_string(),
    }
}

/// One accepting callback and the namespace shape its registration asked for.
pub(crate) type AcceptCallback = quench_runtime::ModuleAcceptCallback;

/// A hot object (`module.hot`, `import.meta.hot`) for the module named `id`.
pub(crate) fn create_hot(context: &mut Context<'_>, id: &str) -> Result<RootId, RootedError> {
    let hot = context.object_rooted()?;
    let data = context.object_rooted()?;
    set(context, hot, "data", data, true)?;
    let id = context.string_rooted(id);
    set(context, hot, "id", id, true)?;
    set_bool(context, hot, "active", true, true)?;
    for flag in [SELF_ACCEPTED, DECLINED, INVALIDATED] {
        set_bool(context, hot, flag, false, false)?;
    }
    for list in [ACCEPT_CALLBACKS, DEPENDENCY_ACCEPTS, DISPOSE_CALLBACKS] {
        let values = context.array_rooted(&[])?;
        set(context, hot, list, values, false)?;
    }
    for (name, operation) in HOT_METHODS {
        let method = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, hot)?;
        set(context, hot, name, method, true)?;
    }
    Ok(hot)
}

/// `hot.accept()`, `hot.accept(callback)`, `hot.accept(dependency | dependencies, callback?)`.
pub(crate) fn hot_accept(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let hot = context.host_function_data()?;
    let Some(first) = args.first().copied().filter(|first| !is_undefined(context, *first)) else {
        set_bool(context, hot, SELF_ACCEPTED, true, false)?;
        return Ok(context.undefined());
    };
    if context.is_callable_rooted(first)? {
        set_bool(context, hot, SELF_ACCEPTED, true, false)?;
        append(context, hot, ACCEPT_CALLBACKS, first)?;
        return Ok(context.undefined());
    }
    let expects_array = context.is_array_rooted(first)?;
    let dependencies = if expects_array { context.array_like_values_rooted(first)? } else { vec![first] };
    for dependency in &dependencies {
        if context.string_text(*dependency)?.is_none() {
            return Err(throw_type_error(context, "hot.accept expects a callback, a dependency, or an array of dependencies")?);
        }
    }
    let callback = args.get(1).copied().unwrap_or_else(|| context.undefined());
    if !is_undefined(context, callback) && !context.is_callable_rooted(callback)? {
        return Err(throw_type_error(context, "hot.accept callback must be a function")?);
    }
    let registration = context.object_rooted()?;
    let dependencies = context.array_rooted(&dependencies)?;
    set(context, registration, REGISTRATION_DEPENDENCIES, dependencies, false)?;
    set(context, registration, REGISTRATION_CALLBACK, callback, false)?;
    set_bool(context, registration, REGISTRATION_EXPECTS_ARRAY, expects_array, false)?;
    append(context, hot, DEPENDENCY_ACCEPTS, registration)?;
    Ok(context.undefined())
}

pub(crate) fn hot_dispose(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = args.first().copied().unwrap_or_else(|| context.undefined());
    if !context.is_callable_rooted(callback)? {
        return Err(throw_type_error(context, "hot.dispose expects a callback")?);
    }
    let hot = context.host_function_data()?;
    append(context, hot, DISPOSE_CALLBACKS, callback)?;
    Ok(context.undefined())
}

pub(crate) fn hot_decline(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let hot = context.host_function_data()?;
    set_bool(context, hot, DECLINED, true, false)?;
    Ok(context.undefined())
}

pub(crate) fn hot_invalidate(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let hot = context.host_function_data()?;
    set_bool(context, hot, INVALIDATED, true, false)?;
    Ok(context.undefined())
}

fn flag(context: &mut Context<'_>, hot: RootId, name: &str) -> Result<bool, RootedError> {
    let value = context.get_named_property_rooted(hot, name)?;
    context.truthy_rooted(value)
}

/// A module that declined updates or invalidated itself rejects the update being applied;
/// lifecycle callbacks may do either, so it is checked before and after they run.
pub(crate) fn rejected(context: &mut Context<'_>, hot: RootId) -> Result<bool, RootedError> {
    Ok(flag(context, hot, DECLINED)? || flag(context, hot, INVALIDATED)?)
}

pub(crate) fn data(context: &mut Context<'_>, hot: RootId) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(hot, "data")
}

pub(crate) fn set_data(context: &mut Context<'_>, hot: RootId, data: RootId) -> Result<(), RootedError> {
    set(context, hot, "data", data, true)
}

pub(crate) fn set_active(context: &mut Context<'_>, hot: RootId, active: bool) -> Result<(), RootedError> {
    set_bool(context, hot, "active", active, true)
}

/// Run `hot`'s dispose callbacks with its `data`, which the replacement then receives.
pub(crate) fn dispose(context: &mut Context<'_>, hot: RootId) -> Result<RootId, RootedError> {
    let data = data(context, hot)?;
    let callbacks = context.get_named_property_rooted(hot, DISPOSE_CALLBACKS)?;
    let receiver = context.undefined();
    for callback in context.array_like_values_rooted(callbacks)? {
        let result = context.call_rooted(callback, receiver, &[data])?;
        context.release_root(result);
    }
    Ok(data)
}

/// Run accept callbacks with a replaced module's new exports or namespace.
pub(crate) fn invoke_accept_callbacks(context: &mut Context<'_>, callbacks: &[AcceptCallback], updated: RootId) -> Result<(), RootedError> {
    let receiver = context.undefined();
    for callback in callbacks {
        let argument = match &callback.arguments {
            quench_runtime::ModuleAcceptArguments::Namespace => updated,
            quench_runtime::ModuleAcceptArguments::NamespaceArray { length, updated_indices } => {
                let mut values = vec![receiver; *length];
                for &index in updated_indices {
                    values[index] = updated;
                }
                context.array_rooted(&values)?
            }
        };
        let result = context.call_rooted(callback.callback, receiver, &[argument])?;
        context.release_root(result);
    }
    Ok(())
}

/// `hot`'s own accept callbacks, when it accepted itself.
fn self_accept_callbacks(context: &mut Context<'_>, hot: RootId) -> Result<Option<Vec<AcceptCallback>>, RootedError> {
    if !flag(context, hot, SELF_ACCEPTED)? {
        return Ok(None);
    }
    let callbacks = context.get_named_property_rooted(hot, ACCEPT_CALLBACKS)?;
    Ok(Some(
        context
            .array_like_values_rooted(callbacks)?
            .into_iter()
            .map(|callback| AcceptCallback { callback, arguments: quench_runtime::ModuleAcceptArguments::Namespace })
            .collect(),
    ))
}

/// The callbacks an importer's `hot` registered for a dependency it imports by
/// `specifiers`; none unless every one of those specifiers is accepted.
fn dependency_accept_callbacks(
    context: &mut Context<'_>,
    hot: RootId,
    specifiers: &[String],
) -> Result<Option<Vec<AcceptCallback>>, RootedError> {
    if specifiers.is_empty() {
        return Ok(None);
    }
    let registrations = context.get_named_property_rooted(hot, DEPENDENCY_ACCEPTS)?;
    let mut callbacks = Vec::new();
    let mut accepted = FxHashSet::default();
    for registration in context.array_like_values_rooted(registrations)? {
        let dependencies = context.get_named_property_rooted(registration, REGISTRATION_DEPENDENCIES)?;
        let dependencies = context.array_like_values_rooted(dependencies)?;
        let callback = context.get_named_property_rooted(registration, REGISTRATION_CALLBACK)?;
        let expects_array = flag(context, registration, REGISTRATION_EXPECTS_ARRAY)?;
        let mut updated_indices = Vec::new();
        for (index, dependency) in dependencies.iter().enumerate() {
            let Some(dependency) = context.string_text(*dependency)? else { continue };
            if specifiers.contains(&dependency) {
                accepted.insert(dependency);
                updated_indices.push(index);
            }
        }
        if updated_indices.is_empty() || !context.is_callable_rooted(callback)? {
            continue;
        }
        let arguments = if expects_array {
            quench_runtime::ModuleAcceptArguments::NamespaceArray { length: dependencies.len(), updated_indices }
        } else {
            quench_runtime::ModuleAcceptArguments::Namespace
        };
        callbacks.push(AcceptCallback { callback, arguments });
    }
    Ok(specifiers.iter().all(|specifier| accepted.contains(specifier)).then_some(callbacks))
}

/// How an update reaches the program: the module accepts itself, or its importers accept it.
pub(crate) struct AcceptPlan {
    pub(crate) callbacks: Vec<AcceptCallback>,
    /// The accepting importers' hot objects, rechecked for rejection after the update.
    pub(crate) importer_hots: Vec<RootId>,
}

/// Which loader's update semantics apply. A CommonJS importer holds the old `exports`
/// object itself, so its update needs a callback that can rebind it; an ES importer keeps
/// its own bindings, so a self-accepting module needs no importer participation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleSystem {
    CommonJs,
    Module,
}

/// Whether and how the update of the module with `hot` is accepted. `importers` pairs
/// each loaded importer's hot object (none when it has no hot API) with its specifiers.
pub(crate) fn accept_plan(
    context: &mut Context<'_>,
    system: ModuleSystem,
    hot: Option<RootId>,
    importers: &[(Option<RootId>, Vec<String>)],
) -> Result<Option<AcceptPlan>, RootedError> {
    if let Some(hot) = hot {
        if rejected(context, hot)? {
            return Ok(None);
        }
    }
    let self_callbacks = match hot {
        Some(hot) => self_accept_callbacks(context, hot)?,
        None => None,
    };
    let importers_needed = match (&self_callbacks, system) {
        (None, _) => true,
        (Some(_), ModuleSystem::Module) => {
            return Ok(self_callbacks.map(|callbacks| AcceptPlan { callbacks, importer_hots: Vec::new() }));
        }
        // CommonJS importers keep the old exports unless the module's own callback runs.
        (Some(callbacks), ModuleSystem::CommonJs) => callbacks.is_empty(),
    };
    if self_callbacks.is_none() && importers.is_empty() {
        return Ok(None);
    }
    let mut callbacks = self_callbacks.unwrap_or_default();
    let mut importer_hots = Vec::new();
    for (importer_hot, specifiers) in importers {
        let importer_callbacks = match importer_hot {
            Some(importer_hot) if !rejected(context, *importer_hot)? => {
                dependency_accept_callbacks(context, *importer_hot, specifiers)?
            }
            Some(_) => return Ok(None),
            None => None,
        };
        let usable = importer_callbacks.filter(|callbacks| system == ModuleSystem::Module || !callbacks.is_empty());
        match usable {
            Some(importer_callbacks) => {
                callbacks.extend(importer_callbacks);
                importer_hots.extend(*importer_hot);
            }
            None if importers_needed => return Ok(None),
            None => {}
        }
    }
    Ok(Some(AcceptPlan { callbacks, importer_hots }))
}

/// The event loop's HMR poll: apply each stable edit, or restart the process for one that
/// cannot be applied. Whether an update ran guest code.
pub(crate) fn poll(runtime: &mut quench_runtime::Runtime<NodeHost>, program: &quench_runtime::ResidualProgram) -> Result<bool, String> {
    let shared_state = runtime.host_mut().shared_state();
    let update = shared_state.borrow_mut().hmr.as_mut().and_then(|state| state.poll(Instant::now()));
    let changed = match update {
        None => return Ok(false),
        Some(Update::Restart) => return Err(restart_process()),
        Some(Update::Replace(changed)) => changed,
    };
    let batch = changed.len() > 1;
    for path in changed {
        match apply(runtime, program, &path) {
            Ok(true) => eprintln!("quench-node HMR: updated {}", path.display()),
            Ok(false) => return Err(restart_process()),
            Err(error) => {
                eprintln!("quench-node HMR: update of {} failed; the prior module stays loaded: {error}", path.display());
                // Later edits of one batch may depend on the one that failed.
                if batch {
                    return Err(restart_process());
                }
            }
        }
    }
    Ok(true)
}

fn apply(runtime: &mut quench_runtime::Runtime<NodeHost>, program: &quench_runtime::ResidualProgram, path: &Path) -> Result<bool, String> {
    let operation = runtime
        .host_function(crate::host::shared_vm::operation("hmrApply"))
        .map_err(|error| runtime.format_error(program, &error.error))?;
    let receiver = runtime.root(quench_runtime::Value::UNDEFINED);
    let path = runtime.string_rooted(&path.to_string_lossy());
    let result = runtime.call_rooted(operation, receiver, &[path]);
    for root in [operation, receiver, path] {
        runtime.release_root(root);
    }
    match result {
        Ok(result) => {
            let applied = runtime.rooted_value(result).and_then(|value| value.as_bool()) == Some(true);
            runtime.release_root(result);
            Ok(applied)
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

/// `hmrApply(path)`: replace one changed module by the loader that loaded it; false when
/// the update is not accepted and the process restarts instead.
pub(crate) fn apply_operation(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = match args.first() {
        Some(path) => context.string_text(*path)?.map(PathBuf::from),
        None => None,
    }
    .ok_or_else(|| RootedError::host("hmrApply expects a path"))?;
    let applied = if crate::host::shared_vm::commonjs::hmr_loaded(context, &path)? {
        crate::host::shared_vm::commonjs::hmr_replace(context, &path)?
    } else {
        replace_es_module(context, &path)?
    };
    Ok(context.boolean(applied))
}

/// The ES module at `path` evaluated again under its compiled name.
fn replace_es_module(context: &mut Context<'_>, path: &Path) -> Result<bool, RootedError> {
    let shared_state = context.host_mut().shared_state();
    let (name, hot, importers) = {
        let state = shared_state.borrow();
        let Some(state) = state.hmr.as_ref() else { return Ok(false) };
        let Some(name) = state.source_name(path) else { return Ok(false) };
        let importers = state
            .importers(path)
            .into_iter()
            .map(|(importer, specifiers)| (state.esm_hot(&importer), specifiers))
            .collect::<Vec<_>>();
        (name.to_owned(), state.esm_hot(path), importers)
    };
    let Some(plan) = accept_plan(context, ModuleSystem::Module, hot, &importers)? else { return Ok(false) };
    let bytes = std::fs::read(path).map_err(|error| RootedError::host(format!("{}: {error}", path.display())))?;
    let module = context.host_mut().module_source(path, name, bytes).map_err(|error| RootedError::host(error.message))?;
    if let Some(hot) = hot {
        let data = dispose(context, hot)?;
        if rejected(context, hot)? {
            return Ok(false);
        }
        set_active(context, hot, false)?;
        let data = context.retain(data)?;
        let previous = shared_state.borrow_mut().hmr.as_mut().and_then(|state| state.replace_carried_data(path, Some(data)));
        release(context, previous);
    }
    let dependencies = with_state(context, |state| {
        state.replace_esm_hot(path, None);
        state.take_dependencies(path)
    });
    let replaced = context.replace_module_sync_rooted(&module, &plan.callbacks);
    let undo = |context: &mut Context<'_>, dependencies| {
        let (replacement, data) = with_state(context, |state| {
            state.restore_dependencies(path, dependencies);
            (state.replace_esm_hot(path, hot), state.replace_carried_data(path, None))
        });
        release(context, replacement);
        release(context, data);
        match hot {
            Some(hot) => set_active(context, hot, true),
            None => Ok(()),
        }
    };
    match replaced {
        Ok(Some(_)) => {
            release(context, hot);
            let data = with_state(context, |state| state.replace_carried_data(path, None));
            release(context, data);
            let replacement = with_state(context, |state| state.esm_hot(path));
            let mut accepted = true;
            for hot in replacement.into_iter().chain(plan.importer_hots) {
                accepted &= !rejected(context, hot)?;
            }
            Ok(accepted)
        }
        Ok(None) => {
            undo(context, dependencies)?;
            Ok(false)
        }
        Err(error) => {
            undo(context, dependencies)?;
            Err(error)
        }
    }
}

/// `import.meta.hot` for an application module (`Host::initialize_import_meta`).
pub(crate) fn initialize_import_meta(context: &mut Context<'_>, source_name: &str, import_meta: RootId) -> Result<(), RootedError> {
    let (_, path) = crate::host::shared_vm::referrer_location(source_name).map_err(|error| RootedError::host(error.message))?;
    let path = context.host_mut().real_path(path);
    if !HmrState::watches(&path) {
        return Ok(());
    }
    let hot = create_hot(context, source_name)?;
    let data = with_state(context, |state| {
        state.record_source_name(&path, source_name);
        state.replace_carried_data(&path, None)
    });
    if let Some(data) = data {
        let applied = set_data(context, hot, data);
        context.release_root(data);
        applied?;
    }
    set(context, import_meta, "hot", hot, true)?;
    let retained = context.retain(hot)?;
    let previous = with_state(context, |state| state.replace_esm_hot(&path, Some(retained)));
    release(context, previous);
    Ok(())
}

/// Run `operate` on the HMR state of a host that has one; callers are HMR operations.
pub(crate) fn with_state<R>(context: &mut Context<'_>, operate: impl FnOnce(&mut HmrState) -> R) -> R {
    let shared_state = context.host_mut().shared_state();
    let mut shared_state = shared_state.borrow_mut();
    operate(shared_state.hmr.as_mut().expect("HMR operations run only with HMR enabled"))
}

fn release(context: &mut Context<'_>, root: Option<RootId>) {
    if let Some(root) = root {
        context.release_root(root);
    }
}

fn is_undefined(context: &mut Context<'_>, value: RootId) -> bool {
    context.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn append(context: &mut Context<'_>, object: RootId, property: &str, value: RootId) -> Result<(), RootedError> {
    let current = context.get_named_property_rooted(object, property)?;
    let mut values = context.array_like_values_rooted(current)?;
    values.push(value);
    let replacement = context.array_rooted(&values)?;
    set(context, object, property, replacement, false)
}

fn set(context: &mut Context<'_>, object: RootId, property: &str, value: RootId, enumerable: bool) -> Result<(), RootedError> {
    let key = context.string_rooted(property);
    context.define_data_property_rooted(object, key, value, true, enumerable, true).map(|_| ())
}

fn set_bool(context: &mut Context<'_>, object: RootId, property: &str, value: bool, enumerable: bool) -> Result<(), RootedError> {
    let value = context.boolean(value);
    set(context, object, property, value, enumerable)
}

fn throw_type_error(context: &mut Context<'_>, message: &str) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    Ok(context.throw(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("quench-hmr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn importers_are_derived_from_forward_edges() {
        let directory = scratch("graph");
        let (entry, dependency) = (directory.join("main.js"), directory.join("dep.js"));
        let mut state = HmrState::new(entry.clone());
        state.record_dependency(&entry, &dependency, Some("./dep.js"));
        state.record_dependency(&dependency, &dependency, Some("./dep.js"));
        assert_eq!(state.importers(&dependency), vec![(entry.clone(), vec!["./dep.js".to_owned()])]);
        assert!(state.importers(&entry).is_empty());
        let taken = state.take_dependencies(&entry);
        assert!(state.importers(&dependency).is_empty());
        state.restore_dependencies(&entry, taken);
        assert_eq!(state.importers(&dependency).len(), 1);
    }

    #[test]
    fn edits_are_reported_once_stable_and_only_when_content_changes() {
        let directory = scratch("poll");
        let source = directory.join("main.js");
        std::fs::write(&source, "1").unwrap();
        let mut state = HmrState::new(source.clone());
        let start = Instant::now();
        assert!(state.poll(start).is_none());
        std::fs::write(&source, "22").unwrap();
        let first = start + POLL_INTERVAL;
        assert!(state.poll(first).is_none(), "a fresh edit waits to hold still");
        let stable = first + STABLE_INTERVAL.max(POLL_INTERVAL);
        assert!(matches!(state.poll(stable), Some(Update::Replace(changed)) if changed == [source.clone()]));
        assert!(state.poll(stable + POLL_INTERVAL).is_none());
        std::fs::remove_file(&source).unwrap();
        let removed = stable + POLL_INTERVAL * 2;
        assert!(state.poll(removed).is_none());
        assert!(matches!(state.poll(removed + STABLE_INTERVAL.max(POLL_INTERVAL)), Some(Update::Restart)));
    }

    #[test]
    fn package_scope_metadata_restarts() {
        let directory = scratch("package");
        let source = directory.join("main.js");
        let package = directory.join("package.json");
        std::fs::write(&source, "").unwrap();
        std::fs::write(&package, "{}").unwrap();
        let mut state = HmrState::new(source);
        let start = Instant::now();
        assert!(state.poll(start).is_none());
        std::fs::write(&package, r#"{"type":"module"}"#).unwrap();
        assert!(state.poll(start + POLL_INTERVAL).is_none());
        let stable = start + POLL_INTERVAL + STABLE_INTERVAL.max(POLL_INTERVAL);
        assert!(matches!(state.poll(stable), Some(Update::Restart)));
    }
}
