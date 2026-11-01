//! CommonJS host policy. Guest code, objects and calls belong to the shared VM.

use crate::host::{EntryGoal, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};

/// The global key of the host's builtin `require`, which host code and bootstrap scripts
/// reach builtins and Node library modules through.
pub(crate) const BOOTSTRAP_REQUIRE: &str = "\0quench:require";

type Context<'a> = NativeContext<'a, NodeHost>;

// Node's public CommonJS wrapper; this is guest compilation input, not an API shim.
const WRAPPER_PREFIX: &str = "(function (exports, require, module, __filename, __dirname) { ";
const WRAPPER_SUFFIX: &str = "\n});";

/// A specifier naming a package rather than a relative or absolute path.
fn is_bare_specifier(specifier: &str) -> bool {
    !specifier.starts_with('.') && !Path::new(specifier).is_absolute()
}

/// Node's `require` export conditions: `node-addons` is present unless `--no-addons`.
fn commonjs_condition_names(addons_enabled: bool) -> Vec<String> {
    let mut condition_names = vec!["node".into()];
    if addons_enabled {
        condition_names.push("node-addons".into());
    }
    condition_names.extend(["require".into(), "default".into()]);
    condition_names
}

/// The require resolver's options: each ancestor's `node_modules`, then Node's global paths.
fn commonjs_resolve_options(addons_enabled: bool) -> oxc_resolver::ResolveOptions {
    let modules = std::iter::once(NODE_MODULES.to_owned())
        .chain(
            crate::modules::module_shared_vm::global_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned()),
        )
        .collect();
    oxc_resolver::ResolveOptions {
        extensions: vec![".js".into(), ".json".into(), ".node".into()],
        main_files: vec!["index".into()],
        condition_names: commonjs_condition_names(addons_enabled),
        modules,
        ..Default::default()
    }
}

thread_local! {
    /// The CommonJS resolver, kept for the thread as Node keeps `Module._pathCache` and its
    /// package config reader for the process: its cache holds each path's metadata and
    /// `package.json`, which a resolver made per require read again (undici: its
    /// `package.json` 309 times). Node enables addons by default.
    static COMMONJS_RESOLVER: oxc_resolver::Resolver =
        oxc_resolver::Resolver::new(commonjs_resolve_options(true));
    /// The `--no-addons` CommonJS resolver, sharing the default resolver's cache.
    static NO_ADDONS_COMMONJS_RESOLVER: oxc_resolver::Resolver =
        COMMONJS_RESOLVER.with(|resolver| resolver.clone_with_options(commonjs_resolve_options(false)));
}

pub(crate) fn with_commonjs_resolver<R>(use_resolver: impl FnOnce(&oxc_resolver::Resolver) -> R) -> R {
    with_commonjs_resolver_for(true, use_resolver)
}

/// Runs `use_resolver` with the CommonJS resolver of the `--addons` setting.
pub(crate) fn with_commonjs_resolver_for<R>(
    addons_enabled: bool,
    use_resolver: impl FnOnce(&oxc_resolver::Resolver) -> R,
) -> R {
    if addons_enabled {
        COMMONJS_RESOLVER.with(use_resolver)
    } else {
        NO_ADDONS_COMMONJS_RESOLVER.with(use_resolver)
    }
}

thread_local! {
    /// The resolver of an ES module's imports. It shares the CommonJS resolver's cache, as
    /// Node's loaders share what they learned of the filesystem: a path either one resolved
    /// keeps its real path for the other (a symlink retargeted after a `require` still names
    /// its old target for an `import`, and the reverse).
    static IMPORT_RESOLVER: oxc_resolver::Resolver = COMMONJS_RESOLVER.with(|resolver| {
        resolver.clone_with_options(oxc_resolver::ResolveOptions {
            extensions: vec![".mjs".into(), ".js".into(), ".json".into(), ".cjs".into()],
            main_files: vec!["index".into()],
            condition_names: vec!["node".into(), "import".into(), "default".into()],
            ..Default::default()
        })
    });
}

/// Runs `use_resolver` with the resolver of an ES module's imports.
pub(crate) fn with_import_resolver<R>(use_resolver: impl FnOnce(&oxc_resolver::Resolver) -> R) -> R {
    IMPORT_RESOLVER.with(use_resolver)
}

/// Resolves through a kept resolver. One that finds nothing clears its cache and resolves
/// again, so a file created since a lookup cached its absence is found: Node keeps resolved
/// requests, never failed ones.
pub(crate) fn resolve_kept<T, E>(
    resolver: &oxc_resolver::Resolver,
    resolve: impl Fn(&oxc_resolver::Resolver) -> Result<T, E>,
) -> Result<T, E> {
    resolve(resolver).or_else(|_| {
        resolver.clear_cache();
        resolve(resolver)
    })
}

/// The directory each ancestor of a requester searches for packages.
const NODE_MODULES: &str = "node_modules";

/// An existing explicit filesystem target needs no package or extension search.
/// URL delimiters and percent escapes stay with the full resolver.
pub(super) fn exact_file_path(base: &Path, specifier: &str) -> Option<PathBuf> {
    if specifier
        .chars()
        .any(|character| matches!(character, '%' | '?' | '#'))
        || !(Path::new(specifier).is_absolute()
            || specifier.starts_with("./")
            || specifier.starts_with("../"))
    {
        return None;
    }
    let path = base.join(specifier);
    path.is_file().then_some(path)
}

pub(crate) fn entry_path(path: &Path, addons_enabled: bool) -> Result<PathBuf, String> {
    let cwd = if path.is_absolute() {
        path.parent()
            .unwrap_or_else(|| Path::new("/"))
            .to_path_buf()
    } else {
        std::env::current_dir().map_err(|error| error.to_string())?
    };
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    if let Some(path) = exact_file_path(&cwd, &path.to_string_lossy()) {
        return crate::host::real_path::real_path(&path).map_err(|error| error.to_string());
    }
    with_commonjs_resolver_for(addons_enabled, |resolver| {
        resolve_kept(resolver, |resolver| resolver.resolve(&cwd, &path.to_string_lossy()))
    })
        .map(|resolution| resolution.full_path())
        .map_err(|error| error.to_string())
}

/// Match Node's tryGetCwd fallback for eval and stdin module setup.
fn inline_directory(context: &mut Context<'_>) -> Result<PathBuf, RootedError> {
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    if let Ok(path) = cwd.query_path() {
        return Ok(path);
    }
    std::env::current_exe()
        .map(|path| {
            path.parent()
                .unwrap_or_else(|| Path::new("/"))
                .to_path_buf()
        })
        .map_err(|error| RootedError::host(error.to_string()))
}

/// Node's explicit extension/package parse-goal policy, before guest execution.
pub(crate) fn source_kind(path: &Path) -> Result<quench_runtime::SourceKind, String> {
    let path = real_path_for_kind(path)?;
    source_kind_of_real(&path)
}

/// The path whose package scope decides `path`'s goal: `.js`, `.ts` or an extensionless file
/// asks its package scope, which is its real path's.
fn real_path_for_kind(path: &Path) -> Result<std::borrow::Cow<'_, Path>, String> {
    if matches!(path.extension().and_then(|extension| extension.to_str()), Some("js" | "ts") | None) {
        return crate::host::real_path::real_path(path).map(std::borrow::Cow::Owned).map_err(|error| error.to_string());
    }
    Ok(std::borrow::Cow::Borrowed(path))
}

const SOURCE_KIND_DETECTION_UNAVAILABLE: &str = "native guest code required: source-kind detection is unavailable";

/// `source_kind` of a path that is already its real path, as a module the loader resolved.
pub(crate) fn source_kind_of_real(path: &Path) -> Result<quench_runtime::SourceKind, String> {
    match declared_source_kind(path)? {
        Some(kind) => Ok(kind),
        None => {
            if quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY {
                return Err(SOURCE_KIND_DETECTION_UNAVAILABLE.into());
            }
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            Ok(detect_source_kind_for_path(path, &source_text(&bytes)))
        }
    }
}

/// `source_kind_of_real` of a file whose bytes the caller already holds: detecting from
/// them keeps a second copy of a large file (typescript.js, 9 MB) from being read alongside.
pub(crate) fn source_kind_of_real_bytes(path: &Path, bytes: &[u8]) -> Result<quench_runtime::SourceKind, String> {
    match declared_source_kind(path)? {
        Some(kind) => Ok(kind),
        None if quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY =>
            Err(SOURCE_KIND_DETECTION_UNAVAILABLE.into()),
        None => Ok(detect_source_kind_for_path(path, &source_text(bytes))),
    }
}

fn detect_source_kind_for_path(path: &Path, text: &str) -> quench_runtime::SourceKind {
    if path.extension().is_some_and(|extension| extension == "ts") {
        let body = script_body(text);
        let Some((projected, has_module_declarations)) = quench_runtime::Engine::project_typescript_module_syntax(body) else {
            return quench_runtime::SourceKind::Script;
        };
        if has_module_declarations { return quench_runtime::SourceKind::Module; }
        let wrapper = [WRAPPER_PREFIX, projected.as_ref(), WRAPPER_SUFFIX].concat();
        if valid_source(&wrapper, oxc_span::SourceType::cjs().with_typescript(true)) {
            return quench_runtime::SourceKind::Script;
        }
        return quench_runtime::SourceKind::Module;
    }
    detect_source_kind(text)
}

/// The goal a real path's extension or package scope declares; none when Node's syntax
/// detection of its text decides (`detect_source_kind`).
fn declared_source_kind(path: &Path) -> Result<Option<quench_runtime::SourceKind>, String> {
    match path.extension().and_then(|extension| extension.to_str()) {
        // Node's `wasm` format is an ES module (`esm_wasm`).
        Some("mjs" | "mts" | "wasm") => return Ok(Some(quench_runtime::SourceKind::Module)),
        Some("cjs" | "cts" | "json" | "node") => return Ok(Some(quench_runtime::SourceKind::Script)),
        Some(extension) if extension != "js" && extension != "ts" => return Ok(None),
        _ => {}
    }
    // Package scope is a filesystem/JSON question, not another resolution of
    // this already-resolved file. In particular, do not build a resolver cache
    // (with parsed exports maps) for every source-kind query.
    for directory in path.ancestors().skip(1) {
        if directory
            .file_name()
            .is_some_and(|name| name == "node_modules")
        {
            break;
        }
        let package = directory.join("package.json");
        let Some(explicit) = package_type(package)? else {
            continue;
        };
        return Ok(explicit);
    }
    Ok(None)
}

/// Node first accepts the CommonJS wrapper. Only syntax that cannot live in
/// that wrapper triggers ESM detection, including top-level await and lexical
/// declarations of wrapper parameters. OXC owns both grammar and early errors.
/// What a package scope's `package.json` declares: none when the file does not exist,
/// else its `"type"` (none for any other string or none given), or why it is invalid.
type PackageType = Result<Option<Option<quench_runtime::SourceKind>>, String>;

thread_local! {
    /// Each `package.json` is read once per thread, as Node's package config reader caches
    /// each file it reads; a package with many modules (date-fns: 316 imports under one
    /// 199 KB `package.json`) otherwise parsed it again for every module. Keyed by the
    /// path's text: `Path`'s own hash walks its components.
    static PACKAGE_TYPES: std::cell::RefCell<rustc_hash::FxHashMap<std::ffi::OsString, PackageType>> =
        Default::default();
}

fn package_type(package: std::path::PathBuf) -> PackageType {
    if let Some(known) = PACKAGE_TYPES.with(|types| types.borrow().get(package.as_os_str()).cloned()) {
        return known;
    }
    let read = || -> PackageType {
        let text = match std::fs::read_to_string(&package) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{}: {error}", package.display())),
        };
        let package_json: serde_json::Value =
            serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text))
                .map_err(|_| format!("Invalid package config {}.", package.display()))?;
        match package_json.get("type") {
            Some(serde_json::Value::String(kind)) if kind == "module" => Ok(Some(Some(quench_runtime::SourceKind::Module))),
            Some(serde_json::Value::String(kind)) if kind == "commonjs" => Ok(Some(Some(quench_runtime::SourceKind::Script))),
            Some(serde_json::Value::String(_)) | None => Ok(Some(None)),
            Some(_) => Err(format!(
                "Invalid package config {}. \"type\" key must be a string",
                package.display()
            )),
        }
    };
    let known = read();
    PACKAGE_TYPES.with(|types| types.borrow_mut().insert(package.into_os_string(), known.clone()));
    known
}

/// A source file's text, replacing invalid UTF-8 as Node's decoder does. Valid text, nearly
/// every file, is checked by the standard library's faster validation and borrowed.
pub(crate) fn source_text(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => std::borrow::Cow::Borrowed(text),
        Err(_) => String::from_utf8_lossy(bytes),
    }
}

fn detect_source_kind(text: &str) -> quench_runtime::SourceKind {
    let text = script_body(text);
    // The goal follows from the text alone, which parsing it (twice, for a module) costs a
    // whole compilation of: typescript.js (9 MB) was parsed on every load. It is kept in the
    // compile cache, keyed by the exact text, as a compiled program is.
    let entry = text_facts_cache().entry(DETECTED_KIND_NAME, &[text]);
    if let Some(kind) = entry.as_ref().and_then(detected_kind_in) {
        return kind;
    }
    let kind = detect_text_kind(text);
    if let Some(entry) = entry {
        entry.store(&[detected_kind_byte(kind)]);
    }
    kind
}

/// The parse goal an earlier detection of `text` kept in the compile cache, computing none.
fn cached_source_kind(text: &str) -> Option<quench_runtime::SourceKind> {
    text_facts_cache().entry(DETECTED_KIND_NAME, &[script_body(text)]).as_ref().and_then(detected_kind_in)
}

fn detected_kind_in(entry: &crate::host::compile_cache::CacheEntry) -> Option<quench_runtime::SourceKind> {
    entry.load().and_then(|bytes| detected_kind_of(bytes.as_ref().as_ref()))
}

/// The compile-cache name a detected parse goal is kept under.
const DETECTED_KIND_NAME: &str = "\0quench:detected-source-kind";

/// The cache facts derived from a file's text alone are kept in (its detected parse goal,
/// its static CommonJS exports): this build's compile cache, read from the environment as
/// the host's own is, since they are derived where no host is reachable.
pub(super) fn text_facts_cache() -> &'static crate::host::compile_cache::CompileCache {
    static CACHE: std::sync::OnceLock<crate::host::compile_cache::CompileCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(crate::host::compile_cache::CompileCache::from_environment)
}

/// Each parse goal's one-byte cache record.
const DETECTED_KIND_BYTES: [(quench_runtime::SourceKind, u8); 2] =
    [(quench_runtime::SourceKind::Script, 0), (quench_runtime::SourceKind::Module, 1)];

fn detected_kind_byte(kind: quench_runtime::SourceKind) -> u8 {
    DETECTED_KIND_BYTES.iter().find(|(known, _)| *known == kind).map(|(_, byte)| *byte).expect("every parse goal has a record")
}

fn detected_kind_of(bytes: &[u8]) -> Option<quench_runtime::SourceKind> {
    let [byte] = bytes else { return None };
    DETECTED_KIND_BYTES.iter().find(|(_, known)| known == byte).map(|(kind, _)| *kind)
}

/// Node's syntax detection of `text`: the CommonJS wrapper first, then ES module syntax.
fn detect_text_kind(text: &str) -> quench_runtime::SourceKind {
    let wrapper = [WRAPPER_PREFIX, text, WRAPPER_SUFFIX].concat();
    if valid_source(&wrapper, oxc_span::SourceType::cjs()) {
        return quench_runtime::SourceKind::Script;
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::unambiguous()).parse();
    if parsed.program.source_type.is_module() || valid_source(text, oxc_span::SourceType::mjs()) {
        quench_runtime::SourceKind::Module
    } else {
        quench_runtime::SourceKind::Script
    }
}

/// Holds one function's tree at a time: the whole tree of a large file (typescript.js, 9 MB)
/// with its semantic tables was the peak of loading it.
fn valid_source(source: &str, source_type: oxc_span::SourceType) -> bool {
    quench_runtime::Engine::syntax_valid(source, source_type)
}

pub(super) fn initialize(context: &mut Context<'_>) -> Result<(), RootedError> {
    crate::modules::process_surface::initialize(context)?;
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key)?;
    // `process.release` is its own group: Node's warning listener reads it, and it needs
    // none of the surface script.
    let release = context.host_function(super::operation("processReleaseInitialize"))?;
    context.define_lazy_data_properties_rooted(process, &[PROCESS_RELEASE], true, release)?;
    context.release_root(release);
    let initializer = context.host_function(super::operation("processSurfaceInitialize"))?;
    context.define_lazy_data_properties_rooted(
        process,
        crate::modules::process_surface::LAZY_MEMBERS,
        true,
        initializer,
    )?;
    context.release_root(initializer);
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_process_surface");
    context.scoped(crate::modules::buffer_shared_vm::install_global)?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_buffer_global");
    crate::host::resident_pages::release_read_only_pages();
    context.scoped(install_shared_web_globals)?;
    context.scoped(crate::modules::util_shared_vm::install_lazy_globals)?;
    let global = context.global_root()?;
    // The process.getBuiltinModule bootstrap needs access to the host's
    // builtin loader in both Script and Module goals. Keep it under a private
    // global key so ESM does not acquire a user-visible global `require`.
    let bootstrap_filename =
        inline_directory(context)?.join(context.host_mut().inline_source.label());
    let bootstrap_require = create_require(context, &bootstrap_filename)?;
    set(context, global, BOOTSTRAP_REQUIRE, bootstrap_require)?;
    context.release_root(bootstrap_require);
    // Keep the host scheduler separate from Node's public queueMicrotask API.
    // Node's task_queues wrapper adds the AsyncResource lifecycle around each
    // callback, while internalBinding('task_queue') must remain a raw queue.
    let raw_queue_microtask = get(context, global, "queueMicrotask")?;
    let raw_queue_key = context.string_rooted("\0quench:queueMicrotaskRaw");
    context.define_data_property_rooted(global, raw_queue_key, raw_queue_microtask, false, false, true)?;
    let require = get(context, global, BOOTSTRAP_REQUIRE)?;
    let task_queues_name = context.string_rooted("internal/process/task_queues");
    let undefined = context.undefined();
    let task_queues = context.call_rooted(require, undefined, &[task_queues_name])?;
    let queue_microtask = get(context, task_queues, "queueMicrotask")?;
    set(context, global, "queueMicrotask", queue_microtask)?;
    let export_values = context.host_function(crate::host::shared_vm::operation("builtinExportValues"))?;
    let key = context.string_rooted(BUILTIN_EXPORT_VALUES);
    context.define_data_property_rooted(global, key, export_values, false, false, false)?;
    let instantiate_wasm = context.host_function(crate::host::shared_vm::operation("esmWasmInstantiate"))?;
    let key = context.string_rooted(super::esm_wasm::INSTANTIATE);
    context.define_data_property_rooted(global, key, instantiate_wasm, false, false, false)?;
    let print_eval = context.host_function(crate::host::shared_vm::operation("nodePrintEvalScript"))?;
    let key = context.string_rooted("\0quench:evalScript");
    context.define_data_property_rooted(global, key, print_eval, false, false, false)?;
    context.scoped(install_performance_global)?;
    #[cfg(quench_openssl_provider)]
    context.scoped(crate::modules::crypto_node::install_global)?;
    if node_primordials_source(context).is_some() {
        // Node 24 exposes these well-known disposal symbols before its
        // primordial snapshot is built. The runtime may not provide them yet,
        // so seed stable symbols for Node's internal modules first.
        let seed_disposal_symbols = crate::host::removed_javascript("node:bootstrap/disposal-symbols.js")?;
        let undefined = context.undefined();
        context.call_rooted(seed_disposal_symbols, undefined, &[])?;
    }
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_primordials");
    // Builtins built after this point see the intrinsics as they are now, whatever a
    // preload or the entry script does to them (`cached_builtin`).
    context.seal_startup_intrinsics();
    // The SDK console reads its primordial snapshot. It loads on the first read of
    // `console`, inside the startup intrinsics, so it sees the original globals whatever a
    // preload or the entry script has done; most of a run that never logs never loads it.
    let initializer = context.host_function(crate::host::shared_vm::operation("consoleGlobalInitialize"))?;
    context.define_lazy_data_properties_rooted(global, &["console"], false, initializer)?;
    context.seal_startup_intrinsics();
    let expose_event_source = context
        .host_mut()
        .shared_state()
        .borrow()
        .exec_argv
        .iter()
        .any(|argument| argument == "--experimental-eventsource");
    if expose_event_source {
        let _ = node_primordials(context)?;
        let require = get(context, global, "\0quench:require")?;
        let specifier = context.string_rooted("internal/deps/undici/undici");
        let undefined = context.undefined();
        let undici = context.call_rooted(require, undefined, &[specifier])?;
        let event_source = get(context, undici, "EventSource")?;
        set(context, global, "EventSource", event_source)?;
    }
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_console_global");
    if context.host_mut().vfs_enabled() {
        // Feature bootstrap loads real builtins that capture these originals.
        // Seal its added globals only after the guarded initialization leaves.
        context.with_startup_intrinsics(install_vfs_globals)?;
        context.seal_startup_intrinsics();
    }
    crate::host::resident_pages::release_read_only_pages();
    if node_primordials_source(context).is_some() {
        context.scoped(crate::modules::node_errors_shared_vm::install_stack_formatter)?;
        // Node's policies hold the validators and error codes they import. Only a script
        // that can require internal modules can replace those exports before a policy's
        // first use, so only then are the policies captured before it runs.
        if internals_exposed(context) {
            // Capture SDK process policies before exposed internal exports can be replaced.
            #[cfg(target_os = "linux")]
            {
                let cpu_usage = get(context, process, "cpuUsage")?;
                context.release_root(cpu_usage);
            }
        }
        context.seal_startup_intrinsics();
    }
    #[cfg(feature = "aot")]
    if let Some(demands) = context.host_mut().native_source_staging.take() {
        return crate::aot::stage_host_sources(context, demands);
    }
    if let Some(kind) = context.host_mut().building_snapshot {
        return crate::host::startup_snapshot::preload(context, kind);
    }
    finish_initialize(context)
}

/// The part of `initialize` after the startup snapshot point (`startup_snapshot`): what
/// this process's options, preloads and entry install.
pub(super) fn finish_initialize(context: &mut Context<'_>) -> Result<(), RootedError> {
    context.scoped(crate::modules::process_shared_vm::initialize_warning_policy)?;
    context.scoped(crate::modules::report_configuration_shared_vm::initialize_signal)?;
    context.scoped(crate::modules::worker_threads_shared_vm::initialize_process_owner)?;
    // IPC and cluster startup precede all user preloads and synchronous entry.
    context.scoped(crate::modules::child_process_ipc_shared_vm::activate_process)?;
    context.scoped(initialize_cluster_worker)?;
    // Node exposes these web and timer globals as enumerable own properties.
    // The host bindings and several bootstrap polyfills use non-enumerable
    // descriptors by default, so normalize their public descriptors before
    // user preloads inspect globalThis. Node 26 also provides navigator by
    // default; retain the standard getter shape even when the engine lacks it.
    context.scoped(install_global_descriptors)?;
    context.scoped(preload_modules)?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_preloads");
    if context.host_mut().eval_builtin_globals {
        context.scoped(crate::modules::eval_globals_shared_vm::install)?;
    }
    if context.is_module()? {
        return Ok(());
    }
    if let Some(entry) = context.host_mut().commonjs_entry.clone() {
        #[cfg(feature = "native-execution")]
        let filename = context.host_mut().module_path(&entry.path).map_err(|error| RootedError::host(error.to_string()))?;
        #[cfg(not(feature = "native-execution"))]
        let filename = std::fs::canonicalize(entry.path).map_err(|error| RootedError::host(error.to_string()))?;
        let module = cached_builtin(context, BuiltinModule::Module)?;
        let run_main = get(context, module, "runMain")?;
        let filename = context.string_rooted(&filename.to_string_lossy());
        context.call_rooted(run_main, module, &[filename])?;
    } else {
        let filename = inline_directory(context)?.join(context.host_mut().inline_source.label());
        let module = module_record(context, &filename, None, false)?;
        let label = context.host_mut().inline_source.label();
        let label = context.string_rooted(label);
        set(context, module, "id", label)?;
        let global = context.global_root()?;
        for name in ["exports", "require"] {
            let value = get(context, module, name)?;
            set(context, global, name, value)?;
        }
        set(context, global, "module", module)?;
        set(context, global, "__filename", label)?;
        let dirname = context.string_rooted(".");
        set(context, global, "__dirname", dirname)?;
    }
    Ok(())
}

/// CLI preloads execute in order through the ordinary CommonJS loader/cache.
fn preload_modules(context: &mut Context<'_>) -> Result<(), RootedError> {
    // The shared option parser combines NODE_OPTIONS with the invocation
    // arguments, preserving repeated --require flags in startup order.
    let requires = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .repeated_values()
        .find_map(|(name, values)| (name == "--require").then(|| values.to_vec()))
        .unwrap_or_default();
    let global = context.global_root()?;
    let require = get(context, global, BOOTSTRAP_REQUIRE)?;
    let undefined = context.undefined();
    for specifier in requires {
        let specifier = context.string_rooted(&specifier);
        context.call_rooted(require, undefined, &[specifier])?;
    }
    Ok(())
}

fn install_vfs_globals(context: &mut Context<'_>) -> Result<(), RootedError> {
    // The configured SDK owns public VFS policy; retain only native descriptor
    // and stream bridges here, without evaluating the superseded JS facade.
    cached_builtin(context, BuiltinModule::Fs)?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_vfs_fs_module");
    let path = crate::modules::path_shared_vm::module(context)?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_vfs_path_module");
    let stream = cached_builtin(context, BuiltinModule::Stream)?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_vfs_stream_module");
    let install = crate::host::removed_javascript("node:bootstrap/vfs-globals.js")?;
    let undefined = context.undefined();
    context.call_rooted(install, undefined, &[path, stream])?;
    let vfs_constructor = node_library_export(
        context,
        "vfs.js",
        "internal/vfs/file_system.js",
        "VirtualFileSystem",
    )?;
    let install_loader_projection = crate::host::removed_javascript("node:bootstrap/vfs-module-loader-projection.js")?;
    context.call_rooted(
        install_loader_projection,
        undefined,
        &[vfs_constructor],
    )?;
    let global = context.global_root()?;
    for (name, export) in [
        ("__quenchVfsReadStream", "VirtualReadStream"),
        ("__quenchVfsWriteStream", "VirtualWriteStream"),
    ] {
        let constructor = node_library_export(
            context,
            "internal/vfs/file_system.js",
            "internal/vfs/streams.js",
            export,
        )?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, constructor, false, false, true)?;
    }
    let registry = get(context, global, "__quenchVfsFdHandles")?;
    let map_get = get(context, registry, "get")?;
    let map_set = get(context, registry, "set")?;
    let map_methods = context.array_rooted(&[map_get, map_set])?;
    let bind_position =
        context.host_function_with_data(super::operation("vfsBindPosition"), map_methods)?;
    let key = context.string_rooted("__quenchVfsBindPosition");
    context.define_data_property_rooted(global, key, bind_position, false, false, true)?;
    let path_projection = context.host_function(super::operation("vfsStreamPath"))?;
    let key = context.string_rooted("__quenchVfsStreamPath");
    context.define_data_property_rooted(global, key, path_projection, false, false, true)?;
    for (name, operation) in [
        ("__quenchVfsOpenDescriptor", "vfsDescriptorOpen"),
        ("__quenchVfsCloseDescriptor", "vfsDescriptorRelease"),
    ] {
        let function = context.host_function(super::operation(operation))?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, function, false, false, true)?;
    }
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_vfs_globals");

    Ok(())
}

/// Source owns virtual descriptor allocation; legacy real-provider records
/// remain actual OS descriptors in the disjoint, unmasked descriptor range.
pub(crate) fn vfs_descriptor_get(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    vfs_descriptor_lookup(context, args, false)
}
pub(crate) fn vfs_descriptor_close(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    vfs_descriptor_lookup(context, args, true)
}
fn vfs_descriptor_lookup(
    context: &mut Context<'_>,
    args: &[RootId],
    closing: bool,
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let source = get(context, data, "0")?;
    let missing = context.undefined();
    let value = context.call_rooted(source, missing, args)?;
    let found = if closing {
        context.truthy_rooted(value)?
    } else {
        !context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
    };
    if found {
        return Ok(value);
    }
    let legacy = get(context, data, "1")?;
    let method = get(context, data, "2")?;
    context.call_rooted(method, legacy, args)
}
pub(crate) fn vfs_descriptor_open(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let module = cached_builtin(context, BuiltinModule::InternalVfsFd)?;
    let open = get(context, module, "openVirtualFd")?;
    let missing = context.undefined();
    context.call_rooted(open, missing, args)
}
pub(crate) fn vfs_descriptor_release(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let module = cached_builtin(context, BuiltinModule::InternalVfsFd)?;
    let get_fd = get(context, module, "getVirtualFd")?;
    let missing = context.undefined();
    let record = context.call_rooted(get_fd, missing, args)?;
    if context.is_object_rooted(record)? {
        let entry = get(context, record, "entry")?;
        let close = get(context, entry, "closeSync")?;
        context.call_rooted(close, entry, &[])?;
    }
    let close = get(context, module, "closeVirtualFd")?;
    context.call_rooted(close, missing, args)
}

/// Public fs streams retain the caller's path while provider I/O uses its mount-relative path.
pub(crate) fn vfs_stream_path(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let stream = args.first().copied().unwrap_or(missing);
    let path = args.get(1).copied().unwrap_or(missing);
    let key = context.string_rooted("path");
    context.define_data_property_rooted(stream, key, path, true, true, true)?;
    Ok(stream)
}

/// Project the descriptor position onto the provider's existing offset authority.
pub(crate) fn vfs_bind_position(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let map = args.first().copied().unwrap_or(missing);
    let fd = args.get(1).copied().unwrap_or(missing);
    let entry = args.get(2).copied().unwrap_or(missing);
    let methods = context.host_function_data()?;
    let getter = get(context, methods, "0")?;
    let setter = get(context, methods, "1")?;
    let data = context.array_rooted(&[map, fd, getter, setter])?;
    let getter = context.host_function_with_data(super::operation("vfsPositionGet"), data)?;
    let setter = context.host_function_with_data(super::operation("vfsPositionSet"), data)?;
    let descriptor = context.object_rooted()?;
    for (name, value) in [("get", getter), ("set", setter)] {
        self::set(context, descriptor, name, value)?;
    }
    let key = context.string_rooted("position");
    context.define_property_descriptor_rooted(entry, key, descriptor)?;
    Ok(entry)
}

pub(crate) fn vfs_position_get(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let map = get(context, data, "0")?;
    let fd = get(context, data, "1")?;
    let getter = get(context, data, "2")?;
    context.call_rooted(getter, map, &[fd])
}

pub(crate) fn vfs_position_set(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let map = get(context, data, "0")?;
    let fd = get(context, data, "1")?;
    let setter = get(context, data, "3")?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    context.call_rooted(setter, map, &[fd, value])?;
    Ok(context.undefined())
}

/// The timeline classes Node exposes as globals, from `perf_hooks`.
const PERFORMANCE_GLOBALS: [&str; 6] = [
    "PerformanceEntry",
    "PerformanceMark",
    "PerformanceMeasure",
    "PerformanceObserver",
    "PerformanceObserverEntryList",
    "PerformanceResourceTiming",
];

/// Node's replaceable `performance` global and the timeline classes load `perf_hooks` on
/// first access.
fn install_performance_global(context: &mut Context<'_>) -> Result<(), RootedError> {
    install_lazy_global(context, "performance", "perf_hooks", "performance", true)?;
    for name in PERFORMANCE_GLOBALS {
        install_lazy_global(context, name, "perf_hooks", name, false)?;
    }
    Ok(())
}

/// The globals each web polyfill defines, in definition order. They are lazy data properties
/// (as in Node), so a polyfill is evaluated only when script first observes one of its globals.
/// A member materializes writable, so a polyfill defining one read-only says so explicitly.
pub(crate) const WEB_STREAM_GLOBALS: &[&str] = &[
    "ReadableStream",
    "WritableStream",
    "TransformStream",
    "CompressionStream",
    "DecompressionStream",
    "TextEncoderStream",
    "TextDecoderStream",
    "ByteLengthQueuingStrategy",
    "CountQueuingStrategy",
    "ReadableStreamDefaultReader",
    "ReadableStreamBYOBReader",
    "ReadableStreamBYOBRequest",
    "ReadableByteStreamController",
    "ReadableStreamDefaultController",
    "TransformStreamDefaultController",
    "WritableStreamDefaultWriter",
    "WritableStreamDefaultController",
];
// Blob/File are independent of Fetch. Importing buffer must not recursively
// load undici while the public Buffer binding is still being initialized.
pub(crate) const WEB_API_GLOBALS: &[&str] = &[
    "__quenchBlobEnsureReadable",
    "Blob",
    "File",
];
pub(crate) const FETCH_API_GLOBALS: &[&str] = &[
    "FormData",
    "Headers",
    "Request",
    "Response",
    "MessageEvent",
    "CloseEvent",
    "ErrorEvent",
    "WebSocket",
];

fn install_shared_web_globals(context: &mut Context<'_>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    for (operation, names) in [
        ("webStreamsInitialize", WEB_STREAM_GLOBALS),
        ("webApisInitialize", WEB_API_GLOBALS),
        ("webFetchInitialize", FETCH_API_GLOBALS),
    ] {
        let names: Vec<&str> = names.iter().copied().filter(|name| web_global_enabled(context, name)).collect();
        let initializer = context.host_function(super::operation(operation))?;
        context.define_lazy_data_properties_rooted(global, &names, false, initializer)?;
        context.release_root(initializer);
    }
    Ok(())
}

/// The WebSocket globals Node installs only under `--experimental-websocket`, on by default.
const WEBSOCKET_GLOBALS: [&str; 2] = ["WebSocket", "CloseEvent"];

/// Whether the process's options expose the web global `name` (Node's bootstrap gates the
/// WebSocket family on `--experimental-websocket`).
fn web_global_enabled(context: &mut Context<'_>, name: &str) -> bool {
    !WEBSOCKET_GLOBALS.contains(&name)
        || context.host_mut().shared_state().borrow().cli_options.enabled("--experimental-websocket")
}


pub(super) fn initialize_web_streams(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = cached_builtin(context, BuiltinModule::WebStreams)?;
    let global = context.global_root()?;
    for name in WEB_STREAM_GLOBALS {
        let value = get(context, module, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, value, true, false, true)?;
    }
    Ok(context.undefined())
}

/// `process.release`'s key and its members (`process.release.name` is Node's runtime name).
const PROCESS_RELEASE: &str = "release";
const RELEASE_MEMBERS: [(&str, &str); 3] = [("name", "node"), ("sourceUrl", ""), ("headersUrl", "")];

/// Defines `process.release`.
pub(super) fn initialize_process_release(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key)?;
    let release = context.object_rooted()?;
    for (name, value) in RELEASE_MEMBERS {
        let key = context.string_rooted(name);
        let value = context.string_rooted(value);
        context.define_data_property_rooted(release, key, value, true, true, true)?;
    }
    let key = context.string_rooted(PROCESS_RELEASE);
    context.define_data_property_rooted(process, key, release, true, true, true)?;
    Ok(context.undefined())
}

pub(super) fn initialize_process_surface(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    crate::modules::process_memory_shared_vm::install(context)?;
    crate::host::removed_javascript("node:bootstrap/process-surface-lazy.js")?;
    #[cfg(unix)]
    crate::modules::process_metrics_shared_vm::install(context)?;
    Ok(context.undefined())
}

pub(super) fn initialize_web_apis(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let expose_web_worker = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--experimental-web-worker");
    if expose_web_worker {
        // Node exposes the browser-style Worker global only when the
        // experimental web worker option is enabled. Use Node's own lazy
        // interface helper so the global has the same accessor semantics and
        // the implementation remains lazily loaded.
        let expose_lazy_interfaces = node_library_export(
            context,
            "internal/util.js",
            "internal/util.js",
            "exposeLazyInterfaces",
        )?;
        let global = context.global_root()?;
        let id = context.string_rooted("internal/webworker");
        let key = context.string_rooted("Worker");
        let keys = context.array_rooted(&[key])?;
        let undefined = context.undefined();
        context.call_rooted(expose_lazy_interfaces, undefined, &[global, id, keys])?;
    }
    crate::host::removed_javascript("node:bootstrap/shared-vm/web-apis.js")?;
    // Keep the global constructor identical to Node's internal EventTarget
    // export. This is observable in Node's own global-surface assertions and
    // avoids a second, subtly incompatible CustomEvent implementation.
    let custom_event = node_library_export(
        context,
        "internal/event_target.js",
        "internal/event_target.js",
        "CustomEvent",
    )?;
    let global = context.global_root()?;
    let key = context.string_rooted("CustomEvent");
    context.define_data_property_rooted(global, key, custom_event, true, false, true)?;
    Ok(context.undefined())
}

pub(super) fn initialize_fetch_apis(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // Execute Node's pinned implementation in this VM, sharing its WebIDL and
    // Fetch algorithms rather than maintaining a second partial implementation.
    let global = context.global_root()?;
    let root = context.host_mut().node_library_root.clone();
    let path = root.join("deps/undici/undici.js");
    let parent = module_record(context, &root.join("lib/internal/fetch.js"), None, false)?;
    let undici = load(context, &path, Some(parent), EntryGoal::CommonJs)?;
    for name in [
        "Headers",
        "Request",
        "Response",
        "FormData",
        "MessageEvent",
        "CloseEvent",
        "ErrorEvent",
        "WebSocket",
    ] {
        if !web_global_enabled(context, name) {
            continue;
        }
        let constructor = get(context, undici, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, constructor, true, false, true)?;
    }
    // The bootstrap fetch wrapper issues requests through Node's HTTP client.
    // ESM fixtures have no global `require`, so provide the same builtin client
    // explicitly instead of letting the wrapper fail before making a request.
    let http = cached_builtin(context, BuiltinModule::Http)?;
    let key = context.string_rooted("__nodeHttp");
    context.define_data_property_rooted(global, key, http, false, false, true)?;
    Ok(context.undefined())
}

pub(crate) fn perf_hooks_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::PerfHooks)
}

pub(super) fn module_run_main(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(filename) = args.first().copied() else {
        return Err(RootedError::host("Module.runMain entry path is missing"));
    };
    let filename = context
        .string_text(filename)?
        .ok_or_else(|| RootedError::host("Module.runMain entry path is not a string"))?;
    let goal = context
        .host_mut()
        .commonjs_entry
        .as_ref()
        .map_or(EntryGoal::Node, |entry| entry.goal);
    let loaded = load(context, Path::new(&filename), None, goal)?;
    context.release_root(loaded);
    Ok(context.undefined())
}

pub(super) fn require(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let parent = context.host_function_data()?;
    require_for_module(context, parent, args)
}

pub(crate) fn require_for_module(
    context: &mut Context<'_>,
    parent: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let requested = specifier(context, args, Request::Require)?;
    let already_tracing = context
        .host_mut()
        .shared_state()
        .borrow()
        .module_require_trace_active;
    if already_tracing {
        return require_for_module_inner(context, parent, args);
    }
    let subscribed = context
        .host_mut()
        .shared_state()
        .borrow()
        .diagnostics
        .channels
        .get("tracing:module.require:start")
        .is_some_and(|subscribers| !subscribers.is_empty());
    if !subscribed {
        return require_for_module_inner(context, parent, args);
    }

    let parent_filename = get(context, parent, "filename")?;
    let node_library_root = context.host_mut().node_library_root.clone();
    let is_node_library_module = context
        .string_text(parent_filename)?
        .is_some_and(|filename| {
            filename.starts_with(node_library_root.join("lib").to_string_lossy().as_ref())
        });
    if is_node_library_module {
        context.release_root(parent_filename);
        return require_for_module_inner(context, parent, args);
    }
    let event = context.object_rooted()?;
    let requested_value = context.string_rooted(&requested);
    set(context, event, "id", requested_value)?;
    context.release_root(requested_value);
    set(context, event, "parentFilename", parent_filename)?;
    context.release_root(parent_filename);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .module_require_trace_active = true;
    let start_publish = crate::modules::diagnostics_channel_shared_vm::publish_named(
        context,
        "tracing:module.require:start",
        event,
    );
    if let Err(error) = start_publish {
        context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .module_require_trace_active = false;
        context.release_root(event);
        return Err(error);
    }
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .module_require_trace_active = false;

    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .module_require_trace_active = true;
    let result = require_for_module_inner(context, parent, args);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .module_require_trace_active = false;
    match result {
        Ok(exports) => {
            set(context, event, "result", exports)?;
            context
                .host_mut()
                .shared_state()
                .borrow_mut()
                .module_require_trace_active = true;
            let end_publish = crate::modules::diagnostics_channel_shared_vm::publish_named(
                context,
                "tracing:module.require:end",
                event,
            );
            context
                .host_mut()
                .shared_state()
                .borrow_mut()
                .module_require_trace_active = false;
            context.release_root(event);
            end_publish?;
            Ok(exports)
        }
        Err(error) => {
            if let Some(exception) = error.exception {
                set(context, event, "error", exception)?;
                context
                    .host_mut()
                    .shared_state()
                    .borrow_mut()
                    .module_require_trace_active = true;
                let _ = crate::modules::diagnostics_channel_shared_vm::publish_named(
                    context,
                    "tracing:module.require:error",
                    event,
                );
                let _ = crate::modules::diagnostics_channel_shared_vm::publish_named(
                    context,
                    "tracing:module.require:end",
                    event,
                );
            }
            context
                .host_mut()
                .shared_state()
                .borrow_mut()
                .module_require_trace_active = false;
            context.release_root(event);
            Err(error)
        }
    }
}

fn require_for_module_inner(
    context: &mut Context<'_>,
    parent: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Require)?;
    let parent_path = get(context, parent, "path")?;
    let api = module_parent_api(context)?;
    let paths = get(context, api, "relativePaths")?;
    let map_get = get(context, api, "mapGet")?;
    let mut directory_cache = context.call_rooted(map_get, paths, &[parent_path])?;
    if !context
        .rooted_value(directory_cache)
        .is_some_and(|value| value.is_undefined())
    {
        let resolved = get(context, directory_cache, &specifier)?;
        if let Some(filename) = context.string_text(resolved)? {
            let cache = commonjs_cache(context)?;
            let cached = get(context, cache, &filename)?;
            if !context
                .rooted_value(cached)
                .is_some_and(|value| value.is_undefined())
            {
                if context
                    .host_mut()
                    .shared_state()
                    .borrow()
                    .active_esm_requires
                    .contains_key(&filename)
                {
                    return load(context, Path::new(&filename), Some(parent), EntryGoal::Node);
                }
                transition_child(context, parent, cached, ChildTransition::Attach)?;
                return get(context, cached, "exports");
            }
            delete_relative_resolution(context, directory_cache, &specifier)?;
        }
    }
    // Node's explicit node: spelling bypasses CommonJS cache substitutions.
    // Bare builtins participate in Module._cache just like resolved filenames.
    if !specifier.starts_with("node:")
        && !specifier.starts_with("internal/")
        && requested_builtin(context, &specifier).is_some()
    {
        let cache = commonjs_cache(context)?;
        let cached = get(context, cache, &specifier)?;
        if !context
            .rooted_value(cached)
            .is_some_and(|value| value.is_undefined())
        {
            transition_child(context, parent, cached, ChildTransition::Attach)?;
            return get(context, cached, "exports");
        }
    }
    if specifier == "vfs" {
        let error = context.error_rooted("Cannot find module 'vfs'")?;
        let code = context.string_rooted("MODULE_NOT_FOUND");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if matches!(specifier.as_str(), "node:stream/iter" | "node:zlib/iter")
        && !context
            .host_mut()
            .shared_state()
            .borrow()
            .cli_options
            .enabled("--experimental-stream-iter")
    {
        let message = format!("No such built-in module: {specifier}");
        let error = context.error_rooted(&message)?;
        let code = context.string_rooted("ERR_UNKNOWN_BUILTIN_MODULE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if matches!(specifier.as_str(), "node:bench" | "node:bench/reporters")
        && !context
            .host_mut()
            .shared_state()
            .borrow()
            .cli_options
            .enabled("--experimental-bench")
    {
        let message = format!("ERR_UNKNOWN_BUILTIN_MODULE: No such built-in module: {specifier}");
        let error = context.error_rooted(&message)?;
        let code = context.string_rooted("ERR_UNKNOWN_BUILTIN_MODULE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if specifier == "node:vfs" && !context.host_mut().vfs_enabled() {
        let error = context
            .error_rooted("ERR_UNKNOWN_BUILTIN_MODULE: No such built-in module: node:vfs")?;
        let code = context.string_rooted("ERR_UNKNOWN_BUILTIN_MODULE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    // The `node:` scheme is reserved for built-ins. Do not fall through to
    // package/file resolution for an unknown `node:` specifier; Node reports
    // ERR_UNKNOWN_BUILTIN_MODULE for these (including node:internal/*).
    if specifier.starts_with("node:") && requested_builtin(context, &specifier).is_none() {
        let message = format!("No such built-in module: {specifier}");
        let error = context.error_rooted(&message)?;
        let code = context.string_rooted("ERR_UNKNOWN_BUILTIN_MODULE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if specifier == "internal/bootstrap/realm" {
        return crate::host::removed_javascript("internal/bootstrap/realm.js");
    }
    match requested_builtin(context, &specifier) {
        Some(BuiltinModule::Process) => {
            return match context.host_mut().shared_state().borrow().process_module {
                Some(root) => Ok(root),
                _ => Err(RootedError::host(
                    "shared process module is not initialized",
                )),
            };
        }
        Some(BuiltinModule::UtilTypes) => {
            return cached_builtin(context, BuiltinModule::UtilTypes);
        }
        Some(BuiltinModule::InternalUrl) => {
            return cached_builtin(context, BuiltinModule::InternalUrl);
        }
        Some(
            module @ BuiltinModule::Internal(
                "_http_agent" | "_http_client" | "_http_common" | "_http_incoming" | "_http_outgoing" | "_http_server",
            ),
        ) => {
            return cached_builtin(context, module);
        }
        Some(BuiltinModule::InternalVfsFd) => {
            return cached_builtin(context, BuiltinModule::InternalVfsFd);
        }
        Some(BuiltinModule::Vm) => {
            return cached_builtin(context, BuiltinModule::Vm);
        }
        Some(BuiltinModule::InternalUndici) => {
            let filename = context
                .host_mut()
                .node_library_root
                .join("deps/undici/undici.js");
            return load(context, &filename, Some(parent), EntryGoal::CommonJs);
        }
        Some(BuiltinModule::Dgram) => {
            return cached_builtin(context, BuiltinModule::Dgram);
        }
        Some(BuiltinModule::InternalDgram) => {
            return node_library_module(context, "internal/dgram.js", "internal/dgram.js");
        }
        Some(BuiltinModule::InternalTestBinding) => {
            // Unlike internal policy initialization, this guest-facing SDK bridge
            // must observe the current process.emitWarning and warning options.
            // Its intrinsic operations already come from the SDK primordials.
            let root = context.host_mut().node_library_root.clone();
            return load(
                context,
                &root.join("lib/internal/test/binding.js"),
                Some(parent),
                EntryGoal::CommonJs,
            );
        }
        Some(BuiltinModule::InternalBlockList) => {
            let module = context.object_rooted()?;
            let (constructor, _) = crate::modules::net_block_list::classes(context)?;
            set(context, module, "BlockList", constructor)?;
            return Ok(module);
        }
        Some(BuiltinModule::InternalSocketAddress) => {
            let module = context.object_rooted()?;
            let (_, constructor) = crate::modules::net_block_list::classes(context)?;
            set(context, module, "SocketAddress", constructor)?;
            return Ok(module);
        }
        Some(BuiltinModule::Assert) => {
            return crate::modules::assert_shared_vm::module(context);
        }
        Some(BuiltinModule::AssertStrict) => {
            return crate::modules::assert_shared_vm::strict_module(context);
        }
        Some(BuiltinModule::Path) => {
            return crate::modules::path_shared_vm::module(context);
        }
        Some(BuiltinModule::PathPosix) => {
            return crate::modules::path_shared_vm::posix(context);
        }
        Some(BuiltinModule::PathWin32) => {
            return crate::modules::path_shared_vm::win32(context);
        }
        Some(BuiltinModule::Url) => {
            return cached_builtin(context, BuiltinModule::Url);
        }
        Some(BuiltinModule::Readline) => {
            return cached_builtin(context, BuiltinModule::Readline);
        }
        Some(BuiltinModule::StreamConsumers) => {
            return cached_builtin(context, BuiltinModule::StreamConsumers);
        }
        Some(BuiltinModule::Querystring) => {
            return cached_builtin(context, BuiltinModule::Querystring);
        }
        Some(BuiltinModule::Events) => {
            return cached_builtin(context, BuiltinModule::Events);
        }
        Some(BuiltinModule::Console) => {
            return cached_builtin(context, BuiltinModule::Console);
        }
        Some(BuiltinModule::Tty) => {
            return cached_builtin(context, BuiltinModule::Tty);
        }
        Some(BuiltinModule::Crypto) => {
            return cached_builtin(context, BuiltinModule::Crypto);
        }
        Some(BuiltinModule::Domain) => {
            return cached_builtin(context, BuiltinModule::Domain);
        }
        Some(BuiltinModule::Tls) => {
            return cached_builtin(context, BuiltinModule::Tls);
        }
        Some(BuiltinModule::V8) => {
            return cached_builtin(context, BuiltinModule::V8);
        }
        Some(BuiltinModule::Module) => {
            return cached_builtin(context, BuiltinModule::Module);
        }
        Some(BuiltinModule::AsyncHooks) => {
            return if specifier.strip_prefix("node:").unwrap_or(&specifier)
                == "internal/async_hooks"
            {
                crate::modules::domain_trampoline::internal_hooks(context)
            } else {
                crate::modules::async_hooks_shared_vm::module(context)
            };
        }
        Some(BuiltinModule::DiagnosticsChannel) => {
            return crate::modules::diagnostics_channel_shared_vm::module(context);
        }
        Some(BuiltinModule::Dns) => {
            return cached_builtin(context, BuiltinModule::Dns);
        }
        Some(BuiltinModule::PerfHooks) => {
            return cached_builtin(context, BuiltinModule::PerfHooks);
        }
        Some(BuiltinModule::Bench) => {
            return cached_builtin(context, BuiltinModule::Bench);
        }
        Some(BuiltinModule::BenchReporters) => {
            return cached_builtin(context, BuiltinModule::BenchReporters);
        }
        Some(BuiltinModule::Zlib) => {
            return cached_builtin(context, BuiltinModule::Zlib);
        }
        Some(BuiltinModule::WebStreams) => {
            return cached_builtin(context, BuiltinModule::WebStreams);
        }
        Some(
            module @ (BuiltinModule::Fs
            | BuiltinModule::FsPromises
            | BuiltinModule::Vfs
            | BuiltinModule::Net
            | BuiltinModule::Http
            | BuiltinModule::Os
            | BuiltinModule::Buffer
            | BuiltinModule::Stream
            | BuiltinModule::StreamPromises
            | BuiltinModule::StreamIter
            | BuiltinModule::ZlibIter
            | BuiltinModule::Timers
            | BuiltinModule::InternalTimers
            | BuiltinModule::TimersPromises
            | BuiltinModule::NodeTest
            | BuiltinModule::StringDecoder
            | BuiltinModule::WorkerThreads
            | BuiltinModule::Util
            | BuiltinModule::Sys
            | BuiltinModule::ChildProcess
            | BuiltinModule::InternalChildProcess
            | BuiltinModule::Https
            | BuiltinModule::Http2
            | BuiltinModule::DnsPromises
            | BuiltinModule::CaresBinding
            | BuiltinModule::Http2Binding
            | BuiltinModule::Inspector
            | BuiltinModule::Repl
            | BuiltinModule::Sea
            | BuiltinModule::Cluster
            | BuiltinModule::Wasi
            | BuiltinModule::TraceEvents),
        ) => {
            return cached_builtin(context, module);
        }
        Some(BuiltinModule::Internal("internal/histogram")) => {
            // Node's internal/histogram class is the implementation exported by
            // perf_hooks here. Keep both require paths on the same constructor
            // so instanceof checks agree across the public and internal APIs.
            return cached_builtin(context, BuiltinModule::PerfHooks);
        }
        Some(module @ BuiltinModule::Internal(_)) => {
            return cached_builtin(context, module);
        }
        None => {}
    }
    let filename = resolve_filename(context, &specifier, parent)?;
    let root = context.host_mut().node_library_root.clone();
    let goal = if filename.starts_with(root.join("lib")) {
        EntryGoal::CommonJs
    } else {
        EntryGoal::Node
    };
    if is_node_library_source(context, &filename) {
        return load(context, &filename, Some(parent), goal);
    }
    if context
        .rooted_value(directory_cache)
        .is_some_and(|value| value.is_undefined())
    {
        directory_cache = context.null_object_rooted()?;
        let map_set = get(context, api, "mapSet")?;
        context.call_rooted(map_set, paths, &[parent_path, directory_cache])?;
    }
    load_with_resolution(
        context,
        &filename,
        Some(parent),
        goal,
        Some((directory_cache, &specifier)),
    )
}

/// Defines the global `console` (`console_shared_vm::facade`).
pub(crate) fn initialize_console_global(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let console = crate::modules::console_shared_vm::facade(context)?;
    let global = context.global_root()?;
    let key = context.string_rooted("console");
    context.define_data_property_rooted(global, key, console, true, false, true)?;
    Ok(context.undefined())
}

/// A require capability installed only in a configured SDK module wrapper.
/// Public module.filename mutation cannot grant access to the timer adapter.
/// The private global through which a builtin's ESM facade reads its exports.
pub(crate) const BUILTIN_EXPORT_VALUES: &str = "\0quench:builtinExportValues";

/// Capture SDK definitions behind the exports an imported builtin can expose.
/// Reading a policy does not invoke its exported operations or train an app.
#[cfg(feature = "aot")]
pub(crate) fn stage_builtin_exports(context: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted(BOOTSTRAP_REQUIRE);
    let require = context.get_property_rooted(global, key)?;
    let specifier = context.string_rooted(name);
    let receiver = context.undefined();
    let module = context.call_rooted(require, receiver, &[specifier])?;
    for name in super::builtin_export_names(name.strip_prefix("node:").unwrap_or(name)) {
        context.scoped(|context| {
            let key = context.string_rooted(name);
            let own = context.own_property_descriptor_rooted(module, key)?;
            if !context.rooted_value(own).is_some_and(|value| value.is_undefined()) {
                context.get_property_rooted(module, key)?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// The values of a builtin module's named exports (`args`: the module and the export names),
/// aligned with the names. As lib/internal/bootstrap/realm.js `syncExports` does, each is read
/// with `getOwn` (absent own property: `undefined`), in the order the module lists its own
/// enumerable keys, then the names it does not list.
pub(crate) fn builtin_export_values(
    context: &mut Context<'_>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let module = args.first().copied().unwrap_or(undefined);
    let names = args.get(1).copied().unwrap_or(undefined);
    let listener = args.get(2).copied().unwrap_or(undefined);
    let count = get(context, names, "length")?;
    let count = context.to_number(count)? as usize;
    let mut wanted = Vec::with_capacity(count);
    for index in 0..count {
        let name = get(context, names, &index.to_string())?;
        wanted.push(context.string_text(name)?.unwrap_or_default());
    }
    let mut values = vec![None; count];
    // A pending lazy member stays pending: the facade binds its pending value, which
    // resolves on first read, and `listener` receives the member's value when it materializes.
    for key in context.enumerable_own_keys_rooted(module, false, false)? {
        let Some(text) = context.string_text(key)? else { continue };
        if let Some(index) = wanted.iter().position(|name| *name == text) {
            values[index] = Some(match context.pending_lazy_data_property_rooted(module, key, listener, index as u32)? {
                Some(pending) => pending,
                None => context.get_property_rooted(module, key)?,
            });
        }
    }
    for (index, name) in wanted.iter().enumerate() {
        if values[index].is_some() { continue; }
        let key = context.string_rooted(name);
        if let Some(pending) = context.pending_lazy_data_property_rooted(module, key, listener, index as u32)? {
            values[index] = Some(pending);
            continue;
        }
        let descriptor = context.own_property_descriptor_rooted(module, key)?;
        let own = !context.rooted_value(descriptor).is_some_and(|value| value.is_undefined());
        values[index] = Some(if own { context.get_property_rooted(module, key)? } else { undefined });
    }
    let values = values.into_iter().map(|value| value.unwrap_or(undefined)).collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(super) fn require_node_library(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if let Some(value) = args.first() {
        match context.string_text(*value)?.as_deref() {
            Some(name) if name.strip_prefix("node:").unwrap_or(name) == "internal/timers" => {
                return cached_builtin(context, BuiltinModule::InternalTimers);
            }
            Some(crate::modules::buffer_shared_vm::FAST_BUFFER_REQUEST) => {
                return crate::modules::buffer_shared_vm::fast_buffer_request(context);
            }
            Some(crate::modules::buffer_shared_vm::BUFFER_POOL_REQUEST) => {
                return crate::modules::buffer_shared_vm::buffer_pool_request(context);
            }
            Some(crate::modules::primordials_shared_vm::COPY_REQUEST) => {
                return crate::modules::primordials_shared_vm::copy_request(context);
            }
            _ => {}
        }
    }
    require(context, receiver, args)
}

/// `require.resolve(request, options)`: `Module._resolveFilename(request, module, false, options)`.
pub(super) fn resolve(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Resolve)?;
    let parent = context.host_function_data()?;
    let lookup = lookup_paths(context, args.get(1).copied())?;
    resolve_request(context, &specifier, parent, lookup.as_deref())
}

/// `require.resolve.paths(request)`: `Module._resolveLookupPaths(request, module)`. A builtin
/// has none; a relative request resolves from its parent's directory; any other searches the
/// parent's `node_modules` chain, then the global paths.
pub(super) fn resolve_paths(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Resolve)?;
    if requested_builtin(context, &specifier).is_some() {
        return Ok(context.null());
    }
    let parent = context.host_function_data()?;
    let filename = get(context, parent, "filename")?;
    let filename = context.string_text(filename)?;
    let paths: Vec<String> = if is_relative_request(&specifier) {
        let directory = filename
            .as_deref()
            .and_then(|filename| Path::new(filename).parent())
            .map_or_else(|| ".".to_owned(), |directory| directory.to_string_lossy().into_owned());
        vec![directory]
    } else {
        let parent_paths = get(context, parent, "paths")?;
        let mut paths = Vec::new();
        if context.is_array_rooted(parent_paths)? {
            let length = get(context, parent_paths, "length")?;
            for index in 0..context.to_number(length)? as usize {
                let path = get(context, parent_paths, &index.to_string())?;
                paths.push(context.to_string(path)?);
            }
        }
        paths.extend(
            crate::modules::module_shared_vm::global_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned()),
        );
        paths
    };
    let paths = paths.iter().map(|path| context.string_rooted(path)).collect::<Vec<_>>();
    context.array_rooted(&paths)
}

/// `_resolveLookupPaths`' relative request: a `.` followed by nothing, `.` or `/`.
fn is_relative_request(specifier: &str) -> bool {
    let mut characters = specifier.chars();
    characters.next() == Some('.') && matches!(characters.next(), None | Some('.' | '/'))
}

/// `options.paths` of `Module._resolveFilename`, each element passed through `path.resolve`.
/// Options that are not an object, or have no `paths`, resolve from the parent.
fn lookup_paths(context: &mut Context<'_>, options: Option<RootId>) -> Result<Option<Vec<PathBuf>>, RootedError> {
    let Some(options) = options else { return Ok(None) };
    if !context.is_object_rooted(options)? || context.is_callable_rooted(options)? {
        return Ok(None);
    }
    let paths = get(context, options, "paths")?;
    if context.rooted_value(paths).is_some_and(|value| value.is_undefined()) {
        return Ok(None);
    }
    if !context.is_array_rooted(paths)? {
        let name = context.string_rooted("options.paths");
        return Err(throw_node_error(context, "ERR_INVALID_ARG_VALUE", &[name, paths])?);
    }
    let path = crate::modules::path_shared_vm::module(context)?;
    let resolve = get(context, path, "resolve")?;
    let length = get(context, paths, "length")?;
    let mut directories = Vec::new();
    for index in 0..context.to_number(length)? as usize {
        let entry = get(context, paths, &index.to_string())?;
        let resolved = context.call_rooted(resolve, path, &[entry])?;
        directories.push(PathBuf::from(context.to_string(resolved)?));
    }
    Ok(Some(directories))
}

/// Throws Node's `code` error built by lib/internal/errors.js from `args`.
fn throw_node_error(context: &mut Context<'_>, code: &str, args: &[RootId]) -> Result<RootedError, RootedError> {
    let codes = node_library_export(context, "internal/modules/cjs/loader.js", "internal/errors.js", "codes")?;
    let constructor = get(context, codes, code)?;
    let error = context.construct_rooted(constructor, constructor, args)?;
    Ok(context.throw(error))
}

pub(crate) fn resolve_from_parent(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Resolve)?;
    if requested_builtin(context, &specifier).is_some() {
        return Ok(context.string_rooted(&specifier));
    }
    let parent = match args.get(1).copied() {
        Some(parent) if context.truthy_rooted(parent)? => parent,
        _ => {
            let filename = inline_directory(context)?.join("noop.js");
            module_record(context, &filename, None, false)?
        }
    };
    let lookup = lookup_paths(context, args.get(3).copied())?;
    resolve_request(context, &specifier, parent, lookup.as_deref())
}

fn resolve_request(
    context: &mut Context<'_>,
    specifier: &str,
    parent: RootId,
    lookup: Option<&[PathBuf]>,
) -> Result<RootId, RootedError> {
    if requested_builtin(context, specifier).is_some() {
        return Ok(context.string_rooted(specifier));
    }
    let filename = resolve_filename_in(context, specifier, parent, lookup)?;
    Ok(context.string_rooted(&filename.to_string_lossy()))
}

#[derive(Clone, Copy)]
enum BuiltinModule {
    Internal(&'static str),
    Process,
    Assert,
    AssertStrict,
    Path,
    PathPosix,
    PathWin32,
    Fs,
    FsPromises,
    Vfs,
    Net,
    Http,
    Os,
    Buffer,
    Bench,
    BenchReporters,
    Stream,
    StreamPromises,
    StreamIter,
    ZlibIter,
    WebStreams,
    StringDecoder,
    WorkerThreads,
    Util,
    /// `sys`, the deprecated alias of `util`.
    Sys,
    UtilTypes,
    Timers,
    TimersPromises,
    NodeTest,
    ChildProcess,
    InternalChildProcess,
    Url,
    Querystring,
    Readline,
    StreamConsumers,
    Events,
    Console,
    Tty,
    Crypto,
    Domain,
    Tls,
    V8,
    Module,
    AsyncHooks,
    DiagnosticsChannel,
    Dns,
    Dgram,
    InternalDgram,
    InternalUrl,
    InternalVfsFd,
    InternalUndici,
    InternalTestBinding,
    InternalTimers,
    InternalBlockList,
    InternalSocketAddress,
    Https,
    Http2,
    Http2Binding,
    CaresBinding,
    DnsPromises,
    Vm,
    Inspector,
    Repl,
    Sea,
    Cluster,
    Wasi,
    TraceEvents,
    PerfHooks,
    Zlib,
}

impl BuiltinModule {
    fn from_specifier(specifier: &str) -> Option<Self> {
        BUILTIN_SPECIFIERS
            .iter()
            .find_map(|(name, module)| (*name == specifier).then_some(*module))
    }

    fn cache_key(self) -> Option<&'static str> {
        match self {
            Self::Internal(name) => Some(name),
            Self::Fs => Some("fs"),
            Self::FsPromises => Some("fs/promises"),
            Self::Vfs => Some("vfs"),
            Self::Net => Some("net"),
            Self::Http => Some("http"),
            Self::Os => Some("os"),
            Self::Buffer => Some("buffer"),
            Self::Bench => Some("node:bench"),
            Self::BenchReporters => Some("node:bench/reporters"),
            Self::Stream => Some("stream"),
            Self::StreamPromises => Some("stream/promises"),
            Self::StreamIter => Some("stream/iter"),
            Self::ZlibIter => Some("zlib/iter"),
            Self::WebStreams => Some("stream/web"),
            Self::StringDecoder => Some("string_decoder"),
            Self::WorkerThreads => Some("worker_threads"),
            Self::Util => Some("util"),
            Self::Sys => Some("sys"),
            Self::UtilTypes => Some("util/types"),
            Self::Timers => Some("timers"),
            Self::InternalTimers => Some("internal/timers"),
            Self::TimersPromises => Some("timers/promises"),
            Self::NodeTest => Some("node:test"),
            Self::ChildProcess => Some("child_process"),
            Self::InternalChildProcess => Some("internal/child_process"),
            Self::Url => Some("url"),
            Self::Querystring => Some("querystring"),
            Self::Readline => Some("readline"),
            Self::StreamConsumers => Some("stream/consumers"),
            Self::Events => Some("events"),
            Self::Console => Some("console"),
            Self::Tty => Some("tty"),
            Self::Crypto => Some("crypto"),
            Self::Domain => Some("domain"),
            Self::Tls => Some("tls"),
            Self::V8 => Some("v8"),
            Self::Module => Some("module"),
            Self::Dns => Some("dns"),
            Self::Dgram => Some("dgram"),
            Self::Https => Some("https"),
            Self::Http2 => Some("http2"),
            Self::DnsPromises => Some("dns/promises"),
            Self::CaresBinding => Some("internalBinding/cares_wrap"),
            Self::Http2Binding => Some("internalBinding/http2"),
            Self::Vm => Some("vm"),
            Self::Inspector => Some("inspector"),
            Self::Repl => Some("repl"),
            Self::Sea => Some("sea"),
            Self::Cluster => Some("cluster"),
            Self::Wasi => Some("wasi"),
            Self::TraceEvents => Some("trace_events"),
            Self::PerfHooks => Some("perf_hooks"),
            Self::Zlib => Some("zlib"),
            Self::AsyncHooks | Self::DiagnosticsChannel => None,
            Self::Process
            | Self::Assert
            | Self::AssertStrict
            | Self::Path
            | Self::PathPosix
            | Self::PathWin32 => None,
            Self::InternalUrl => Some("internal/url"),
            Self::InternalVfsFd => Some("internal/vfs/fd"),
            Self::InternalUndici => None,
            Self::InternalDgram
            | Self::InternalTestBinding
            | Self::InternalBlockList
            | Self::InternalSocketAddress => None,
        }
    }
}

/// Whether scripts can require Node's internal modules (`--expose-internals`).
fn internals_exposed(context: &mut Context<'_>) -> bool {
    context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--expose-internals")
}

fn requested_builtin(context: &mut Context<'_>, specifier: &str) -> Option<BuiltinModule> {
    let builtin = BuiltinModule::from_specifier(specifier)?;
    if matches!(builtin, BuiltinModule::Bench | BuiltinModule::BenchReporters)
        && !context
            .host_mut()
            .shared_state()
            .borrow()
            .cli_options
            .enabled("--experimental-bench")
    {
        return None;
    }
    if matches!(builtin, BuiltinModule::StreamIter | BuiltinModule::ZlibIter)
        && !context
            .host_mut()
            .shared_state()
            .borrow()
            .cli_options
            .enabled("--experimental-stream-iter")
    {
        return None;
    }
    if matches!(builtin, BuiltinModule::InternalTimers) && !internals_exposed(context) {
        None
    } else {
        Some(builtin)
    }
}

pub(crate) fn is_builtin_specifier(specifier: &str) -> bool {
    BuiltinModule::from_specifier(specifier).is_some()
}

const BUILTIN_SPECIFIERS: &[(&str, BuiltinModule)] = &[
    (
        "internal/source_map/source_map_cache",
        BuiltinModule::Internal("internal/source_map/source_map_cache"),
    ),
    (
        "internal/histogram",
        BuiltinModule::Internal("internal/histogram"),
    ),
    ("internal/zip", BuiltinModule::Internal("internal/zip")),
    (
        "internal/zip/archive",
        BuiltinModule::Internal("internal/zip/archive"),
    ),
    (
        "internal/zip/binary",
        BuiltinModule::Internal("internal/zip/binary"),
    ),
    (
        "internal/zip/buffer",
        BuiltinModule::Internal("internal/zip/buffer"),
    ),
    (
        "internal/zip/compression",
        BuiltinModule::Internal("internal/zip/compression"),
    ),
    (
        "internal/zip/constants",
        BuiltinModule::Internal("internal/zip/constants"),
    ),
    (
        "internal/zip/content-size",
        BuiltinModule::Internal("internal/zip/content-size"),
    ),
    (
        "internal/zip/dos",
        BuiltinModule::Internal("internal/zip/dos"),
    ),
    (
        "internal/zip/entry",
        BuiltinModule::Internal("internal/zip/entry"),
    ),
    (
        "internal/zip/extra-fields",
        BuiltinModule::Internal("internal/zip/extra-fields"),
    ),
    (
        "internal/zip/file",
        BuiltinModule::Internal("internal/zip/file"),
    ),
    (
        "internal/zip/fs-util",
        BuiltinModule::Internal("internal/zip/fs-util"),
    ),
    (
        "internal/zip/header-builders",
        BuiltinModule::Internal("internal/zip/header-builders"),
    ),
    (
        "internal/zip/headers",
        BuiltinModule::Internal("internal/zip/headers"),
    ),
    (
        "internal/process/task_queues",
        BuiltinModule::Internal("internal/process/task_queues"),
    ),
    (
        "internal/crypto/keys",
        BuiltinModule::Internal("internal/crypto/keys"),
    ),
    (
        "internal/abort_controller",
        BuiltinModule::Internal("internal/abort_controller"),
    ),
    (
        "internal/worker/io",
        BuiltinModule::Internal("internal/worker/io"),
    ),
    ("internal/worker", BuiltinModule::Internal("internal/worker")),
    ("constants", BuiltinModule::Internal("constants")),
    ("node:constants", BuiltinModule::Internal("constants")),
    ("dns/promises", BuiltinModule::DnsPromises),
    ("node:dns/promises", BuiltinModule::DnsPromises),
    (
        "inspector/promises",
        BuiltinModule::Internal("inspector/promises"),
    ),
    (
        "node:inspector/promises",
        BuiltinModule::Internal("inspector/promises"),
    ),
    ("punycode", BuiltinModule::Internal("punycode")),
    ("node:punycode", BuiltinModule::Internal("punycode")),
    (
        "readline/promises",
        BuiltinModule::Internal("readline/promises"),
    ),
    (
        "node:readline/promises",
        BuiltinModule::Internal("readline/promises"),
    ),
    (
        "node:test/reporters",
        BuiltinModule::Internal("node:test/reporters"),
    ),
    ("_http_agent", BuiltinModule::Internal("_http_agent")),
    ("node:_http_agent", BuiltinModule::Internal("_http_agent")),
    ("_http_client", BuiltinModule::Internal("_http_client")),
    ("node:_http_client", BuiltinModule::Internal("_http_client")),
    ("_http_common", BuiltinModule::Internal("_http_common")),
    ("node:_http_common", BuiltinModule::Internal("_http_common")),
    ("_http_incoming", BuiltinModule::Internal("_http_incoming")),
    (
        "node:_http_incoming",
        BuiltinModule::Internal("_http_incoming"),
    ),
    ("_http_outgoing", BuiltinModule::Internal("_http_outgoing")),
    (
        "node:_http_outgoing",
        BuiltinModule::Internal("_http_outgoing"),
    ),
    ("_http_server", BuiltinModule::Internal("_http_server")),
    ("node:_http_server", BuiltinModule::Internal("_http_server")),
    ("_tls_common", BuiltinModule::Internal("_tls_common")),
    ("node:_tls_common", BuiltinModule::Internal("_tls_common")),
    ("_tls_wrap", BuiltinModule::Internal("_tls_wrap")),
    ("node:_tls_wrap", BuiltinModule::Internal("_tls_wrap")),
    ("process", BuiltinModule::Process),
    ("node:process", BuiltinModule::Process),
    ("assert", BuiltinModule::Assert),
    ("node:assert", BuiltinModule::Assert),
    ("assert/strict", BuiltinModule::AssertStrict),
    ("node:assert/strict", BuiltinModule::AssertStrict),
    ("path", BuiltinModule::Path),
    ("node:path", BuiltinModule::Path),
    ("path/posix", BuiltinModule::PathPosix),
    ("node:path/posix", BuiltinModule::PathPosix),
    ("path/win32", BuiltinModule::PathWin32),
    ("node:path/win32", BuiltinModule::PathWin32),
    ("fs", BuiltinModule::Fs),
    ("node:fs", BuiltinModule::Fs),
    ("fs/promises", BuiltinModule::FsPromises),
    ("node:fs/promises", BuiltinModule::FsPromises),
    ("vfs", BuiltinModule::Vfs),
    ("node:vfs", BuiltinModule::Vfs),
    ("net", BuiltinModule::Net),
    ("node:net", BuiltinModule::Net),
    ("http", BuiltinModule::Http),
    ("node:http", BuiltinModule::Http),
    ("os", BuiltinModule::Os),
    ("node:os", BuiltinModule::Os),
    ("buffer", BuiltinModule::Buffer),
    ("node:buffer", BuiltinModule::Buffer),
    ("node:bench", BuiltinModule::Bench),
    ("node:bench/reporters", BuiltinModule::BenchReporters),
    ("stream", BuiltinModule::Stream),
    ("node:stream", BuiltinModule::Stream),
    ("stream/iter", BuiltinModule::StreamIter),
    ("node:stream/iter", BuiltinModule::StreamIter),
    ("zlib/iter", BuiltinModule::ZlibIter),
    ("node:zlib/iter", BuiltinModule::ZlibIter),
    ("stream/promises", BuiltinModule::StreamPromises),
    ("node:stream/promises", BuiltinModule::StreamPromises),
    ("stream/web", BuiltinModule::WebStreams),
    ("node:stream/web", BuiltinModule::WebStreams),
    ("stream/consumers", BuiltinModule::StreamConsumers),
    ("node:stream/consumers", BuiltinModule::StreamConsumers),
    ("timers", BuiltinModule::Timers),
    ("internal/timers", BuiltinModule::InternalTimers),
    ("node:timers", BuiltinModule::Timers),
    ("timers/promises", BuiltinModule::TimersPromises),
    ("node:timers/promises", BuiltinModule::TimersPromises),
    ("node:test", BuiltinModule::NodeTest),
    ("string_decoder", BuiltinModule::StringDecoder),
    ("node:string_decoder", BuiltinModule::StringDecoder),
    ("worker_threads", BuiltinModule::WorkerThreads),
    ("node:worker_threads", BuiltinModule::WorkerThreads),
    ("util", BuiltinModule::Util),
    // Node keeps the deprecated `sys` specifier as an alias of `util`.
    ("sys", BuiltinModule::Sys),
    ("node:sys", BuiltinModule::Sys),
    ("node:util", BuiltinModule::Util),
    ("util/types", BuiltinModule::UtilTypes),
    ("node:util/types", BuiltinModule::UtilTypes),
    ("child_process", BuiltinModule::ChildProcess),
    ("node:child_process", BuiltinModule::ChildProcess),
    ("url", BuiltinModule::Url),
    ("node:url", BuiltinModule::Url),
    ("querystring", BuiltinModule::Querystring),
    ("node:querystring", BuiltinModule::Querystring),
    ("readline", BuiltinModule::Readline),
    ("node:readline", BuiltinModule::Readline),
    ("events", BuiltinModule::Events),
    ("node:events", BuiltinModule::Events),
    ("console", BuiltinModule::Console),
    ("node:console", BuiltinModule::Console),
    ("tty", BuiltinModule::Tty),
    ("node:tty", BuiltinModule::Tty),
    ("crypto", BuiltinModule::Crypto),
    ("node:crypto", BuiltinModule::Crypto),
    ("domain", BuiltinModule::Domain),
    ("node:domain", BuiltinModule::Domain),
    ("tls", BuiltinModule::Tls),
    ("node:tls", BuiltinModule::Tls),
    ("v8", BuiltinModule::V8),
    ("node:v8", BuiltinModule::V8),
    ("module", BuiltinModule::Module),
    ("node:module", BuiltinModule::Module),
    ("async_hooks", BuiltinModule::AsyncHooks),
    ("node:async_hooks", BuiltinModule::AsyncHooks),
    ("internal/async_hooks", BuiltinModule::AsyncHooks),
    (
        "internal/child_process",
        BuiltinModule::InternalChildProcess,
    ),
    ("diagnostics_channel", BuiltinModule::DiagnosticsChannel),
    (
        "node:diagnostics_channel",
        BuiltinModule::DiagnosticsChannel,
    ),
    ("dns", BuiltinModule::Dns),
    ("node:dns", BuiltinModule::Dns),
    ("dgram", BuiltinModule::Dgram),
    ("node:dgram", BuiltinModule::Dgram),
    ("internal/dgram", BuiltinModule::InternalDgram),
    ("internal/url", BuiltinModule::InternalUrl),
    ("internal/vfs/fd", BuiltinModule::InternalVfsFd),
    ("internal/deps/undici/undici", BuiltinModule::InternalUndici),
    ("internal/test/binding", BuiltinModule::InternalTestBinding),
    ("internal/blocklist", BuiltinModule::InternalBlockList),
    (
        "internal/socketaddress",
        BuiltinModule::InternalSocketAddress,
    ),
    ("https", BuiltinModule::Https),
    ("node:https", BuiltinModule::Https),
    ("http2", BuiltinModule::Http2),
    ("node:http2", BuiltinModule::Http2),
    ("vm", BuiltinModule::Vm),
    ("node:vm", BuiltinModule::Vm),
    ("inspector", BuiltinModule::Inspector),
    ("node:inspector", BuiltinModule::Inspector),
    ("repl", BuiltinModule::Repl),
    ("node:repl", BuiltinModule::Repl),
    ("node:sea", BuiltinModule::Sea),
    ("cluster", BuiltinModule::Cluster),
    ("node:cluster", BuiltinModule::Cluster),
    ("wasi", BuiltinModule::Wasi),
    ("node:wasi", BuiltinModule::Wasi),
    ("trace_events", BuiltinModule::TraceEvents),
    ("node:trace_events", BuiltinModule::TraceEvents),
    ("perf_hooks", BuiltinModule::PerfHooks),
    ("node:perf_hooks", BuiltinModule::PerfHooks),
    ("zlib", BuiltinModule::Zlib),
    ("node:zlib", BuiltinModule::Zlib),
];

fn cached_builtin(
    context: &mut Context<'_>,
    builtin: BuiltinModule,
) -> Result<RootId, RootedError> {
    // Source-owned CommonJS modules keep their live exports on the module record.
    // Reentrant requires must observe module.exports reassignment during loading,
    // rather than a second cache retaining an earlier partial export object.
    let canonical = builtin
        .cache_key()
        .ok_or_else(|| RootedError::host("builtin module has no canonical cache key"))?;
    let key = format!("\0builtin:{canonical}");
    let roots = context.host_mut().shared_state();
    if let Some(module) = roots.borrow().builtin_cache.get(&key).copied() {
        return Ok(module);
    }

    let module = host_load(|| {
        HOST_LOADS.with(|loads| loads.compiled.set(true));
        context.with_startup_intrinsics(|context| build_builtin(context, builtin))
    })?;
    let retained = context.retain(module)?;
    let previous = roots.borrow_mut().builtin_cache.insert(key.clone(), retained);
    if let Some(previous) = previous { context.release_root(previous); }
    Ok(module)
}

/// One lifetime owner for native binding objects shared by configured libraries.
pub(crate) fn cached_native_binding(
    context: &mut Context<'_>,
    key: &'static str,
    build: impl FnOnce(&mut Context<'_>) -> Result<RootId, RootedError>,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(key).copied() {
        return Ok(value);
    }
    let value = build(context)?;
    let retained = context.retain(value)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(key.to_owned(), retained);
    Ok(value)
}

/// Load a real configured standard-library export over this VM's native bindings.
/// A library module's `require(id)`: a host builtin when the id names one, otherwise the
/// pinned library file.
pub(crate) fn library_require(context: &mut Context<'_>, id: &str) -> Result<RootId, RootedError> {
    if let Some(builtin) = BuiltinModule::from_specifier(id) {
        return cached_builtin(context, builtin);
    }
    let relative = if id.ends_with(".js") { id.to_owned() } else { format!("{id}.js") };
    node_library_module(context, "internal/util.js", &relative)
}

/// A library function a host fast path stands for, as its definition made it: the value of
/// `method` on the `prototype` of the constructor its `Definition` names, recorded before
/// any other code can replace it (`record_library_originals`,
/// `record_bootstrap_originals`). A fast path that skips a call of the method holds only
/// while the method is still this value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryOriginal {
    /// `Readable.prototype.push` (`lib/internal/streams/readable.js`).
    ReadablePush,
    /// `EventEmitter.prototype.listenerCount` (the bootstrap's shared EventEmitter).
    EmitterListenerCount,
    /// `IncomingMessage.prototype._addHeaderLines` (`lib/_http_incoming.js`).
    IncomingAddHeaderLines,
}

/// Where a `LibraryOriginal`'s constructor comes from.
#[derive(Clone, Copy)]
enum Definition {
    /// An export of a library module, as the module finishes evaluating: the named one, or
    /// the module's exports themselves.
    Module(&'static str, Option<&'static str>),
    /// A global the bootstrap installs, as the bootstrap finishes.
    BootstrapGlobal(&'static str),
}

impl LibraryOriginal {
    pub(crate) const ALL: [Self; 3] = [Self::ReadablePush, Self::EmitterListenerCount, Self::IncomingAddHeaderLines];

    /// The constructor's definition and the prototype method the original is.
    fn definition(self) -> (Definition, &'static str) {
        match self {
            Self::ReadablePush => (Definition::Module("lib/internal/streams/readable.js", None), "push"),
            Self::IncomingAddHeaderLines => {
                (Definition::Module("lib/_http_incoming.js", Some("IncomingMessage")), "_addHeaderLines")
            }
            Self::EmitterListenerCount => (Definition::BootstrapGlobal("__nodeEventEmitter"), "listenerCount"),
        }
    }
}

/// The recorded `LibraryOriginal`s, by their position in `LibraryOriginal::ALL`.
#[derive(Default)]
pub(crate) struct LibraryOriginals(pub(crate) [Option<RootId>; LibraryOriginal::ALL.len()]);

impl LibraryOriginals {
    pub(crate) fn get(&self, original: LibraryOriginal) -> Option<RootId> {
        self.0[original as usize]
    }
}

/// Records `original`, whose constructor is `constructor`, unless it is recorded.
fn record_library_original(context: &mut Context<'_>, original: LibraryOriginal, constructor: RootId) -> Result<(), RootedError> {
    if context.host_mut().shared_state().borrow().library_originals.get(original).is_some() {
        return Ok(());
    }
    let (_, method) = original.definition();
    let prototype = get(context, constructor, "prototype")?;
    let value = get(context, prototype, method)?;
    let retained = context.retain(value)?;
    context.host_mut().shared_state().borrow_mut().library_originals.0[original as usize] = Some(retained);
    Ok(())
}

/// Records the `LibraryOriginal`s the library module at `path` defines from its `exports`.
fn record_library_originals(context: &mut Context<'_>, path: &Path, exports: RootId) -> Result<(), RootedError> {
    for original in LibraryOriginal::ALL {
        let Definition::Module(module, export) = original.definition().0 else {
            continue;
        };
        if path == Path::new(module) {
            let constructor = match export {
                Some(name) => get(context, exports, name)?,
                None => exports,
            };
            record_library_original(context, original, constructor)?;
        }
    }
    Ok(())
}

/// Records the `LibraryOriginal`s of the bootstrap's globals, before any script runs.
pub(crate) fn record_bootstrap_originals(context: &mut Context<'_>) -> Result<(), RootedError> {
    for original in LibraryOriginal::ALL {
        if let Definition::BootstrapGlobal(name) = original.definition().0 {
            let global = context.global_root()?;
            let constructor = get(context, global, name)?;
            record_library_original(context, original, constructor)?;
        }
    }
    Ok(())
}

pub(crate) fn node_library_export(
    context: &mut Context<'_>,
    owner: &str,
    relative: &str,
    name: &str,
) -> Result<RootId, RootedError> {
    let library = node_library_module(context, owner, relative)?;
    get(context, library, name)
}

/// A module of the pinned Node library, loaded on first use inside the startup intrinsics:
/// its initialization captures the original intrinsic methods however a script has replaced
/// or frozen them since (`with_startup_intrinsics` restores even nonconfigurable ones).
pub(crate) fn node_library_module(
    context: &mut Context<'_>,
    _owner: &str,
    relative: &str,
) -> Result<RootId, RootedError> {
    if relative == "internal/histogram.js" {
        return cached_builtin(context, BuiltinModule::PerfHooks);
    }
    if let Some(module) = crate::modules::stream_shared_vm::library_module(context, relative)? {
        return Ok(module);
    }
    if relative == "internal/net.js" {
        return crate::modules::net_internal_shared_vm::module(context);
    }
    crate::host::removed_javascript(format!("node:{}", relative.trim_end_matches(".js")))
}

/// Runs a load of host code: a library module or a builtin the host builds. A load on first
/// use compiles after the entry's compilation released the pages compiling touches; they are
/// released again once the outermost load ends, if any load inside it compiled, at most once
/// per `LOAD_RELEASE_INTERVAL` (`release_read_only_pages_after_load`).
fn host_load<T>(load: impl FnOnce() -> T) -> T {
    HOST_LOADS.with(|loads| loads.depth.set(loads.depth.get() + 1));
    let result = load();
    let compiled = HOST_LOADS.with(|loads| {
        loads.depth.set(loads.depth.get() - 1);
        loads.depth.get() == 0 && loads.compiled.replace(false)
    });
    if compiled {
        crate::host::resident_pages::release_read_only_pages_after_load();
    }
    result
}

/// Host loads in progress, and whether one compiled since the last release.
struct HostLoads {
    depth: std::cell::Cell<u32>,
    compiled: std::cell::Cell<bool>,
}

thread_local! {
    static HOST_LOADS: HostLoads = const {
        HostLoads { depth: std::cell::Cell::new(0), compiled: std::cell::Cell::new(false) }
    };
}

pub(crate) fn module_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Module)
}

pub(crate) fn stream_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Stream)
}

pub(crate) fn cares_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::CaresBinding)
}

pub(crate) fn http2_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Http2Binding)
}

pub(crate) fn fs_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Fs)
}

pub(crate) fn buffer_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Buffer)
}

pub(crate) fn readline_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Readline)
}

pub(crate) fn util_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Util)
}

pub(crate) fn timers_promises_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::TimersPromises)
}

pub(crate) fn crypto_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Crypto)
}

pub(crate) fn http_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Http)
}

pub(crate) fn https_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Https)
}

pub(crate) fn net_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Net)
}

pub(crate) fn dns_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Dns)
}

pub(crate) fn events_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Events)
}

pub(crate) fn path_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::modules::path_shared_vm::module(context)
}

pub(crate) fn url_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Url)
}

#[cfg(feature = "aot")]
pub(crate) fn internal_url_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::InternalUrl)
}

/// Private namespaces project the same original objects as the public globals.
fn native_global_namespace(
    context: &mut Context<'_>,
    names: &[&str],
) -> Result<RootId, RootedError> {
    context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let module = context.object_rooted()?;
        for name in names {
            let value = match context.startup_data_property_rooted(global, name) {
                Some(original) => original,
                None => get(context, global, name)?,
            };
            let key = context.string_rooted(name);
            context.define_data_property_rooted(module, key, value, true, true, true)?;
        }
        Ok(module)
    })
}

fn task_queues_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let raw_queue_key = context.string_rooted("\0quench:queueMicrotaskRaw");
    let raw_enqueue = context.get_property_rooted(global, raw_queue_key)?;
    let require = get(context, global, BOOTSTRAP_REQUIRE)?;
    let state = context.null_object_rooted()?;
    set(context, state, "enqueue", raw_enqueue)?;
    set(context, state, "require", require)?;
    let queue = context.host_function_named_with_data(super::operation("taskQueueMicrotask"), "queueMicrotask", state)?;
    let module = context.object_rooted()?;
    set(context, module, "queueMicrotask", queue)?;
    Ok(module)
}

/// The web and timer globals Node defines enumerable.
const ENUMERABLE_GLOBALS: &[&str] = &[
    "queueMicrotask", "clearImmediate", "clearInterval", "clearTimeout", "setImmediate",
    "setInterval", "setTimeout", "structuredClone", "fetch", "crypto", "performance",
];

/// Node's global descriptors: `ENUMERABLE_GLOBALS` enumerable, a `navigator` getter, and
/// `globalThis[Symbol.toStringTag]` as `'global'`.
fn install_global_descriptors(context: &mut Context<'_>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let yes = context.boolean(true);
    for name in ENUMERABLE_GLOBALS {
        let key = context.string_rooted(name);
        let descriptor = context.own_property_descriptor_rooted(global, key)?;
        if !context.is_object_rooted(descriptor)? {
            continue;
        }
        let enumerable = get(context, descriptor, "enumerable")?;
        if !context.truthy_rooted(enumerable)? {
            set(context, descriptor, "enumerable", yes)?;
            context.define_property_descriptor_rooted(global, key, descriptor)?;
        }
    }
    let key = context.string_rooted("navigator");
    let existing = context.own_property_descriptor_rooted(global, key)?;
    if !context.is_object_rooted(existing)? {
        let getter = context.host_function_named(super::operation("navigatorGet"), "get navigator")?;
        let descriptor = context.null_object_rooted()?;
        set(context, descriptor, "get", getter)?;
        set(context, descriptor, "enumerable", yes)?;
        set(context, descriptor, "configurable", yes)?;
        context.define_property_descriptor_rooted(global, key, descriptor)?;
    }
    let symbol = get(context, global, "Symbol")?;
    let tag = get(context, symbol, "toStringTag")?;
    let own = context.own_property_descriptor_rooted(global, tag)?;
    if !context.is_object_rooted(own)? {
        let value = context.string_rooted("global");
        context.define_data_property_rooted(global, tag, value, false, false, true)?;
    }
    Ok(())
}

/// `navigator`'s getter: one object whose `userAgent` names the Node major version.
pub(crate) fn navigator_get(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    const CACHE: &str = "\0builtin:navigator";
    let shared = context.host_mut().shared_state();
    if let Some(navigator) = shared.borrow().builtin_cache.get(CACHE).copied() {
        return Ok(navigator);
    }
    let major = crate::modules::process_state::NODE_VERSION.split('.').next().unwrap_or_default();
    let navigator = context.object_rooted()?;
    let agent = context.string_rooted(&format!("Node.js/{major}"));
    set(context, navigator, "userAgent", agent)?;
    let navigator = context.retain(navigator)?;
    shared.borrow_mut().builtin_cache.insert(CACHE.to_owned(), navigator);
    Ok(navigator)
}

/// A replaceable global whose getter loads `module` on first access and reads its `export`;
/// assigning it replaces the value, as Node's lazy bootstrap globals do.
pub(crate) fn install_lazy_global(
    context: &mut Context<'_>,
    name: &str,
    module: &str,
    export: &str,
    enumerable: bool,
) -> Result<(), RootedError> {
    let slot = context.null_object_rooted()?;
    let module_name = context.string_rooted(module);
    set(context, slot, "module", module_name)?;
    let export_name = context.string_rooted(export);
    set(context, slot, "export", export_name)?;
    let getter = context.host_function_named_with_data(super::operation("lazyGlobalGet"), &format!("get {name}"), slot)?;
    let setter = context.host_function_named_with_data(super::operation("lazyGlobalSet"), &format!("set {name}"), slot)?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    set(context, descriptor, "set", setter)?;
    let enumerable = context.boolean(enumerable);
    set(context, descriptor, "enumerable", enumerable)?;
    let yes = context.boolean(true);
    set(context, descriptor, "configurable", yes)?;
    let global = context.global_root()?;
    let key = context.string_rooted(name);
    context.define_property_descriptor_rooted(global, key, descriptor)?;
    Ok(())
}

/// A lazy global's getter.
pub(crate) fn lazy_global_get(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let slot = context.host_function_data()?;
    if context.has_own_property_rooted(slot, "value")? {
        return get(context, slot, "value");
    }
    let global = context.global_root()?;
    let require = get(context, global, BOOTSTRAP_REQUIRE)?;
    let module = get(context, slot, "module")?;
    let undefined = context.undefined();
    let exports = context.call_rooted(require, undefined, &[module])?;
    let export = get(context, slot, "export")?;
    let value = context.get_property_rooted(exports, export)?;
    set(context, slot, "value", value)?;
    Ok(value)
}

/// A lazy global's setter.
pub(crate) fn lazy_global_set(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let slot = context.host_function_data()?;
    let value = match args.first() {
        Some(value) => *value,
        None => context.undefined(),
    };
    set(context, slot, "value", value)?;
    Ok(context.undefined())
}

/// `queueMicrotask(callback)`: the callback runs in a `Microtask` AsyncResource's scope,
/// which is destroyed after it, so async context follows it; async_hooks loads on the first
/// queued callback. Other values reach the host operation, which validates them.
pub(crate) fn queue_microtask(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = context.host_function_data()?;
    let enqueue = get(context, state, "enqueue")?;
    let callback = match args.first() {
        Some(callback) => *callback,
        None => context.undefined(),
    };
    let undefined = context.undefined();
    if !context.is_callable_rooted(callback)? {
        return context.call_rooted(enqueue, undefined, &[callback]);
    }
    let mut resource_class = get(context, state, "AsyncResource")?;
    if context.rooted_value(resource_class).is_some_and(|value| value.is_undefined()) {
        let require = get(context, state, "require")?;
        let name = context.string_rooted("async_hooks");
        let hooks = context.call_rooted(require, undefined, &[name])?;
        resource_class = get(context, hooks, "AsyncResource")?;
        set(context, state, "AsyncResource", resource_class)?;
    }
    let options = context.object_rooted()?;
    let yes = context.boolean(true);
    set(context, options, "requireManualDestroy", yes)?;
    let kind = context.string_rooted("Microtask");
    let resource = context.construct_rooted(resource_class, resource_class, &[kind, options])?;
    set(context, resource, "callback", callback)?;
    let job = context.host_function_with_data(super::operation("taskQueueMicrotaskJob"), resource)?;
    context.call_rooted(enqueue, undefined, &[job])
}

/// A queued microtask: runs its callback in its resource's scope.
pub(crate) fn queue_microtask_job(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = context.host_function_data()?;
    let run = context.host_function_with_data(super::operation("taskQueueMicrotaskRun"), resource)?;
    let scope = get(context, resource, "runInAsyncScope")?;
    context.call_rooted(scope, resource, &[run])
}

/// The scope's body: the callback, then the resource's destroy, whatever the callback did.
pub(crate) fn queue_microtask_run(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = context.host_function_data()?;
    let callback = get(context, resource, "callback")?;
    let result = context.call_rooted(callback, resource, &[]);
    let destroy = get(context, resource, "emitDestroy")?;
    context.call_rooted(destroy, resource, &[])?;
    result
}

fn build_builtin(context: &mut Context<'_>, builtin: BuiltinModule) -> Result<RootId, RootedError> {
    match builtin {
        BuiltinModule::Internal(name @ ("_http_agent" | "_http_client" | "_http_incoming" | "_http_outgoing" | "_http_server")) => {
            crate::modules::http_shared_vm::internal_module(context, name)
        }
        BuiltinModule::Internal("_tls_common") | BuiltinModule::Internal("_tls_wrap") => {
            let process = context
                .host_mut()
                .shared_state()
                .borrow()
                .process_module
                .ok_or_else(|| RootedError::host("shared process module is not initialized"))?;
            crate::modules::tls_shared_vm::deprecated_internal_module(
                context,
                process,
                match builtin {
                    BuiltinModule::Internal(name) => name,
                    _ => unreachable!(),
                },
            )
        }
        BuiltinModule::Internal("internal/crypto/keys") => {
            let module = node_library_module(context, "crypto.js", "internal/crypto/keys.js")?;
            crate::modules::crypto_key_shared_vm::install_provider_accessors(context, module)?;
            Ok(module)
        }
        BuiltinModule::Internal("internal/source_map/source_map_cache") => {
            // Node's error formatter imports these helpers even when source
            // maps are disabled. The full Node implementation depends on
            // internals that Quench does not yet provide, so expose its
            // disabled-state contract without evaluating it during bootstrap.
            crate::host::removed_javascript("internal/source_map/source_map_cache-quench.js")
        }
        BuiltinModule::Internal("internal/zip") => {
            node_library_module(context, "zlib.js", "internal/zip.js")
        }
        BuiltinModule::Internal("internal/zip/archive") => {
            node_library_module(context, "zlib.js", "internal/zip/archive.js")
        }
        BuiltinModule::Internal("internal/zip/binary") => {
            node_library_module(context, "zlib.js", "internal/zip/binary.js")
        }
        BuiltinModule::Internal("internal/zip/buffer") => {
            node_library_module(context, "zlib.js", "internal/zip/buffer.js")
        }
        BuiltinModule::Internal("internal/zip/compression") => {
            node_library_module(context, "zlib.js", "internal/zip/compression.js")
        }
        BuiltinModule::Internal("internal/zip/constants") => {
            node_library_module(context, "zlib.js", "internal/zip/constants.js")
        }
        BuiltinModule::Internal("internal/zip/content-size") => {
            node_library_module(context, "zlib.js", "internal/zip/content-size.js")
        }
        BuiltinModule::Internal("internal/zip/dos") => {
            node_library_module(context, "zlib.js", "internal/zip/dos.js")
        }
        BuiltinModule::Internal("internal/zip/entry") => {
            node_library_module(context, "zlib.js", "internal/zip/entry.js")
        }
        BuiltinModule::Internal("internal/zip/extra-fields") => {
            node_library_module(context, "zlib.js", "internal/zip/extra-fields.js")
        }
        BuiltinModule::Internal("internal/zip/file") => {
            node_library_module(context, "zlib.js", "internal/zip/file.js")
        }
        BuiltinModule::Internal("internal/zip/fs-util") => {
            node_library_module(context, "zlib.js", "internal/zip/fs-util.js")
        }
        BuiltinModule::Internal("internal/zip/header-builders") => {
            node_library_module(context, "zlib.js", "internal/zip/header-builders.js")
        }
        BuiltinModule::Internal("internal/zip/headers") => {
            node_library_module(context, "zlib.js", "internal/zip/headers.js")
        }
        BuiltinModule::InternalVfsFd => {
            let module = node_library_module(context, "vfs.js", "internal/vfs/fd.js")?;
            let global = context.global_root()?;
            let legacy = get(context, global, "__quenchVfsFdHandles")?;
            if context.is_object_rooted(legacy)? {
                for (name, method, operation) in [
                    ("getVirtualFd", "get", "vfsDescriptorGet"),
                    ("closeVirtualFd", "delete", "vfsDescriptorClose"),
                ] {
                    let source = get(context, module, name)?;
                    let legacy_method = get(context, legacy, method)?;
                    let data = context.array_rooted(&[source, legacy, legacy_method])?;
                    let bridge =
                        context.host_constructor_with_data(super::operation(operation), data)?;
                    let key = context.string_rooted("name");
                    let display = context.string_rooted(name);
                    context
                        .define_data_property_rooted(bridge, key, display, false, false, true)?;
                    set(context, module, name, bridge)?;
                }
            }
            Ok(module)
        }
        BuiltinModule::Internal("internal/worker/io") => {
            let module = native_global_namespace(context, &["MessageChannel", "MessagePort"])?;
            // Node exposes this protocol enum to internal consumers. Keep the
            // native MessagePort implementation while supplying the exact
            // message tags used by the Worker bootstrap/control channel.
            let message_types = context.object_rooted()?;
            for (name, value) in [
                ("UP_AND_RUNNING", "upAndRunning"),
                ("COULD_NOT_SERIALIZE_ERROR", "couldNotSerializeError"),
                ("ERROR_MESSAGE", "errorMessage"),
                ("STDIO_PAYLOAD", "stdioPayload"),
                ("STDIO_WANTS_MORE_DATA", "stdioWantsMoreData"),
                ("LOAD_SCRIPT", "loadScript"),
            ] {
                let value = context.string_rooted(value);
                set(context, message_types, name, value)?;
            }
            set(context, module, "messageTypes", message_types)?;
            Ok(module)
        }
        // Node's lib reads its thread facts here (`process.execve`, `crypto.setFips`,
        // `trace_events`, Web Locks); they are the public `worker_threads` module's.
        BuiltinModule::Internal("internal/worker") => {
            let threads = cached_builtin(context, BuiltinModule::WorkerThreads)?;
            let module = context.object_rooted()?;
            for name in ["isMainThread", "isInternalThread", "threadId", "threadName", "resourceLimits"] {
                let value = get(context, threads, name)?;
                set(context, module, name, value)?;
            }
            // Only the main thread owns process-wide state (Node's `ownsProcessState`).
            let main = get(context, threads, "isMainThread")?;
            set(context, module, "ownsProcessState", main)?;
            Ok(module)
        }
        BuiltinModule::Internal("internal/abort_controller") => {
            let module = native_global_namespace(context, &["AbortController", "AbortSignal"])?;
            let data = context
                .host_mut()
                .shared_state()
                .borrow()
                .event_targets_api
                .ok_or_else(|| RootedError::host("EventTarget API missing"))?;
            let operation = get(context, data, "abortSignal")?;
            set(context, module, "abortSignal", operation)?;
            let aborted = crate::modules::util_aborted_source_shared_vm::original_aborted(context)?;
            set(context, module, "aborted", aborted)?;
            Ok(module)
        }
        BuiltinModule::Internal("internal/process/task_queues") => task_queues_module(context),
        BuiltinModule::Internal("_http_common") => {
            crate::modules::http_common_shared_vm::module(context)
        }
        BuiltinModule::Internal("dns/promises") => crate::modules::dns_module_shared_vm::promises(context),
        BuiltinModule::Internal("punycode") => crate::modules::punycode_shared_vm::module(context),
        BuiltinModule::Internal("readline/promises") => crate::modules::readline_shared_vm::promises_module(context),
        // Every other such builtin is Node's own library module of that name.
        BuiltinModule::Internal(name) => {
            let relative = format!("{}.js", name.strip_prefix("node:").unwrap_or(name));
            node_library_module(context, &relative, &relative)
        }
        BuiltinModule::Fs => crate::modules::fs_shared_vm::module(context),
        BuiltinModule::InternalUrl => crate::modules::url_shared_vm::internal_module(context),
        // lib/util/types.js re-exports internal/util/types, the object `util.types` names.
        BuiltinModule::UtilTypes => {
            let util = crate::modules::util_shared_vm::module(context)?;
            get(context, util, "types")
        }
        BuiltinModule::FsPromises => {
            let fs = cached_builtin(context, BuiltinModule::Fs)?;
            get(context, fs, "promises")
        }
        // The pinned SDK owns VFS layer identity, reserved mount roots and lifecycle.
        // Native descriptor and fs handler operations remain registered below.
        BuiltinModule::Vfs => node_library_module(context, "vfs.js", "vfs.js"),
        BuiltinModule::Net => crate::modules::net_shared_vm::module(context),
        BuiltinModule::Http => crate::modules::http_shared_vm::build_module(context),
        BuiltinModule::Os => crate::modules::os_shared_vm::module(context),
        BuiltinModule::Buffer => crate::modules::buffer_shared_vm::module(context),
        BuiltinModule::Stream => crate::modules::stream_shared_vm::module(context),
        BuiltinModule::StreamPromises => crate::modules::stream_shared_vm::promises(context),
        BuiltinModule::StreamIter => node_library_module(context, "stream/iter.js", "stream/iter.js"),
        BuiltinModule::ZlibIter => node_library_module(context, "zlib/iter.js", "zlib/iter.js"),
        BuiltinModule::WebStreams => node_library_module(context, "stream/web.js", "stream/web.js"),
        BuiltinModule::StringDecoder => crate::modules::string_decoder_shared_vm::module(context),
        BuiltinModule::Timers => {
            let promises = cached_builtin(context, BuiltinModule::TimersPromises)?;
            crate::modules::timers_shared_vm::timers_module(context, promises)
        }
        BuiltinModule::TimersPromises => crate::modules::timers_shared_vm::promises_module(context),
        BuiltinModule::InternalTimers => crate::modules::timers_shared_vm::internal_module(context),
        BuiltinModule::NodeTest => crate::modules::test_shared_vm::module(context),
        BuiltinModule::Vm => crate::modules::vm_shared_vm::module(context),
        BuiltinModule::WorkerThreads => crate::modules::worker_shared_vm::module(context),
        BuiltinModule::Url => crate::modules::url_shared_vm::module(context),
        BuiltinModule::Querystring => crate::modules::querystring_shared_vm::module(context),
        BuiltinModule::Readline => crate::modules::readline_shared_vm::module(context),
        BuiltinModule::StreamConsumers => {
            crate::modules::web_stream_consumers_shared_vm::module(context)
        }
        BuiltinModule::Events => crate::modules::events_shared_vm::module(context),
        BuiltinModule::Console => crate::modules::console_shared_vm::module(context),
        BuiltinModule::Tty => crate::modules::tty_shared_vm::module(context),
        BuiltinModule::Dns => crate::modules::dns_module_shared_vm::module(context),
        BuiltinModule::Dgram => crate::modules::dgram_shared_vm::module(context),
        BuiltinModule::PerfHooks => crate::modules::perf_hooks::module(context),
        BuiltinModule::Bench => node_library_module(context, "bench.js", "bench.js"),
        BuiltinModule::BenchReporters => node_library_module(context, "bench/reporters.js", "bench/reporters.js"),
        BuiltinModule::Zlib => crate::modules::zlib_shared_vm::module(context),
        #[cfg(quench_openssl_provider)]
        BuiltinModule::Crypto => crate::modules::crypto_node::module(context),
        #[cfg(not(quench_openssl_provider))]
        BuiltinModule::Crypto => Err(RootedError::host("node:crypto requires OpenSSL 3")),
        BuiltinModule::Domain => crate::modules::domain_shared_vm::module(context),
        BuiltinModule::Tls => crate::modules::tls_shared_vm::module(context),
        BuiltinModule::V8 => crate::modules::v8_shared_vm::module(context),
        BuiltinModule::Module => crate::modules::module_shared_vm::module(context),
        BuiltinModule::AsyncHooks | BuiltinModule::DiagnosticsChannel => Err(RootedError::host(
            "stateful builtin passed to generic shared module builder",
        )),
        BuiltinModule::Util => crate::modules::util_shared_vm::module(context),
        BuiltinModule::Sys => crate::modules::util_module_shared_vm::sys_module(context),
        BuiltinModule::ChildProcess => crate::modules::child_process_shared_vm::module(context),
        BuiltinModule::InternalChildProcess => {
            let public = cached_builtin(context, BuiltinModule::ChildProcess)?;
            let module =
                node_library_module(context, "child_process.js", "internal/child_process.js")?;
            let constructor = get(context, public, "ChildProcess")?;
            set(context, module, "ChildProcess", constructor)?;
            // The public spawnSync facade dispatches through this cached
            // internal module so Node's established internal hook points stay
            // observable (and replaceable) by callers.
            let spawn_sync = get(context, public, "__quenchSpawnSync")?;
            set(context, module, "spawnSync", spawn_sync)?;
            let key = context.string_rooted("__quenchInternalChildProcess");
            context.define_data_property_rooted(public, key, module, true, false, true)?;
            Ok(module)
        }
        BuiltinModule::Https => crate::modules::https_shared_vm::module(context),
        BuiltinModule::TraceEvents => crate::modules::trace_shared_vm::module(context),
        BuiltinModule::DnsPromises => {
            let dns = dns_module(context)?;
            get(context, dns, "promises")
        }
        BuiltinModule::CaresBinding => crate::modules::cares_binding::module(context),
        // HTTP/2 has no Rust implementation; its Node-sourced engine was removed.
        BuiltinModule::Http2Binding | BuiltinModule::Http2 => {
            Err(RootedError::host("node:http2 is not implemented".to_owned()))
        }
        BuiltinModule::Cluster => node_library_module(context, "cluster.js", "cluster.js"),
        BuiltinModule::Repl => {
            // The internal CJS loader normally initializes this list during
            // Node's pre-execution phase. Quench loads the library module on
            // demand, so seed it from the public Module API before REPL
            // utilities read it.
            let cjs_loader =
                node_library_module(context, "repl.js", "internal/modules/cjs/loader.js")?;
            let cjs_module = get(context, cjs_loader, "Module")?;
            let public_module = cached_builtin(context, BuiltinModule::Module)?;
            let builtin_modules = get(context, public_module, "builtinModules")?;
            set(context, cjs_module, "builtinModules", builtin_modules)?;
            node_library_module(context, "repl.js", "repl.js")
        }
        BuiltinModule::Inspector | BuiltinModule::Wasi => context.object_rooted(),
        BuiltinModule::Sea => crate::host::removed_javascript("node:sea.js"),
        BuiltinModule::Process
        | BuiltinModule::Assert
        | BuiltinModule::AssertStrict
        | BuiltinModule::Path
        | BuiltinModule::PathPosix
        | BuiltinModule::PathWin32
        | BuiltinModule::InternalDgram
        | BuiltinModule::InternalTestBinding
        | BuiltinModule::InternalBlockList
        | BuiltinModule::InternalSocketAddress
        | BuiltinModule::InternalUndici => Err(RootedError::host(
            "special builtin passed to generic shared module builder",
        )),
    }
}

enum Request {
    Require,
    Resolve,
}

fn specifier(
    context: &mut Context<'_>,
    args: &[RootId],
    request: Request,
) -> Result<String, RootedError> {
    let argument = args.first().copied().unwrap_or_else(|| context.undefined());
    match context.string_text(argument)? {
        Some(value) if !value.is_empty() || matches!(request, Request::Resolve) => Ok(value),
        Some(_) => {
            let error = context
                .type_error_rooted("The argument 'id' must be a non-empty string. Received ''")?;
            let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
            set(context, error, "code", code)?;
            Err(context.throw(error))
        }
        None => {
            let value = context
                .rooted_value(argument)
                .ok_or_else(|| RootedError::host("invalid request root"))?;
            let received = if value.is_undefined() || value.is_null() {
                context.to_string(argument)?
            } else if value.as_number().is_some() {
                format!("type number ({})", context.to_string(argument)?)
            } else if value.as_bool().is_some() {
                format!("type boolean ({})", context.to_string(argument)?)
            } else {
                "an instance of Object".to_owned()
            };
            let name = match request {
                Request::Require => "id",
                Request::Resolve => "request",
            };
            let error = context.type_error_rooted(&format!(
                "The \"{name}\" argument must be of type string. Received {received}"
            ))?;
            let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
            set(context, error, "code", code)?;
            Err(context.throw(error))
        }
    }
}

fn resolve_filename(
    context: &mut Context<'_>,
    specifier: &str,
    parent: RootId,
) -> Result<PathBuf, RootedError> {
    resolve_filename_in(context, specifier, parent, None)
}

/// `Module._resolveFilename`: `lookup` holds `options.paths`, the directories a request
/// resolves from in order instead of its parent's directory.
fn resolve_filename_in(
    context: &mut Context<'_>,
    specifier: &str,
    parent: RootId,
    lookup: Option<&[PathBuf]>,
) -> Result<PathBuf, RootedError> {
    let filename = get(context, parent, "filename")?;
    let filename = context.string_text(filename)?;
    #[cfg(feature = "native-execution")]
    if lookup.is_none() {
        if let Some(target) = filename.as_deref().and_then(|parent| context.host_mut().embedded_resolution(Path::new(parent), specifier)) {
            return Ok(target);
        }
    }
    let resolved = resolve_filename_from_host(context, specifier, parent, lookup, filename.as_deref());
    #[cfg(feature = "aot")]
    if lookup.is_none() {
        if let (Some(parent), Ok(target)) = (filename.as_deref(), &resolved) {
            context.host_mut().record_module_resolution(Path::new(parent), specifier, target).map_err(RootedError::host)?;
        }
    }
    resolved
}

fn resolve_filename_from_host(
    context: &mut Context<'_>, specifier: &str, parent: RootId,
    lookup: Option<&[PathBuf]>, filename: Option<&str>,
) -> Result<PathBuf, RootedError> {
    let directory = match filename {
        Some(filename) => Path::new(filename).parent().unwrap_or(Path::new(".")).to_owned(),
        None => {
            let directory = get(context, parent, "path")?;
            let directory = context
                .string_text(directory)?
                .ok_or_else(|| RootedError::host("invalid parent module path"))?;
            PathBuf::from(directory)
        }
    };
    let node_lib = context.host_mut().node_library_root.join("lib");
    let internal_specifier = specifier.strip_prefix("node:").unwrap_or(specifier);
    let node_internal = (internal_specifier.starts_with("internal/")
        && (internal_specifier != "internal/timers" || internals_exposed(context)))
        || (internal_specifier.starts_with('_')
            && module_is_file(context, &node_lib.join(internal_specifier).with_extension("js")));
    if !node_internal {
        if let Some(path) = resolve_vfs_module_path(context, &directory, specifier)? {
            return Ok(path);
        }
    }
    #[cfg(feature = "native-execution")]
    {
        let virtual_path = if node_internal {
            Some(node_lib.join(internal_specifier).with_extension("js"))
        } else if specifier.starts_with("./") || specifier.starts_with("../") {
            Some(directory.join(specifier).with_extension("js"))
        } else {
            None
        };
        if let Some(path) = virtual_path {
            if context.host_mut().embedded_module(&path).is_some() {
                return Ok(path);
            }
        }
    }
    let (bases, resolved_specifier) = if node_internal {
        (vec![node_lib], format!("./{internal_specifier}"))
    } else {
        (lookup.map_or_else(|| vec![directory], <[PathBuf]>::to_vec), specifier.to_owned())
    };
    // Each base in order: an existing explicit file, else the resolver's answer. The first
    // that is not "not found" decides, as does the last base's; a failure keeps its base.
    let resolve_in_bases = |resolver: &oxc_resolver::Resolver| {
        let mut failure = (
            bases.first().map_or_else(|| PathBuf::from("."), PathBuf::clone),
            oxc_resolver::ResolveError::NotFound(resolved_specifier.clone()),
        );
        for directory in &bases {
            if let Some(path) = exact_file_path(directory, &resolved_specifier) {
                return Ok(Err(path));
            }
            match resolver.resolve(directory, &resolved_specifier) {
                Ok(resolution) => return Ok(Ok(resolution)),
                Err(error @ oxc_resolver::ResolveError::NotFound(_)) => failure = (directory.clone(), error),
                Err(error) => return Err((directory.clone(), error)),
            }
        }
        Err(failure)
    };
    let addons_enabled = context.host_mut().shared_state().borrow().cli_options.enabled("--addons");
    let (outcome, missing_package) = with_commonjs_resolver_for(addons_enabled, |resolver| match resolve_kept(resolver, resolve_in_bases) {
        Ok(found) => (Ok(found), None),
        Err((base, error)) => {
            let missing_package = (matches!(error, oxc_resolver::ResolveError::NotFound(_))
                && !node_internal
                && is_bare_specifier(specifier))
            .then(|| super::esm_errors::missing_package_file(resolver, &base, specifier))
            .flatten();
            (Err(error), missing_package)
        }
    });
    let outcome = match outcome {
        Ok(Err(exact)) => {
            return crate::host::real_path::real_path(&exact).map_err(|error| RootedError::host(error.to_string()));
        }
        Ok(Ok(resolution)) => Ok(resolution),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(resolution) => Ok(resolution.full_path()),
        Err(oxc_resolver::ResolveError::NotFound(_)) if let Some(super::esm_errors::MissingPackageFile::ExportsTarget { target, package }) =
            missing_package =>
        {
            // `finalizeEsmResolution`: the missing target, with the package as `path`.
            let error = context.error_rooted(&format!("Cannot find module '{}'", target.display()))?;
            let code = context.string_rooted("MODULE_NOT_FOUND");
            set(context, error, "code", code)?;
            let path = context.string_rooted(&package.to_string_lossy());
            set(context, error, "path", path)?;
            Err(context.throw(error))
        }
        Err(oxc_resolver::ResolveError::NotFound(_) | oxc_resolver::ResolveError::Specifier(_)) => {
            let mut stack = Vec::new();
            let mut module = parent;
            loop {
                let name = get(context, module, "filename")?;
                if let Some(name) = context.string_text(name)? {
                    stack.push(name);
                }
                module = get(context, module, "parent")?;
                if context
                    .rooted_value(module)
                    .is_some_and(|value| value.is_null() || value.is_undefined())
                {
                    break;
                }
            }
            let message = format!(
                "Cannot find module '{specifier}'\nRequire stack:\n{}",
                stack
                    .iter()
                    .map(|name| format!("- {name}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let error = context.error_rooted(&message)?;
            let code = context.string_rooted("MODULE_NOT_FOUND");
            set(context, error, "code", code)?;
            let names = stack
                .iter()
                .map(|name| context.string_rooted(name))
                .collect::<Vec<_>>();
            let stack = context.array_rooted(&names)?;
            set(context, error, "requireStack", stack)?;
            Err(context.throw(error))
        }
        Err(error) => match super::esm_errors::require_unresolved(&error) {
            Some(error) => Err(throw_module_load_error(context, error)?),
            None => Err(RootedError::host(error.to_string())),
        },
    }
}

fn resolve_vfs_module_path(
    context: &mut Context<'_>,
    base: &Path,
    specifier: &str,
) -> Result<Option<PathBuf>, RootedError> {
    // The loader projection exists only under `--experimental-vfs` (`install_vfs_globals`).
    if !context.host_mut().vfs_enabled() {
        return Ok(None);
    }
    let global = context.global_root()?;
    let resolve = get(context, global, "__quenchVfsResolveModulePath")?;
    if !context.is_callable_rooted(resolve)? {
        return Ok(None);
    }
    let base = context.string_rooted(&base.to_string_lossy());
    let specifier = context.string_rooted(specifier);
    let receiver = context.undefined();
    let resolved = context.call_rooted(resolve, receiver, &[base, specifier])?;
    Ok(context.string_text(resolved)?.map(PathBuf::from))
}

fn read_vfs_module_source(
    context: &mut Context<'_>,
    filename: &Path,
) -> Result<Option<String>, RootedError> {
    // The loader projection exists only under `--experimental-vfs` (`install_vfs_globals`).
    if !context.host_mut().vfs_enabled() {
        return Ok(None);
    }
    let global = context.global_root()?;
    let read_source = get(context, global, "__quenchVfsReadModuleSource")?;
    if !context.is_callable_rooted(read_source)? {
        return Ok(None);
    }
    let filename = context.string_rooted(&filename.to_string_lossy());
    let receiver = context.undefined();
    let source = context.call_rooted(read_source, receiver, &[filename])?;
    context.string_text(source)
}

fn vfs_module_type(
    context: &mut Context<'_>,
    filename: &Path,
) -> Result<Option<String>, RootedError> {
    // The loader projection exists only under `--experimental-vfs` (`install_vfs_globals`).
    if !context.host_mut().vfs_enabled() {
        return Ok(None);
    }
    let global = context.global_root()?;
    let module_type = get(context, global, "__quenchVfsModuleType")?;
    if !context.is_callable_rooted(module_type)? {
        return Ok(None);
    }
    let filename = context.string_rooted(&filename.to_string_lossy());
    let receiver = context.undefined();
    let result = context.call_rooted(module_type, receiver, &[filename])?;
    context.string_text(result)
}

/// Captured once by bootstrap's first module, before preloads or guest code.
/// The weak keys preserve Node's private first-parent relation independently of
/// the mutable public module.parent projection.
fn module_parent_api(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(api) = context.host_mut().shared_state().borrow().module_parent_api {
        return Ok(api);
    }
    let global = context.global_root()?;
    let constructor = get(context, global, "WeakMap")?;
    let map = context.construct_rooted(constructor, constructor, &[])?;
    let prototype = get(context, constructor, "prototype")?;
    let setter = get(context, prototype, "set")?;
    let api = context.null_object_rooted()?;
    set(context, api, "map", map)?;
    set(context, api, "set", setter)?;
    let array = get(context, global, "Array")?;
    let prototype = get(context, array, "prototype")?;
    for name in ["push", "join"] {
        let method = get(context, prototype, name)?;
        set(context, api, name, method)?;
    }
    let reflect = get(context, global, "Reflect")?;
    let delete = get(context, reflect, "deleteProperty")?;
    set(context, api, "deleteProperty", delete)?;
    let map_constructor = get(context, global, "Map")?;
    let paths = context.construct_rooted(map_constructor, map_constructor, &[])?;
    set(context, api, "relativePaths", paths)?;
    let prototype = get(context, map_constructor, "prototype")?;
    for (key, name) in [("mapGet", "get"), ("mapSet", "set")] {
        let method = get(context, prototype, name)?;
        set(context, api, key, method)?;
    }
    let api = context.retain(api)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .module_parent_api = Some(api);
    Ok(api)
}

/// Records the public CJS module prototype while the `node:module` builtin is
/// being initialized. CommonJS module records created after this point inherit
/// the real Module prototype, so `module.parent` goes through its getter and
/// setter instead of being shadowed by an own data property.
pub(crate) fn register_module_parent_accessors(
    context: &mut Context<'_>,
    prototype: RootId,
    native_parent_setter: RootId,
) -> Result<(), RootedError> {
    let api = module_parent_api(context)?;
    set(context, api, "modulePrototype", prototype)?;
    set(context, api, "moduleParentSetter", native_parent_setter)
}

fn delete_relative_resolution(
    context: &mut Context<'_>,
    cache: RootId,
    specifier: &str,
) -> Result<(), RootedError> {
    let api = module_parent_api(context)?;
    let delete = get(context, api, "deleteProperty")?;
    let key = context.string_rooted(specifier);
    let undefined = context.undefined();
    context.call_rooted(delete, undefined, &[cache, key])?;
    Ok(())
}

pub(crate) fn commonjs_cache(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    use crate::host::node_host::CommonJsCache;
    let shared = context.host_mut().shared_state();
    let owner = shared.borrow().commonjs_cache;
    match owner {
        Some(CommonJsCache::Bootstrap(cache)) => Ok(cache),
        Some(CommonJsCache::Module(module)) => get(context, module, "_cache"),
        None => {
            let cache = context.null_object_rooted()?;
            let cache = context.retain(cache)?;
            shared.borrow_mut().commonjs_cache = Some(CommonJsCache::Bootstrap(cache));
            Ok(cache)
        }
    }
}

pub(crate) fn attach_module_cache(
    context: &mut Context<'_>,
    module: RootId,
) -> Result<(), RootedError> {
    use crate::host::node_host::CommonJsCache;
    let cache = commonjs_cache(context)?;
    set(context, module, "_cache", cache)?;
    let owner = context.retain(module)?;
    let previous = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .commonjs_cache
        .replace(CommonJsCache::Module(owner));
    if let Some(CommonJsCache::Bootstrap(root) | CommonJsCache::Module(root)) = previous {
        context.release_root(root);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ModuleCache {
    NodeLibrary,
    CommonJs,
}

impl ModuleCache {
    fn get(self, context: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
        match self {
            Self::NodeLibrary => Ok(context
                .host_mut()
                .shared_state()
                .borrow()
                .builtin_cache
                .get(name)
                .copied()
                .unwrap_or_else(|| context.undefined())),
            Self::CommonJs => {
                let cache = commonjs_cache(context)?;
                get(context, cache, name)
            }
        }
    }

    fn insert(
        self,
        context: &mut Context<'_>,
        name: &str,
        module: RootId,
    ) -> Result<(), RootedError> {
        match self {
            Self::NodeLibrary => {
                let root = context.retain(module)?;
                let old = context
                    .host_mut()
                    .shared_state()
                    .borrow_mut()
                    .builtin_cache
                    .insert(name.to_owned(), root);
                if let Some(old) = old {
                    context.release_root(old);
                }
                Ok(())
            }
            Self::CommonJs => {
                let cache = commonjs_cache(context)?;
                context.assign_named_property_rooted(cache, name, module)
            }
        }
    }

    fn remove(self, context: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
        match self {
            Self::NodeLibrary => {
                let old = context
                    .host_mut()
                    .shared_state()
                    .borrow_mut()
                    .builtin_cache
                    .remove(name);
                if let Some(old) = old {
                    context.release_root(old);
                }
                Ok(())
            }
            Self::CommonJs => {
                let cache = commonjs_cache(context)?;
                let api = module_parent_api(context)?;
                let delete = get(context, api, "deleteProperty")?;
                let key = context.string_rooted(name);
                let undefined = context.undefined();
                let deleted = context.call_rooted(delete, undefined, &[cache, key])?;
                if !context.truthy_rooted(deleted)? {
                    let error = context.type_error_rooted(&format!(
                        "Cannot delete property '{name}' of [object Object]"
                    ))?;
                    return Err(context.throw(error));
                }
                Ok(())
            }
        }
    }
}

fn first_module_parent(context: &mut Context<'_>, module: RootId) -> Result<RootId, RootedError> {
    let api = module_parent_api(context)?;
    let map = get(context, api, "map")?;
    context.weak_map_get_rooted(map, module)
}

fn require_stack(context: &mut Context<'_>, parent: RootId) -> Result<RootId, RootedError> {
    let api = module_parent_api(context)?;
    let push = get(context, api, "push")?;
    let stack = context.array_rooted(&[])?;
    let mut cursor = parent;
    while context.truthy_rooted(cursor)? {
        let filename = get(context, cursor, "filename")?;
        let name = if context.truthy_rooted(filename)? {
            filename
        } else {
            get(context, cursor, "id")?
        };
        context.call_rooted(push, stack, &[name])?;
        cursor = first_module_parent(context, cursor)?;
    }
    Ok(stack)
}

fn module_record(
    context: &mut Context<'_>,
    filename: &Path,
    parent: Option<RootId>,
    is_main: bool,
) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let exports = context.object_rooted()?;
    set(context, module, "exports", exports)?;
    let id = if parent.is_none() {
        if is_main {
            "."
        } else {
            "[eval]"
        }
    } else {
        filename
            .to_str()
            .ok_or_else(|| RootedError::host("module filename is not UTF-8"))?
    };
    let id = context.string_rooted(id);
    set(context, module, "id", id)?;
    let name = context.string_rooted(&filename.to_string_lossy());
    set(context, module, "filename", name)?;
    let directory = filename.parent().unwrap_or(Path::new("."));
    let path = context.string_rooted(&directory.to_string_lossy());
    set(context, module, "path", path)?;
    let parent = parent.unwrap_or_else(|| context.null());
    let api = module_parent_api(context)?;
    let module_prototype = get(context, api, "modulePrototype")?;
    if context.is_object_rooted(module_prototype)? {
        if !context.set_prototype_rooted(module, module_prototype)? {
            context.release_root(module_prototype);
            return Err(RootedError::host(
                "CommonJS module rejected the public Module prototype",
            ));
        }
        let parent_setter = get(context, api, "moduleParentSetter")?;
        let set_parent = context.call_rooted(parent_setter, module, &[parent]);
        context.release_root(parent_setter);
        set_parent?;
    } else {
        set(context, module, "parent", parent)?;
    }
    context.release_root(module_prototype);
    let map = get(context, api, "map")?;
    let setter = get(context, api, "set")?;
    context.call_rooted(setter, map, &[module, parent])?;
    let loaded = context.boolean(false);
    set(context, module, "loaded", loaded)?;
    let children = context.array_rooted(&[])?;
    set(context, module, "children", children)?;
    let paths = crate::modules::module_shared_vm::node_search_paths(directory)
        .map(|path| context.string_rooted(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    let paths = context.array_rooted(&paths)?;
    set(context, module, "paths", paths)?;
    let process = match context.host_mut().shared_state().borrow().process_module {
        Some(root) => root,
        _ => {
            return Err(RootedError::host(
                "shared process module is not initialized",
            ));
        }
    };
    if is_main
        && context
            .rooted_value(parent)
            .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        set(context, process, "mainModule", module)?;
    }
    let require = context.host_function_with_data(super::operation("require"), module)?;
    install_resolve(context, require, module)?;
    let main = get(context, process, "mainModule")?;
    set(context, require, "main", main)?;
    let cache = commonjs_cache(context)?;
    set(context, require, "cache", cache)?;
    set(context, module, "require", require)?;
    Ok(module)
}

/// `require.resolve` and `require.resolve.paths` for `module` (`makeRequireFunction`).
fn install_resolve(context: &mut Context<'_>, require: RootId, module: RootId) -> Result<(), RootedError> {
    let resolve = context.host_function_with_data(super::operation("resolve"), module)?;
    let paths = context.host_function_named_with_data(super::operation("resolvePaths"), "paths", module)?;
    set(context, resolve, "paths", paths)?;
    set(context, require, "resolve", resolve)
}

pub(crate) fn create_require(
    context: &mut Context<'_>,
    filename: &Path,
) -> Result<RootId, RootedError> {
    let module = module_record(context, filename, None, false)?;
    get(context, module, "require")
}

fn require_module_error(
    context: &mut Context<'_>,
    code: &'static str,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = context.error_rooted(message)?;
    crate::modules::node_errors_shared_vm::decorate_error(context, error, code)?;
    Ok(context.throw(error))
}

/// Throw a module-load failure as Node's coded error.
fn throw_module_load_error(context: &mut Context<'_>, error: quench_runtime::ModuleLoadError) -> Result<RootedError, RootedError> {
    let exception = match error.kind {
        quench_runtime::ModuleLoadErrorKind::Error => context.error_rooted(&error.message)?,
        quench_runtime::ModuleLoadErrorKind::TypeError => context.type_error_rooted(&error.message)?,
    };
    match (error.kind, error.code) {
        (_, None) => {}
        (quench_runtime::ModuleLoadErrorKind::Error, Some(code)) => crate::modules::node_errors_shared_vm::decorate_error(context, exception, code)?,
        (quench_runtime::ModuleLoadErrorKind::TypeError, Some(code)) => {
            crate::modules::node_errors_shared_vm::decorate_type_error(context, exception, code)?
        }
    }
    Ok(context.throw(exception))
}

fn require_es_module(context: &mut Context<'_>, filename: &Path, parent: Option<RootId>) -> Result<RootId, RootedError> {
    if super::esm_errors::node_modules_typescript(filename) {
        let error = super::esm_errors::type_stripping_unsupported(&filename.to_string_lossy());
        return Err(throw_module_load_error(context, error)?);
    }
    let url = url::Url::from_file_path(filename).map_err(|_| RootedError::host("invalid ES module filename"))?;
    let module = if let Some(source) = read_vfs_module_source(context, filename)? {
        // VFS providers live in the guest realm, so the NodeHost's ordinary
        // file resolver cannot read their source. A direct ESM module with no
        // host-side graph lookup can still be evaluated from the projected
        // bytes; static dependencies continue through the host resolver.
        quench_runtime::ModuleSource {
            name: filename.to_string_lossy().into_owned(),
            typescript: false,
            source: source.into(),
            original_bytes: None,
        }
    } else {
        match quench_runtime::Host::resolve_dynamic_import(context.host_mut(), &filename.to_string_lossy(), url.as_str()) {
            Ok(module) => module,
            Err(error) => return Err(throw_module_load_error(context, error)?),
        }
        .ok_or_else(|| RootedError::host("ES module source unavailable"))?
    };
    let namespace = match context.evaluate_module_sync_rooted(&module)? {
        quench_runtime::SynchronousModule::Namespace(namespace) => namespace,
        quench_runtime::SynchronousModule::AsyncGraph => {
            let mut units = format!("require() cannot be used on an ESM graph with top-level await. Use import() instead. To see where the top-level await comes from, use --experimental-print-required-tla.\nRequired module: {}", filename.display()).encode_utf16().collect::<Vec<_>>();
            let stack = if let Some(parent) = parent {
                Some(require_stack(context, parent)?)
            } else {
                None
            };
            if let Some(stack) = stack {
                let length = get(context, stack, "length")?;
                if context.truthy_rooted(length)? {
                    let api = module_parent_api(context)?;
                    let join = get(context, api, "join")?;
                    let separator = context.string_rooted("\n- ");
                    let joined = context.call_rooted(join, stack, &[separator])?;
                    units.extend("\nRequire stack:\n- ".encode_utf16());
                    units.extend(context.to_string_code_units(joined)?);
                }
            }
            let error = context.error_rooted("")?;
            let message = context.string_units_rooted(&units);
            context.assign_named_property_rooted(error, "message", message)?;
            crate::modules::node_errors_shared_vm::decorate_error(
                context,
                error,
                "ERR_REQUIRE_ASYNC_MODULE",
            )?;
            if let Some(stack) = stack {
                let key = context.string_rooted("requireStack");
                context.define_data_property_rooted(error, key, stack, true, false, true)?;
            }
            return Err(context.throw(error));
        }
        quench_runtime::SynchronousModule::Evaluating => {
            let mut message = format!(
                "Cannot require() ES Module {} in a cycle.",
                filename.display()
            );
            if let Some(parent) = parent {
                let name = get(context, parent, "filename")?;
                message.push_str(&format!(" (from {})", context.to_string(name)?));
            }
            message.push_str(" A cycle involving require(esm) is not allowed to maintain invariants mandated by the ECMAScript specification. Try making at least part of the dependency in the graph lazily loaded.");
            return Err(require_module_error(
                context,
                "ERR_REQUIRE_CYCLE_MODULE",
                &message,
            )?);
        }
    };
    let key = context.string_rooted("module.exports");
    if context.has_property_rooted(namespace, key)? {
        return context.get_property_rooted(namespace, key);
    }
    let default = context.string_rooted("default");
    let marker = context.string_rooted("__esModule");
    if context.has_property_rooted(namespace, default)?
        && !context.has_property_rooted(namespace, marker)?
    {
        let yes = context.boolean(true);
        context.module_namespace_with_export_rooted(namespace, "__esModule", yes)
    } else {
        Ok(namespace)
    }
}

/// Whether the pinned Node library is available (its primordials module stands for it).
pub(crate) fn node_library_present(context: &mut Context<'_>) -> bool {
    node_primordials_source(context).is_some()
}

/// The pinned Node library's primordials module, when the library is present.
fn node_primordials_source(context: &mut Context<'_>) -> Option<PathBuf> {
    let root = context.host_mut().node_library_root.clone();
    let path = root.join("lib/internal/per_context/primordials.js");
    module_is_file(context, &path).then_some(path)
}

/// Node's primordials snapshot, which only Node's internal modules and their host
/// counterparts read. Node initialization captures it before guests can make
/// public intrinsic properties nonconfigurable; later consumers share that owner.
pub(crate) fn node_primordials(context: &mut Context<'_>) -> Result<Option<RootId>, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(primordials) = state.borrow().primordials_module {
        return Ok(Some(primordials));
    }
    let Some(path) = node_primordials_source(context) else {
        return Ok(None);
    };
    // Node builds its primordials before `Error.prepareStackTrace` exists; materializing
    // that lazy property first builds them from inside its initializer, where it reads unset.
    crate::modules::node_errors_shared_vm::materialize_prepare_stack_trace(context)?;
    if let Some(primordials) = state.borrow().primordials_module {
        return Ok(Some(primordials));
    }
    let exports = context
        .with_startup_intrinsics(|context| load(context, &path, None, EntryGoal::CommonJs))?;
    let retained = context.retain(exports)?;
    context.release_root(exports);
    state.borrow_mut().primordials_module = Some(retained);
    Ok(Some(retained))
}

fn module_is_file(context: &mut Context<'_>, path: &Path) -> bool {
    #[cfg(feature = "native-execution")]
    if context.host_mut().embedded_module(path).is_some() { return true; }
    let _ = context;
    path.is_file()
}

fn is_node_library_source(context: &mut Context<'_>, path: &Path) -> bool {
    // Only frozen images can still hold library sources; quench embeds none.
    #[cfg(feature = "native-execution")]
    if path.starts_with(&context.host_mut().node_library_root) && context.host_mut().embedded_module(path).is_some_and(|file| file.kind == quench_runtime::SourceKind::Script) {
        return true;
    }
    let _ = (context, path);
    false
}

/// Read a module's original bytes through the selected module input provider.
fn module_file_bytes(context: &mut Context<'_>, path: &Path, kind: quench_runtime::SourceKind) -> std::io::Result<Vec<u8>> {
    #[cfg(feature = "native-execution")]
    { context.host_mut().module_bytes(path, kind) }
    #[cfg(not(feature = "native-execution"))]
    { let _ = (context, kind); std::fs::read(path) }
}

fn load(
    context: &mut Context<'_>,
    filename: &Path,
    parent: Option<RootId>,
    goal: EntryGoal,
) -> Result<RootId, RootedError> {
    load_with_resolution(context, filename, parent, goal, None)
}

fn load_with_resolution(
    context: &mut Context<'_>,
    filename: &Path,
    parent: Option<RootId>,
    goal: EntryGoal,
    resolution: Option<(RootId, &str)>,
) -> Result<RootId, RootedError> {
    let key = filename.to_string_lossy().into_owned();
    let roots = context.host_mut().shared_state();
    let active = roots.borrow().active_esm_requires.get(&key).copied();
    if let Some(active) = active {
        let mut message = format!("Cannot require() ES Module {key} in a cycle.");
        let parent = first_module_parent(context, active)?;
        if context.truthy_rooted(parent)? {
            let name = get(context, parent, "filename")?;
            message.push_str(&format!(" (from {})", context.to_string(name)?));
        }
        return Err(require_module_error(
            context,
            "ERR_REQUIRE_CYCLE_MODULE",
            &message,
        )?);
    }
    let library_root = context.host_mut().node_library_root.clone();
    let library_relative = filename.strip_prefix(&library_root).ok();
    let is_primordials =
        library_relative == Some(Path::new("lib/internal/per_context/primordials.js"));
    let in_node_internal = library_relative.is_some_and(|path| path.starts_with("lib"));
    // A library module sees Node's bootstrap done before it starts, as in Node: building
    // the primordials first may itself load this module (`Error.prepareStackTrace` loads
    // `internal/errors`), which the cache then returns instead of a partial copy.
    let primordials = if in_node_internal && !is_primordials {
        node_primordials(context)?
    } else {
        None
    };
    let cache = if is_node_library_source(context, filename) {
        ModuleCache::NodeLibrary
    } else { ModuleCache::CommonJs };
    let cached = cache.get(context, &key)?;
    if !context
        .rooted_value(cached)
        .is_some_and(|value| value.is_undefined())
    {
        if let Some(parent) = parent {
            transition_child(context, parent, cached, ChildTransition::Attach)?;
        }
        return get(context, cached, "exports");
    }
    // A file whose goal its text decides compiles as CommonJS first, as Node's loader does:
    // only a wrapper that does not compile asks the text for ES module syntax, so a CommonJS
    // file is parsed once, not again for detection (typescript.js: a 9 MB text's tree and
    // tables, the peak of loading it).
    #[cfg(feature = "native-execution")]
    let archived_kind = context.host_mut().embedded_module(filename).map(|file| file.kind);
    #[cfg(not(feature = "native-execution"))]
    let archived_kind: Option<quench_runtime::SourceKind> = None;
    // A file of a mounted VFS layer has no host path: its text and package type are the
    // layer's (`__quenchVfsReadModuleSource`, `__quenchVfsModuleType`).
    let vfs_source = read_vfs_module_source(context, filename)?;
    let vfs_kind = match vfs_source {
        Some(_) => Some(match filename.extension().and_then(|extension| extension.to_str()) {
            Some("mjs") => quench_runtime::SourceKind::Module,
            Some("cjs") => quench_runtime::SourceKind::Script,
            _ if vfs_module_type(context, filename)?.as_deref() == Some("module") => quench_runtime::SourceKind::Module,
            _ => quench_runtime::SourceKind::Script,
        }),
        None => None,
    };
    let declared = match goal {
        EntryGoal::Node if archived_kind.is_some() => archived_kind,
        EntryGoal::Node if vfs_kind.is_some() => vfs_kind,
        EntryGoal::Node => declared_source_kind(&real_path_for_kind(filename).map_err(RootedError::host)?)
            .map_err(RootedError::host)?,
        _ => Some(quench_runtime::SourceKind::Script),
    };
    let is_main = parent.is_none() && context.host_mut().commonjs_entry.is_some();
    let module = module_record(context, filename, parent, is_main)?;
    if let Some(parent) = parent {
        transition_child(context, parent, module, ChildTransition::Attach)?;
    }
    cache.insert(context, &key, module)?;
    if library_relative.is_some() {
        HOST_LOADS.with(|loads| loads.compiled.set(true));
    }
    if let Some((directory, specifier)) = resolution {
        let name = context.string_rooted(&key);
        set(context, directory, specifier, name)?;
    }
    let result = (|| {
        if declared == Some(quench_runtime::SourceKind::Module) {
            return load_es_module(context, &key, module, filename, parent);
        }
        if filename
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("node")
        {
            // Module._extensions['.node']: process.dlopen(module, filename).
            let path = context.string_rooted(&filename.to_string_lossy());
            crate::modules::napi::dlopen(context, module, &[module, path])?;
            let exports = get(context, module, "exports")?;
            let loaded = context.boolean(true);
            set(context, module, "loaded", loaded)?;
            return Ok(exports);
        }
        let file_text: std::borrow::Cow<'static, str> = match vfs_source {
            Some(source) => source.into(),
            None => match module_file_bytes(context, filename, declared.unwrap_or(quench_runtime::SourceKind::Script)) {
                Ok(bytes) => String::from_utf8(bytes)
                    .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
                    .into(),
                Err(error) => return Err(RootedError::host(error.to_string())),
            },
        };
        #[cfg(feature = "native-execution")]
        let prepared = context.host_mut().embedded_module(filename).and_then(|file| file.prepared);
        #[cfg(not(feature = "native-execution"))]
        let prepared: Option<()> = None;
        let file_text: std::borrow::Cow<'static, str> = match prepared {
            #[cfg(feature = "native-execution")]
            Some(crate::aot::PreparedModuleSource::Original) => file_text,
            #[cfg(feature = "native-execution")]
            Some(crate::aot::PreparedModuleSource::Projected(text)) => text.to_owned().into(),
            _ if quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY => {
                if library_relative.is_some() {
                    return Err(RootedError::host(format!("SDK module has no frozen source preparation: {}", filename.display())));
                }
                file_text
            }
            _ => {
                if library_relative.is_some() {
                    return Err(RootedError::host(format!("{}: Node library module is not implemented in Rust", filename.display())));
                }
                file_text
            }
        };
        let typed_commonjs = filename.extension().is_some_and(|extension| extension == "ts" || extension == "cts");
        let file_text = if prepared.is_none() && typed_commonjs {
            match quench_runtime::Engine::project_typescript_module_syntax(&file_text) {
                Some((std::borrow::Cow::Owned(source), _)) => source.into(),
                _ => file_text,
            }
        } else { file_text };
        #[cfg(feature = "aot")]
        if library_relative.is_some() || typed_commonjs {
            context.host_mut().record_prepared_module(filename, &file_text).map_err(RootedError::host)?;
        }

        // An embedded library file is static text the compiled program can retain as is;
        // a projected one is new text.
        let embedded = match file_text {
            std::borrow::Cow::Borrowed(text) => Some(script_body(text)),
            std::borrow::Cow::Owned(_) => None,
        };
        let text = file_text.as_ref();
        match filename
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("json") => {
                let exports = context.parse_json_rooted(text.strip_prefix('\u{feff}').unwrap_or(text))?;
                set(context, module, "exports", exports)?;
            }
            Some("mjs") => {
                return Err(RootedError::host(
                    "this module format is not implemented on the shared VM",
                ));
            }
            _ => {
                let text = script_body(text);
                let primordial_seed = if is_primordials {
                    Some(context.object_rooted()?)
                } else {
                    None
                };

                let exports = get(context, module, "exports")?;
                let public_require = get(context, module, "require")?;
                let require = if in_node_internal {
                    let require = context.host_function_named_with_data(
                        super::operation("nodeLibraryRequire"),
                        "require",
                        module,
                    )?;
                    for name in ["resolve", "main", "cache"] {
                        let value = get(context, public_require, name)?;
                        set(context, require, name, value)?;
                    }
                    require
                } else {
                    public_require
                };
                let name = context.string_rooted(&key);
                let directory = context.string_rooted(
                    &filename
                        .parent()
                        .unwrap_or(Path::new("."))
                        .to_string_lossy(),
                );
                let primordials = match primordial_seed.or(primordials) {
                    Some(root) => root,
                    None => context.undefined(),
                };
                // Node's own library carries an internal-binding preamble; quench runs none
                // of that library (decision 38).
                if library_relative.is_some() {
                    return Err(RootedError::host(format!(
                        "{}: Node library module is not implemented in Rust",
                        filename.display()
                    )));
                }
                let prefix = WRAPPER_PREFIX.to_owned();
                if parent.is_none() {
                    crate::host::resident_pages::release_before_entry_compile();
                }
                // Node's primordials file fills the object its wrapper passes and exports it.
                let (prefix, suffix) = if is_primordials {
                    (
                        format!("{prefix}primordials ??= {{}};\n"),
                        format!("\nmodule.exports = primordials;\n{WRAPPER_SUFFIX}"),
                    )
                } else {
                    (prefix, WRAPPER_SUFFIX.to_owned())
                };
                let wrapper = match embedded {
                    // The program retains the embedded text itself, not a wrapped copy.
                    Some(body) => context
                        .evaluate_specialized_static_body_rooted(&prefix, body, &suffix, &key)?,
                    // A library module's text the host projected from its embedded text keeps
                    // its records, as an embedded one does, and goes through the same cache.
                    None if in_node_internal => context
                        .evaluate_specialized_library_body_rooted(&prefix, text.to_owned(), &suffix, &key)?,
                    // A text an earlier run detected as a module loads as one, without the
                    // CommonJS compile that would fail again (a failed compile is not cached).
                    None if declared.is_none() && cached_source_kind(text) == Some(quench_runtime::SourceKind::Module) => {
                        drop(file_text);
                        return load_es_module(context, &key, module, filename, parent);
                    }
                    None => {
                        // The wrapper is built in the file's own buffer: growing a large one
                        // remaps its pages, where a second copy of typescript.js (9 MB) was
                        // briefly resident beside it. Its byte-order mark and `#!` line are
                        // what the prefix replaces.
                        let body_start = file_text.len() - text.len();
                        let mut wrapper_source = file_text.into_owned();
                        wrapper_source.reserve_exact(prefix.len() + suffix.len());
                        wrapper_source.replace_range(..body_start, &prefix);
                        wrapper_source.push_str(&suffix);
                        let typescript = matches!(
                            filename.extension().and_then(|extension| extension.to_str()),
                            Some("cts" | "ts"),
                        );
                        let wrapper = match evaluate_wrapper(context, &prefix, &wrapper_source, &key, is_main, typescript) {
                            Ok(wrapper) => wrapper,
                            Err(_) if !quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY && declared.is_none()
                                && detect_source_kind_for_path(filename, &wrapper_source[prefix.len()..wrapper_source.len() - suffix.len()])
                                    == quench_runtime::SourceKind::Module =>
                            {
                                drop(wrapper_source);
                                return load_es_module(context, &key, module, filename, parent);
                            }
                            Err(error) => return Err(error),
                        };
                        // The compiled program keeps its own source; the module body runs
                        // inside this call, so the wrapper is released before it.
                        drop(wrapper_source);
                        wrapper
                    }
                };
                if parent.is_none() {
                    crate::host::resident_pages::release_read_only_code_and_data();
                    crate::host::resident_pages::release_free_heap_pages();
                    crate::host::resident_pages::quench_entry_compiled();
                } else if in_node_internal {
                    crate::host::resident_pages::release_read_only_pages_after_load();
                }
                let mut arguments = vec![exports, require, module, name, directory];
                if in_node_internal {
                    arguments.push(primordials);
                    arguments.push(
                        context
                            .host_function(crate::host::shared_vm::operation("traceInternal"))?,
                    );
                }
                context.call_rooted(wrapper, exports, &arguments)?;
                if let Some(root) = primordial_seed {
                    context.release_root(root);
                }
            }
        }
        let exports = get(context, module, "exports")?;
        if library_relative == Some(Path::new("lib/internal/util/debuglog.js")) {
            let initialize = get(context, exports, "initializeDebugEnv")?;
            let initial = context.host_mut().initial_node_debug.clone();
            let debug = initial
                .map(|value| context.string_rooted(&value))
                .unwrap_or_else(|| context.undefined());
            let absent = context.undefined();
            context.call_rooted(initialize, absent, &[debug])?;
        }
        if library_relative == Some(Path::new("lib/internal/util/inspect.js")) {
            crate::modules::console_shared_vm::note_inspect_loaded(context)?;
        }
        if library_relative == Some(Path::new("lib/diagnostics_channel.js")) {
            crate::modules::console_shared_vm::note_channels_loaded(context)?;
        }
        let exports = get(context, module, "exports")?;
        if let Some(path) = library_relative {
            record_library_originals(context, path, exports)?;
        }
        let loaded = context.boolean(true);
        set(context, module, "loaded", loaded)?;
        Ok(exports)
    })();
    if let Some(retained) = roots.borrow_mut().active_esm_requires.remove(&key) {
        context.release_root(retained);
    }
    if result.is_err() {
        cache.remove(context, &key)?;
        if let Some((directory, specifier)) = resolution {
            delete_relative_resolution(context, directory, specifier)?;
        }
        if let Some(parent) = parent {
            transition_child(context, parent, module, ChildTransition::Detach)?;
        }
    }
    if library_relative == Some(Path::new("lib/internal/crypto/util.js")) {
        if let Ok(exports) = &result {
            if roots.borrow().crypto_handle_symbol.is_none() {
                let symbol = get(context, *exports, "kHandle")?;
                roots.borrow_mut().crypto_handle_symbol = Some(context.retain(symbol)?);
            }
        }
    }
    if library_relative == Some(Path::new("lib/internal/webidl.js")) {
        if let Ok(exports) = &result {
            crate::modules::events_shared_vm::adopt_abort_converter(context, *exports)?;
        }
    }
    let is_fs_utilities = library_relative == Some(Path::new("lib/internal/fs/utils.js"));
    if is_fs_utilities {
        if let Ok(exports) = &result {
            let state = get(context, *exports, "vfsState")?;
            let global = context.global_root()?;
            set(context, global, "\0quench:vfsState", state)?;
        }
    }
    result
}

/// `require` of an ES module: `module` is active until `load_with_resolution` finishes, so a
/// cycle back to it is the error Node reports.
fn load_es_module(
    context: &mut Context<'_>,
    key: &str,
    module: RootId,
    filename: &Path,
    parent: Option<RootId>,
) -> Result<RootId, RootedError> {
    let retained = context.retain(module)?;
    context.host_mut().shared_state().borrow_mut().active_esm_requires.insert(key.to_owned(), retained);
    let exports = require_es_module(context, filename, parent)?;
    set(context, module, "exports", exports)?;
    let loaded = context.boolean(true);
    set(context, module, "loaded", loaded)?;
    Ok(exports)
}

/// A CommonJS module's executed text without its initial hashbang comment. Keep
/// terminators and BOM whitespace: a BOM before `#!` is not a hashbang in Node.
pub(crate) fn script_body(text: &str) -> &str {
    if text.starts_with("#!") {
        text.find(oxc_syntax::line_terminator::is_line_terminator).map_or("", |end| &text[end..])
    } else {
        text
    }
}

/// Native compilation uses the ordinary loader's wrapper and source admission.
#[cfg(feature = "aot")]
pub(crate) fn native_commonjs_source(
    filename: &Path, bytes: &[u8], entry: bool,
) -> Result<quench_runtime::NativeSourceKey, String> {
    let decoded = source_text(bytes);
    let body = script_body(&decoded);
    let typescript = filename.extension().is_some_and(|extension| extension == "ts" || extension == "cts");
    if typescript && super::esm_errors::node_modules_typescript(filename) {
        return Err(format!("TypeScript in node_modules is not supported: {}", filename.display()));
    }
    let options = quench_runtime::HostScriptOptions {
        origin: Some(quench_runtime::wrapper_origin(WRAPPER_PREFIX).ok_or("CommonJS wrapper origin exceeds compiler positions")?),
        keep_records: entry, typescript,
    };
    let body = if typescript {
        quench_runtime::Engine::project_typescript_module_syntax(body).map(|(source, _)| source).unwrap_or(body.into())
    } else { body.into() };
    let source = [WRAPPER_PREFIX, body.as_ref(), WRAPPER_SUFFIX].concat();
    quench_runtime::NativeSourceKey::new(&filename.to_string_lossy(), &["specialized", &options.cache_goal(), &source])
}

/// Compiles and evaluates a CommonJS wrapper. An entry module's functions keep their compiled
/// records: an application's own code mostly runs, so compiling it again on first call would
/// only repeat work; the modules it loads compile their functions on first call.
fn evaluate_wrapper(
    context: &mut Context<'_>,
    prefix: &str,
    source: &str,
    filename: &str,
    entry: bool,
    typescript: bool,
) -> Result<RootId, RootedError> {
    let (line_offset, column_offset) = quench_runtime::wrapper_origin(prefix)
        .ok_or_else(|| RootedError::host("CommonJS wrapper origin exceeds compiler positions"))?;
    if typescript {
        if super::esm_errors::node_modules_typescript(Path::new(filename)) {
            let error = super::esm_errors::type_stripping_unsupported(filename);
            return Err(throw_module_load_error(context, error)?);
        }
        context.evaluate_typescript_script_with_origin_rooted(source, filename, line_offset, column_offset, entry)
    } else if entry {
        context.evaluate_entry_script_with_origin_rooted(source, filename, line_offset, column_offset)
    } else {
        context.evaluate_specialized_script_with_origin_rooted(source, filename, line_offset, column_offset)
    }
}

pub(crate) fn compile_public(
    context: &mut Context<'_>,
    module: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let content = args.first().copied().unwrap_or_else(|| context.undefined());
    let filename = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let content = context.string_text(content)?.ok_or_else(|| RootedError::host("Module._compile content must be a string"))?;
    let name = context.string_text(filename)?.ok_or_else(|| RootedError::host("Module._compile filename must be a string"))?;
    let text = script_body(content.as_str());
    let source = [WRAPPER_PREFIX, text, WRAPPER_SUFFIX].concat();
    let wrapper = evaluate_wrapper(context, WRAPPER_PREFIX, &source, &name, false, false)?;
    let exports = get(context, module, "exports")?;
    let require = context.host_function_with_data(super::operation("require"), module)?;
    install_resolve(context, require, module)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let main = get(context, process, "mainModule")?;
    set(context, require, "main", main)?;
    let directory = crate::modules::path_shared_vm::host_dirname(context, filename)?;
    context.call_rooted(
        wrapper,
        exports,
        &[exports, require, module, filename, directory],
    )?;
    Ok(context.undefined())
}

enum ChildTransition {
    Attach,
    Detach,
}

fn transition_child(
    context: &mut Context<'_>,
    parent: RootId,
    module: RootId,
    transition: ChildTransition,
) -> Result<(), RootedError> {
    let children = get(context, parent, "children")?;
    let length = get(context, children, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("invalid module children length"))?
        as usize;
    let mut position = None;
    for index in 0..length {
        let child = get(context, children, &index.to_string())?;
        if context.rooted_value(child) == context.rooted_value(module) {
            position = Some(index);
            break;
        }
    }
    match (transition, position) {
        (ChildTransition::Attach, None) => set(context, children, &length.to_string(), module),
        (ChildTransition::Detach, Some(position)) => {
            for index in position..length - 1 {
                let next = get(context, children, &(index + 1).to_string())?;
                set(context, children, &index.to_string(), next)?;
            }
            let length = context.number((length - 1) as f64);
            set(context, children, "length", length)
        }
        _ => Ok(()),
    }
}

fn get(context: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}

fn set(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot set module property {name}"
        )))
    }
}

pub(crate) fn node_library_module_in_realm(
    _: &mut Context<'_>,
    _: RootId,
    relative: &str,
    _: RootId,
    _: RootId,
) -> Result<RootId, RootedError> {
    crate::host::removed_javascript(format!("node:{relative}"))
}

/// Run the unchanged SDK worker policy after native process IPC and require
/// exist, matching initializeClusterIPC's startup boundary.
pub(crate) fn initialize_cluster_worker(context: &mut Context<'_>) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let process = shared
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process module owner unavailable"))?;
    let argv = get(context, process, "argv")?;
    let entry = get(context, argv, "1")?;
    if !context.truthy_rooted(entry)? {
        return Ok(());
    }
    // The environment as `process.env` starts from: reading `process.env` itself would copy
    // every variable on each run that is no cluster worker.
    if !crate::modules::process_shared_vm::environment(context)
        .iter()
        .any(|(name, value)| name == "NODE_UNIQUE_ID" && !value.is_empty())
    {
        return Ok(());
    }
    let env = get(context, process, "env")?;
    let cluster = cached_builtin(context, BuiltinModule::Cluster)?;
    let setup = get(context, cluster, "_setupWorker")?;
    context.call_rooted(setup, cluster, &[])?;
    context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let reflect = get(context, global, "Reflect")?;
        let delete = get(context, reflect, "deleteProperty")?;
        let key = context.string_rooted("NODE_UNIQUE_ID");
        context.call_rooted(delete, reflect, &[env, key])?;
        let undefined = context.undefined();
        crate::modules::process_shared_vm::delete_env(context, undefined, &[key])?;
        Ok(())
    })
}
