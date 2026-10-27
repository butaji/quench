//! CommonJS host policy. Guest code, objects and calls belong to the shared VM.

use crate::host::{EntryGoal, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};

/// The global key of the host's builtin `require`, which host code and bootstrap scripts
/// reach builtins and Node library modules through.
pub(crate) const BOOTSTRAP_REQUIRE: &str = "\0quench:require";

type Context<'a> = NativeContext<'a, NodeHost>;

// Node's public CommonJS wrapper; this is guest compilation input, not an API shim.
const WRAPPER_PREFIX: &str =
    "(function (exports, require, module, __filename, __dirname) { ";
const WRAPPER_SUFFIX: &str = "\n});";

/// A specifier naming a package rather than a relative or absolute path.
fn is_bare_specifier(specifier: &str) -> bool {
    !specifier.starts_with('.') && !Path::new(specifier).is_absolute()
}

/// The require resolver: each ancestor's `node_modules`, then Node's global paths.
fn new_commonjs_resolver() -> oxc_resolver::Resolver {
    let modules = std::iter::once(NODE_MODULES.to_owned())
        .chain(
            crate::modules::module_shared_vm::global_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned()),
        )
        .collect();
    oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        extensions: vec![".js".into(), ".json".into(), ".node".into()],
        main_files: vec!["index".into()],
        condition_names: vec!["node".into(), "require".into(), "default".into()],
        modules,
        ..Default::default()
    })
}

thread_local! {
    /// The CommonJS resolver, kept for the thread as Node keeps `Module._pathCache` and its
    /// package config reader for the process: its cache holds each path's metadata and
    /// `package.json`, which a resolver made per require read again (undici: its
    /// `package.json` 309 times).
    static COMMONJS_RESOLVER: oxc_resolver::Resolver = new_commonjs_resolver();
}

pub(super) fn with_commonjs_resolver<R>(use_resolver: impl FnOnce(&oxc_resolver::Resolver) -> R) -> R {
    COMMONJS_RESOLVER.with(use_resolver)
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
pub(super) fn resolve_kept<T, E>(
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

pub(crate) fn entry_path(path: &Path) -> Result<PathBuf, String> {
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
    with_commonjs_resolver(|resolver| resolve_kept(resolver, |resolver| resolver.resolve(&cwd, &path.to_string_lossy())))
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

/// The path whose package scope decides `path`'s goal: only a `.js` or extensionless file
/// asks its package scope, which is its real path's.
fn real_path_for_kind(path: &Path) -> Result<std::borrow::Cow<'_, Path>, String> {
    if matches!(path.extension().and_then(|extension| extension.to_str()), Some("js") | None) {
        return crate::host::real_path::real_path(path).map(std::borrow::Cow::Owned).map_err(|error| error.to_string());
    }
    Ok(std::borrow::Cow::Borrowed(path))
}

/// `source_kind` of a path that is already its real path, as a module the loader resolved.
pub(crate) fn source_kind_of_real(path: &Path) -> Result<quench_runtime::SourceKind, String> {
    match declared_source_kind(path)? {
        Some(kind) => Ok(kind),
        None => {
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            Ok(detect_source_kind(&source_text(&bytes)))
        }
    }
}

/// `source_kind_of_real` of a file whose bytes the caller already holds: detecting from
/// them keeps a second copy of a large file (typescript.js, 9 MB) from being read alongside.
pub(crate) fn source_kind_of_real_bytes(path: &Path, bytes: &[u8]) -> Result<quench_runtime::SourceKind, String> {
    Ok(declared_source_kind(path)?.unwrap_or_else(|| detect_source_kind(&source_text(bytes))))
}

/// The goal a real path's extension or package scope declares; none when Node's syntax
/// detection of its text decides (`detect_source_kind`).
fn declared_source_kind(path: &Path) -> Result<Option<quench_runtime::SourceKind>, String> {
    match path.extension().and_then(|extension| extension.to_str()) {
        // Node's `wasm` format is an ES module (`esm_wasm`).
        Some("mjs" | "wasm") => return Ok(Some(quench_runtime::SourceKind::Module)),
        Some("cjs" | "json" | "node") => return Ok(Some(quench_runtime::SourceKind::Script)),
        Some(extension) if extension != "js" => return Ok(None),
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
    if let Some(kind) = entry.as_ref().and_then(|entry| entry.load()).and_then(|bytes| detected_kind_of(bytes.as_ref().as_ref())) {
        return kind;
    }
    let kind = detect_text_kind(text);
    if let Some(entry) = entry {
        entry.store(&[detected_kind_byte(kind)]);
    }
    kind
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
    let parsed = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::unambiguous()).parse();
    if parsed.program.source_type.is_module() || valid_source(text, oxc_span::SourceType::mjs()) {
        quench_runtime::SourceKind::Module
    } else {
        quench_runtime::SourceKind::Script
    }
}

fn valid_source(source: &str, source_type: oxc_span::SourceType) -> bool {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    parsed.diagnostics.is_empty() && oxc_semantic::SemanticBuilder::new()
        .with_check_syntax_error(true).build(&parsed.program).diagnostics.is_empty()
}

pub(super) fn initialize(context: &mut Context<'_>) -> Result<(), RootedError> {
    let process_surface = context.evaluate_script_rooted(
        crate::polyfills::post_bootstrap::process_surface_00::JS,
        "node:bootstrap/process-surface.js",
    )?;
    context.release_root(process_surface);
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
        crate::polyfills::post_bootstrap::process_surface_00::LAZY_MEMBERS,
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
    context.scoped(crate::modules::crypto_shared_vm::install_global)?;
    if node_primordials_source(context).is_some() {
        // Node 24 exposes these well-known disposal symbols before its
        // primordial snapshot is built. The runtime may not provide them yet,
        // so seed stable symbols for Node's internal modules first.
        let seed_disposal_symbols = context.evaluate_script_rooted(
            "() => { Symbol.dispose ||= Symbol('Symbol.dispose'); Symbol.asyncDispose ||= Symbol('Symbol.asyncDispose'); }",
            "node:bootstrap/disposal-symbols.js",
        )?;
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
            context.scoped(crate::modules::util_shared_vm::initialize_call_site_policy)?;
            // Capture SDK process policies before exposed internal exports can be replaced.
            #[cfg(target_os = "linux")]
            {
                let cpu_usage = get(context, process, "cpuUsage")?;
                context.release_root(cpu_usage);
            }
        }
        context.seal_startup_intrinsics();
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
        let filename = std::fs::canonicalize(entry.path)
            .map_err(|error| RootedError::host(error.to_string()))?;
        load(context, &filename, None, entry.goal)?;
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
    let arguments = context.host_mut().shared_state().borrow().exec_argv.clone();
    let global = context.global_root()?;
    let require = get(context, global, BOOTSTRAP_REQUIRE)?;
    let undefined = context.undefined();
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        let specifier = match argument.as_str() {
            "-e" | "--eval" | "-p" | "--print" | "-pe" | "-ep" => break,
            "-r" | "--require" => {
                index += 1;
                arguments.get(index).map(String::as_str)
            }
            argument => argument.strip_prefix("--require="),
        };
        if let Some(specifier) = specifier {
            let specifier = context.string_rooted(specifier);
            context.call_rooted(require, undefined, &[specifier])?;
        }
        index += 1;
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
    let install = context.evaluate_script_rooted(
        "(path, stream) => Object.defineProperties(globalThis, { __nodePath: { configurable: true, value: path }, __nodeStream: { configurable: true, value: stream }, __nodeFdPaths: { configurable: true, value: {} }, __quenchVfsFdHandles: { configurable: true, value: new Map() } })",
        "node:bootstrap/vfs-globals.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install, undefined, &[path, stream])?;
    let global = context.global_root()?;
    for (name, export) in [("__quenchVfsReadStream", "VirtualReadStream"), ("__quenchVfsWriteStream", "VirtualWriteStream")] {
        let constructor = node_library_export(context, "internal/vfs/file_system.js", "internal/vfs/streams.js", export)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, constructor, false, false, true)?;
    }
    let registry = get(context, global, "__quenchVfsFdHandles")?;
    let map_get = get(context, registry, "get")?;
    let map_set = get(context, registry, "set")?;
    let map_methods = context.array_rooted(&[map_get, map_set])?;
    let bind_position = context.host_function_with_data(super::operation("vfsBindPosition"), map_methods)?;
    let key = context.string_rooted("__quenchVfsBindPosition");
    context.define_data_property_rooted(global, key, bind_position, false, false, true)?;
    let path_projection = context.host_function(super::operation("vfsStreamPath"))?;
    let key = context.string_rooted("__quenchVfsStreamPath");
    context.define_data_property_rooted(global, key, path_projection, false, false, true)?;
    for (name, operation) in [("__quenchVfsOpenDescriptor", "vfsDescriptorOpen"), ("__quenchVfsCloseDescriptor", "vfsDescriptorRelease")] {
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
pub(crate) fn vfs_descriptor_get(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    vfs_descriptor_lookup(context, args, false)
}
pub(crate) fn vfs_descriptor_close(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    vfs_descriptor_lookup(context, args, true)
}
fn vfs_descriptor_lookup(context: &mut Context<'_>, args: &[RootId], closing: bool) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let source = get(context, data, "0")?;
    let missing = context.undefined();
    let value = context.call_rooted(source, missing, args)?;
    let found = if closing { context.truthy_rooted(value)? } else { !context.rooted_value(value).is_some_and(|value| value.is_undefined()) };
    if found { return Ok(value); }
    let legacy = get(context, data, "1")?;
    let method = get(context, data, "2")?;
    context.call_rooted(method, legacy, args)
}
pub(crate) fn vfs_descriptor_open(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let module = cached_builtin(context, BuiltinModule::InternalVfsFd)?;
    let open = get(context, module, "openVirtualFd")?;
    let missing = context.undefined();
    context.call_rooted(open, missing, args)
}
pub(crate) fn vfs_descriptor_release(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
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
pub(crate) fn vfs_stream_path(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let stream = args.first().copied().unwrap_or(missing);
    let path = args.get(1).copied().unwrap_or(missing);
    let key = context.string_rooted("path");
    context.define_data_property_rooted(stream, key, path, true, true, true)?;
    Ok(stream)
}

/// Project the descriptor position onto the provider's existing offset authority.
pub(crate) fn vfs_bind_position(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
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
    for (name, value) in [("get", getter), ("set", setter)] { self::set(context, descriptor, name, value)?; }
    let key = context.string_rooted("position");
    context.define_property_descriptor_rooted(entry, key, descriptor)?;
    Ok(entry)
}

pub(crate) fn vfs_position_get(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let map = get(context, data, "0")?;
    let fd = get(context, data, "1")?;
    let getter = get(context, data, "2")?;
    context.call_rooted(getter, map, &[fd])
}

pub(crate) fn vfs_position_set(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let map = get(context, data, "0")?;
    let fd = get(context, data, "1")?;
    let setter = get(context, data, "3")?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    context.call_rooted(setter, map, &[fd, value])?;
    Ok(context.undefined())
}

/// Node's replaceable `performance` global loads `perf_hooks` on first access.
fn install_performance_global(context: &mut Context<'_>) -> Result<(), RootedError> {
    let root = context.evaluate_script_rooted(
        quench_js_check::checked_js!(
            r#"(() => {
  let value;
  let loaded = false;
  const accessors = {
    get performance() {
      if (!loaded) {
        value = globalThis["\0quench:require"]("perf_hooks").performance;
        loaded = true;
      }
      return value;
    },
    set performance(replacement) {
      value = replacement;
      loaded = true;
    },
  };
  Object.defineProperty(globalThis, "performance",
    Object.getOwnPropertyDescriptor(accessors, "performance"));
})()"#
        ),
        "node:bootstrap/performance-global.js",
    )?;
    context.release_root(root);
    Ok(())
}

/// The globals each web polyfill defines, in definition order. They are lazy data properties
/// (as in Node), so a polyfill is evaluated only when script first observes one of its globals.
/// A member materializes writable, so a polyfill defining one read-only says so explicitly.
const WEB_STREAM_GLOBALS: &[&str] = &[
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
const WEB_API_GLOBALS: &[&str] = &[
    "__quenchBlobEnsureReadable",
    "Blob",
    "File",
];
const FETCH_API_GLOBALS: &[&str] = &[
    "FormData",
    "Headers",
    "Request",
    "Response",
];

fn install_shared_web_globals(context: &mut Context<'_>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    for (operation, names) in [
        ("webStreamsInitialize", WEB_STREAM_GLOBALS),
        ("webApisInitialize", WEB_API_GLOBALS),
        ("webFetchInitialize", FETCH_API_GLOBALS),
    ] {
        let initializer = context.host_function(super::operation(operation))?;
        context.define_lazy_data_properties_rooted(global, names, false, initializer)?;
        context.release_root(initializer);
    }
    Ok(())
}

/// Evaluates a polyfill as a block, so its declarations stay its own instead of becoming
/// global lexical bindings that script could collide with.
fn evaluate_polyfill_block(
    context: &mut Context<'_>,
    source: &str,
    label: &str,
) -> Result<RootId, RootedError> {
    let root = context.evaluate_script_rooted(&format!("{{\n{source}\n}}"), label)?;
    context.release_root(root);
    Ok(context.undefined())
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
    #[cfg(target_os = "linux")]
    crate::modules::process_memory_shared_vm::install(context)?;
    evaluate_polyfill_block(
        context,
        crate::polyfills::post_bootstrap::process_surface_00::LAZY_JS,
        "node:bootstrap/process-surface-lazy.js",
    )?;
    #[cfg(target_os = "linux")]
    crate::modules::process_metrics_shared_vm::install(context)?;
    Ok(context.undefined())
}

pub(super) fn initialize_web_apis(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    evaluate_polyfill_block(
        context,
        crate::polyfills::bootstrap::globals_extra::web_api_source(),
        "node:bootstrap/shared-vm/web-apis.js",
    )?;
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
    for name in ["Headers", "Request", "Response", "FormData"] {
        let constructor = get(context, undici, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, constructor, true, false, true)?;
    }
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
    let specifier = specifier(context, args, Request::Require)?;
    let parent_path = get(context, parent, "path")?;
    let api = module_parent_api(context)?;
    let paths = get(context, api, "relativePaths")?;
    let map_get = get(context, api, "mapGet")?;
    let mut directory_cache = context.call_rooted(map_get, paths, &[parent_path])?;
    if !context.rooted_value(directory_cache).is_some_and(|value| value.is_undefined()) {
        let resolved = get(context, directory_cache, &specifier)?;
        if let Some(filename) = context.string_text(resolved)? {
            let cache = commonjs_cache(context)?;
            let cached = get(context, cache, &filename)?;
            if !context.rooted_value(cached).is_some_and(|value| value.is_undefined()) {
                if context.host_mut().shared_state().borrow().active_esm_requires.contains_key(&filename) {
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
        && requested_builtin(context,&specifier).is_some()
    {
        let cache = commonjs_cache(context)?;
        let cached = get(context, cache, &specifier)?;
        if !context.rooted_value(cached).is_some_and(|value| value.is_undefined()) {
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
    if specifier == "node:stream/iter" && !context.host_mut().shared_state().borrow().cli_options.enabled("--experimental-stream-iter") {
        let error=context.error_rooted("No such built-in module: node:stream/iter")?;
        let code=context.string_rooted("ERR_UNKNOWN_BUILTIN_MODULE");set(context,error,"code",code)?;
        return Err(context.throw(error));
    }
    if specifier == "node:vfs" && !context.host_mut().vfs_enabled() {
        let error = context.error_rooted(
            "ERR_UNKNOWN_BUILTIN_MODULE: No such built-in module: node:vfs",
        )?;
        let code = context.string_rooted("ERR_UNKNOWN_BUILTIN_MODULE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if specifier == "internal/bootstrap/realm" {
        let builtin_ids = BUILTIN_SPECIFIERS
            .iter()
            .filter_map(|(name, _)| {
                (!name.starts_with("node:") && !name.starts_with("internal/")).then_some(*name)
            })
            .collect::<std::collections::BTreeSet<_>>();
        let builtin_ids = serde_json::to_string(&builtin_ids)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let source = format!(
            r#"((nativeUtil) => {{
  const ids = new Set({builtin_ids});
  class BuiltinModule {{
    static map = new Map(Array.from(ids, (id) => [id, new BuiltinModule(id)]));
    constructor(id) {{ this.id = id; this.filename = `${{id}}.js`; this.exports = {{}}; this.loaded = false; this.loading = false; }}
    static exists(id) {{ return this.map.has(id); }}
    static canBeRequiredByUsers(id) {{ return ids.has(id); }}
    static canBeRequiredWithoutScheme(id) {{ return ids.has(id); }}
    static normalizeRequirableId(id) {{ const name = String(id).replace(/^node:/, ""); return ids.has(name) ? name : undefined; }}
    static isBuiltin(id) {{ return this.normalizeRequirableId(id) !== undefined; }}
    static getAllBuiltinModuleIds() {{ return Array.from(ids); }}
    static allowRequireByUsers(id) {{ ids.add(id); this.map.set(id, new BuiltinModule(id)); }}
    static exposeInternals() {{}}
    compileForInternalLoader() {{ return this.exports; }}
    compileForPublicLoader() {{ return this.exports; }}
  }}
  const internalBinding = (name) => {{
    if (name === "constants") return {{
      fs: Object.assign({{ UV_DIRENT_UNKNOWN: 0, UV_DIRENT_FILE: 1, UV_DIRENT_DIR: 2, UV_DIRENT_LINK: 3, UV_DIRENT_FIFO: 4, UV_DIRENT_SOCKET: 5, UV_DIRENT_CHAR: 6, UV_DIRENT_BLOCK: 7 }}, globalThis["\0quench:require"]("fs").constants),
      os: {{ errno: {{ EISDIR: -21 }}, signals: {{}} }}
    }};
    if (name === "types") return {{ isNativeError: (value) => value instanceof Error, isPromise: (value) => value instanceof Promise }};
    if (name === "string_decoder") return {{ encodings: ["ascii", "utf8", "utf-8", "utf16le", "ucs2", "ucs-2", "base64", "base64url", "latin1", "binary", "hex"] }};
    if (name === "util") return nativeUtil;
    return {{}};
  }};
  return {{ BuiltinModule, internalBinding, require: () => {{ throw new Error("builtin source loading is unavailable"); }} }};
}})"#
        );
        let factory = context.evaluate_script_rooted(&source, "internal/bootstrap/realm.js")?;
        let util = crate::modules::util_shared_vm::key_binding(context)?;
        let undefined = context.undefined();
        return context.call_rooted(factory, undefined, &[util]);
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
        Some(BuiltinModule::Dgram) => { return node_library_module(context, "dgram.js", "dgram.js"); }
        Some(BuiltinModule::InternalDgram) => {return node_library_module(context,"internal/dgram.js","internal/dgram.js");}
        Some(BuiltinModule::InternalTestBinding) => {
            // Unlike internal policy initialization, this guest-facing SDK bridge
            // must observe the current process.emitWarning and warning options.
            // Its intrinsic operations already come from the SDK primordials.
            let root = context.host_mut().node_library_root.clone();
            return load(context, &root.join("lib/internal/test/binding.js"), Some(parent), EntryGoal::CommonJs);
        }
        Some(BuiltinModule::InternalBlockList) => {
            let module = context.evaluate_script_rooted(
                "({ kHandle: Symbol.for('quench.internal.blocklist.handle') })",
                "internal/blocklist.js",
            )?;
            let net = cached_builtin(context, BuiltinModule::Net)?;
            let constructor = get(context, net, "BlockList")?;
            set(context, module, "BlockList", constructor)?;
            return Ok(module);
        }
        Some(BuiltinModule::InternalSocketAddress) => {
            let module = context.evaluate_script_rooted(
                "({ kHandle: Symbol.for('quench.internal.socketaddress.handle') })",
                "internal/socketaddress.js",
            )?;
            let net = cached_builtin(context, BuiltinModule::Net)?;
            let constructor = get(context, net, "SocketAddress")?;
            // cached_builtin may return the cache's retained root: ownership
            // remains with builtin_cache, not this projection's call frame.
            set(context, module, "SocketAddress", constructor)?;
            return Ok(module);
        }
        Some(BuiltinModule::Assert) => {
            let util = cached_builtin(context, BuiltinModule::Util)?;
            return crate::modules::assert_shared_vm::module(context, util);
        }
        Some(BuiltinModule::AssertStrict) => {
            let util = cached_builtin(context, BuiltinModule::Util)?;
            let module = crate::modules::assert_shared_vm::module(context, util)?;
            return get(context, module, "strict");
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
            return if specifier.strip_prefix("node:").unwrap_or(&specifier) == "internal/async_hooks" {
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
        Some(module @ BuiltinModule::Internal(_)) => {
            return cached_builtin(context, module);
        }
        None => {}
    }
    let filename = resolve_filename(context, &specifier, parent)?;
    let root = context.host_mut().node_library_root.clone();
    let bundled = crate::node_sources::BUNDLED_DEPENDENCIES
        .iter().any(|(_, relative)| filename == root.join(relative));
    let goal = if filename.starts_with(root.join("lib")) || bundled {
        EntryGoal::CommonJs
    } else {
        EntryGoal::Node
    };
    if crate::node_sources::source(&filename, &context.host_mut().node_library_root).is_some() {
        return load(context, &filename, Some(parent), goal);
    }
    if context.rooted_value(directory_cache).is_some_and(|value| value.is_undefined()) {
        directory_cache = context.null_object_rooted()?;
        let map_set = get(context, api, "mapSet")?;
        context.call_rooted(map_set, paths, &[parent_path, directory_cache])?;
    }
    load_with_resolution(context, &filename, Some(parent), goal, Some((directory_cache, &specifier)))
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
    context: &mut Context<'_>, receiver: RootId, args: &[RootId],
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
    Stream,
    StreamPromises,
    StreamIter,
    ZlibIter,
    WebStreams,
    StringDecoder,
    WorkerThreads,
    Util,
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
            Self::Stream => Some("stream"),
            Self::StreamPromises => Some("stream/promises"),
            Self::StreamIter => Some("stream/iter"),
            Self::ZlibIter => Some("zlib/iter"),
            Self::WebStreams => Some("stream/web"),
            Self::StringDecoder => Some("string_decoder"),
            Self::WorkerThreads => Some("worker_threads"),
            Self::Util => Some("util"),
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
    context.host_mut().shared_state().borrow().cli_options.enabled("--expose-internals")
}

fn requested_builtin(context: &mut Context<'_>, specifier: &str) -> Option<BuiltinModule> {
    let builtin = BuiltinModule::from_specifier(specifier)?;
    if matches!(builtin,BuiltinModule::StreamIter | BuiltinModule::ZlibIter) && !context.host_mut().shared_state().borrow().cli_options.enabled("--experimental-stream-iter") { return None; }
    if matches!(builtin, BuiltinModule::InternalTimers) && !internals_exposed(context) {
        None
    } else { Some(builtin) }
}

pub(crate) fn is_builtin_specifier(specifier: &str) -> bool {
    BuiltinModule::from_specifier(specifier).is_some()
}

const BUILTIN_SPECIFIERS: &[(&str, BuiltinModule)] = &[
    ("internal/process/task_queues", BuiltinModule::Internal("internal/process/task_queues")),
    ("internal/crypto/keys", BuiltinModule::Internal("internal/crypto/keys")),
    ("internal/abort_controller", BuiltinModule::Internal("internal/abort_controller")),
    ("internal/worker/io", BuiltinModule::Internal("internal/worker/io")),
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
    ("test", BuiltinModule::NodeTest),
    ("string_decoder", BuiltinModule::StringDecoder),
    ("node:string_decoder", BuiltinModule::StringDecoder),
    ("worker_threads", BuiltinModule::WorkerThreads),
    ("node:worker_threads", BuiltinModule::WorkerThreads),
    ("util", BuiltinModule::Util),
    // Node keeps the deprecated `sys` specifier as an alias of `util`.
    ("sys", BuiltinModule::Util),
    ("node:sys", BuiltinModule::Util),
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
    if matches!(builtin, BuiltinModule::Stream | BuiltinModule::StreamPromises) {
        return context.with_startup_intrinsics(|context| build_builtin(context, builtin));
    }
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
    // `fs` installs its library policy with the rest of its members (`initialize_module`).
    if matches!(builtin, BuiltinModule::Crypto | BuiltinModule::Events) {
        let policy = context.with_startup_intrinsics(|context| {
            if matches!(builtin, BuiltinModule::Events) {
                let listener = get(context, module, "addAbortListener")?;
                let data = context.array_rooted(&[module, listener])?;
                let initialize = context.host_function_with_data(
                    crate::host::shared_vm::operation("eventsOnInitialize"), data,
                )?;
                return context.define_lazy_data_properties_rooted(module, &["on"], true, initialize);
            }
            let setter = node_library_export(context, "crypto.js", "internal/crypto/util.js", "setEngine")?;
            set(context, module, "setEngine", setter)
        });
        if let Err(error) = policy {
            roots.borrow_mut().builtin_cache.remove(&key);
            context.release_root(retained);
            return Err(error);
        }
    }
    Ok(module)
}

/// One lifetime owner for native binding objects shared by configured libraries.
pub(crate) fn cached_native_binding(
    context: &mut Context<'_>,
    key: &'static str,
    build: impl FnOnce(&mut Context<'_>) -> Result<RootId, RootedError>,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(key).copied() { return Ok(value); }
    let value = build(context)?;
    let retained = context.retain(value)?;
    shared.borrow_mut().builtin_cache.insert(key.to_owned(), retained);
    Ok(value)
}

pub(crate) fn events_on_initialize(
    context: &mut Context<'_>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let module = get(context, data, "0")?;
    let listener = get(context, data, "1")?;
    // The pinned public policy uses the same native abort authority as events.
    // Capture it before guest replacement of the public alias, and load policy
    // only when `on` is observed, through the VM's ordinary data-property model.
    let abort = node_library_module(context, "events.js", "internal/events/abort_listener.js")?;
    set(context, abort, "addAbortListener", listener)?;
    let on = node_library_export(context, "events.js", "events.js", "on")?;
    set(context, module, "on", on)?;
    Ok(context.undefined())
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

pub(crate) fn node_library_export(
    context: &mut Context<'_>, owner: &str, relative: &str, name: &str,
) -> Result<RootId, RootedError> {
    let library = node_library_module(context, owner, relative)?;
    get(context, library, name)
}

/// A module of the pinned Node library, loaded on first use inside the startup intrinsics:
/// its initialization captures the original intrinsic methods however a script has replaced
/// or frozen them since (`with_startup_intrinsics` restores even nonconfigurable ones).
pub(crate) fn node_library_module(context: &mut Context<'_>, owner: &str, relative: &str) -> Result<RootId, RootedError> {
    let root = context.host_mut().node_library_root.clone();
    let parent = module_record(context, &root.join("lib").join(owner), None, false)?;
    host_load(|| {
        context.with_startup_intrinsics(|context| {
            load(context, &root.join("lib").join(relative), Some(parent), EntryGoal::CommonJs)
        })
    })
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

pub(crate) fn console_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Console)
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

pub(crate) fn internal_url_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::InternalUrl)
}

/// Private namespaces project the same original objects as the public globals.
fn native_global_namespace(context: &mut Context<'_>, names: &[&str]) -> Result<RootId, RootedError> {
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

fn build_builtin(context: &mut Context<'_>, builtin: BuiltinModule) -> Result<RootId, RootedError> {
    match builtin {
        BuiltinModule::Internal("internal/crypto/keys") => {
            let module = node_library_module(context, "crypto.js", "internal/crypto/keys.js")?;
            crate::modules::crypto_key_shared_vm::install_provider_accessors(context, module)?;
            Ok(module)
        }
        BuiltinModule::InternalVfsFd => {
            let module = node_library_module(context, "vfs.js", "internal/vfs/fd.js")?;
            let global = context.global_root()?;
            let legacy = get(context, global, "__quenchVfsFdHandles")?;
            if context.is_object_rooted(legacy)? {
                for (name, method, operation) in [("getVirtualFd", "get", "vfsDescriptorGet"), ("closeVirtualFd", "delete", "vfsDescriptorClose")] {
                    let source = get(context, module, name)?;
                    let legacy_method = get(context, legacy, method)?;
                    let data = context.array_rooted(&[source, legacy, legacy_method])?;
                    let bridge = context.host_constructor_with_data(super::operation(operation), data)?;
                    let key = context.string_rooted("name");
                    let display = context.string_rooted(name);
                    context.define_data_property_rooted(bridge, key, display, false, false, true)?;
                    set(context, module, name, bridge)?;
                }
            }
            Ok(module)
        }
        BuiltinModule::Internal("internal/worker/io") =>
            native_global_namespace(context, &["MessageChannel", "MessagePort"]),
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
            let module=native_global_namespace(context, &["AbortController", "AbortSignal"])?;
            let data=context.host_mut().shared_state().borrow().event_targets_api
                .ok_or_else(||RootedError::host("EventTarget API missing"))?;
            let operation=get(context,data,"abortSignal")?;
            set(context,module,"abortSignal",operation)?;
            let aborted = crate::modules::util_aborted_source_shared_vm::original_aborted(context)?;
            set(context,module,"aborted",aborted)?;
            Ok(module)
        },
        BuiltinModule::Internal("internal/process/task_queues") =>
            native_global_namespace(context, &["queueMicrotask"]),
        BuiltinModule::Internal("_http_incoming") => {
            let root = context.host_mut().node_library_root.clone();
            let parent = module_record(context, &root.join("lib/http.js"), None, false)?;
            load(context, &root.join("lib/_http_incoming.js"), Some(parent), EntryGoal::CommonJs)
        }
        BuiltinModule::Internal("dns/promises") => node_library_module(context, "dns.js", "dns/promises.js"),
        // Node's `_http_*` modules build on its `http_parser` binding, which the native `http`
        // module does not use; until they are projected from it they stay empty.
        BuiltinModule::Internal(
            "_http_agent" | "_http_client" | "_http_common" | "_http_outgoing" | "_http_server",
        ) => context.object_rooted(),
        // Every other such builtin is Node's own library module of that name.
        BuiltinModule::Internal(name) => {
            let relative = format!("{}.js", name.strip_prefix("node:").unwrap_or(name));
            node_library_module(context, &relative, &relative)
        }
        BuiltinModule::Fs => crate::modules::fs_shared_vm::module(context),
        BuiltinModule::InternalUrl => {
            // Node's bootstrap (exposed-window-or-worker.js) installs `URL.createObjectURL`
            // and `URL.revokeObjectURL`; the module loads on first use here, so it does then.
            let module = node_library_module(context, "url.js", "internal/url.js")?;
            let install = get(context, module, "installObjectURLMethods")?;
            let undefined = context.undefined();
            context.call_rooted(install, undefined, &[])?;
            Ok(module)
        }
        // lib/util/types.js re-exports internal/util/types, the object `util.types` names.
        BuiltinModule::UtilTypes => node_library_module(context, "util/types.js", "util/types.js"),
        BuiltinModule::FsPromises => {
            let fs = cached_builtin(context, BuiltinModule::Fs)?;
            get(context, fs, "promises")
        }
        // The pinned SDK owns VFS layer identity, reserved mount roots and lifecycle.
        // Native descriptor and fs handler operations remain registered below.
        BuiltinModule::Vfs => node_library_module(context, "vfs.js", "vfs.js"),
        BuiltinModule::Net => crate::modules::net_shared_vm::module(context),
        BuiltinModule::Http => crate::modules::http_shared_vm::module(context),
        #[cfg(target_os = "linux")]
        BuiltinModule::Os => node_library_module(context, "os.js", "os.js"),
        #[cfg(not(target_os = "linux"))]
        BuiltinModule::Os => crate::modules::os_shared_vm::module(context),
        BuiltinModule::Buffer => crate::modules::buffer_shared_vm::module(context),
        BuiltinModule::Stream => node_library_module(context, "stream.js", "stream.js"),
        BuiltinModule::StreamPromises => node_library_module(context, "stream.js", "stream/promises.js"),
        BuiltinModule::StreamIter => node_library_module(context, "stream/iter.js", "stream/iter.js"),
        BuiltinModule::ZlibIter => node_library_module(context, "zlib/iter.js", "zlib/iter.js"),
        BuiltinModule::WebStreams => node_library_module(context, "stream/web.js", "stream/web.js"),
        BuiltinModule::StringDecoder => node_library_module(context, "string_decoder.js", "string_decoder.js"),
        BuiltinModule::Timers => {
            let promises = cached_builtin(context, BuiltinModule::TimersPromises)?;
            crate::modules::timers_shared_vm::timers_module(context, promises)
        }
        BuiltinModule::TimersPromises => crate::modules::timers_shared_vm::promises_module(context),
        BuiltinModule::InternalTimers => crate::modules::timers_shared_vm::internal_module(context),
        BuiltinModule::NodeTest => crate::modules::test_shared_vm::module(context),
        BuiltinModule::Vm => crate::modules::vm_shared_vm::module(context),
        BuiltinModule::WorkerThreads => {
            let mut markers = Vec::new();
            for name in ["markAsUncloneable", "markAsUntransferable", "isMarkedAsUntransferable"] {
                let operation = context.host_function(super::operation(name))?;
                let factory = context.evaluate_script_rooted(
                    &format!("(operation)=>function {name}(object){{return operation(object)}}"),
                    "node:worker_threads/marker.js",
                )?;
                let undefined = context.undefined();
                markers.push((name, context.call_rooted(factory, undefined, &[operation])?));
            }
            crate::modules::worker_shared_vm::module(context, &markers)
        }
        BuiltinModule::Url => node_library_module(context, "url.js", "url.js"),
        BuiltinModule::Querystring => node_library_module(context, "querystring.js", "querystring.js"),
        BuiltinModule::Readline => crate::modules::readline_shared_vm::module(context),
        BuiltinModule::StreamConsumers => {
            crate::modules::web_stream_consumers_shared_vm::module(context)
        }
        BuiltinModule::Events => crate::modules::events_shared_vm::module(context),
        BuiltinModule::Console => crate::modules::console_shared_vm::module(context),
        BuiltinModule::Tty => crate::modules::tty_shared_vm::module(context),
        BuiltinModule::Dns => {
            let utils = node_library_module(context, "dns.js", "internal/dns/utils.js")?;
            let initialize = get(context, utils, "initializeDns")?;
            let undefined = context.undefined();
            context.call_rooted(initialize, undefined, &[])?;
            node_library_module(context, "dns.js", "dns.js")
        }
        BuiltinModule::Dgram => {
            let dgram_tail = crate::polyfills::bootstrap::dgram_tail::JS
                .split("globalThis.require = (specifier) =>")
                .next()
                .unwrap_or(crate::polyfills::bootstrap::dgram_tail::JS);
            let source = format!(
                "(__quenchDgramBufferSystemError => {{\n{}\n{}\n{}\n{}\n}})",
                crate::polyfills::bootstrap::dgram_head::JS,
                crate::polyfills::bootstrap::dgram::JS,
                crate::polyfills::bootstrap::membership::JS,
                dgram_tail,
            );
            let root = context.evaluate_script_rooted(&source, "node:dgram/bootstrap.js")?;
            let codes = node_library_export(context, "internal/errors.js", "internal/errors.js", "codes")?;
            let constructor = get(context, codes, "ERR_SOCKET_BUFFER_SIZE")?;
            let undefined = context.undefined();
            context.call_rooted(root, undefined, &[constructor])?;
            context.release_root(root);
            let global = context.global_root()?;
            get(context, global, "\0quench:dgram_module")
        }
        BuiltinModule::PerfHooks => crate::modules::perf_hooks::module(context),
        BuiltinModule::Zlib => node_library_module(context, "zlib.js", "zlib.js"),
        BuiltinModule::Crypto => {
            let stream = cached_builtin(context, BuiltinModule::Stream)?;
            let transform_key = context.string_rooted("Transform");
            let transform = context.get_property_rooted(stream, transform_key)?;
            crate::modules::crypto_shared_vm::module(context, transform)
        }
        BuiltinModule::Domain => crate::modules::domain_shared_vm::module(context),
        BuiltinModule::Tls => crate::modules::tls_shared_vm::module(context),
        BuiltinModule::V8 => crate::modules::v8_shared_vm::module(context),
        BuiltinModule::Module => crate::modules::module_shared_vm::module(context),
        BuiltinModule::AsyncHooks | BuiltinModule::DiagnosticsChannel => Err(RootedError::host(
            "stateful builtin passed to generic shared module builder",
        )),
        BuiltinModule::Util => crate::modules::util_shared_vm::module(context),
        BuiltinModule::ChildProcess => crate::modules::child_process_shared_vm::module(context),
        BuiltinModule::InternalChildProcess => {
            let public = cached_builtin(context, BuiltinModule::ChildProcess)?;
            let module = node_library_module(context, "child_process.js", "internal/child_process.js")?;
            let constructor = get(context, public, "ChildProcess")?;
            set(context, module, "ChildProcess", constructor)?;
            Ok(module)
        }
        BuiltinModule::Https => {
            let http = cached_builtin(context, BuiltinModule::Http)?;
            crate::modules::http_shared_vm::https_module(context, http)
        }
        BuiltinModule::TraceEvents => crate::modules::trace_shared_vm::module(context),
        BuiltinModule::DnsPromises => { let dns = dns_module(context)?; get(context, dns, "promises") },
        BuiltinModule::CaresBinding => crate::modules::cares_binding::module(context),
        BuiltinModule::Http2Binding => {
            let binding = context.object_rooted()?;
            // Native constant values and attributes from Node v26.12.0's HTTP/2
            // binding (src/node_http2.cc), shared by public exports and internals.
            let descriptors = context.parse_json_rooted(include_str!("../../modules/http2_constants.json"))?;
            let constants = context.object_rooted()?;
            for key in context.own_keys_rooted(descriptors)? {
                let descriptor = context.get_property_rooted(descriptors, key)?;
                context.define_property_descriptor_rooted(constants, key, descriptor)?;
            }
            set(context, binding, "constants", constants)?;
            Ok(binding)
        }
        BuiltinModule::Http2 => {
            let path = context.host_mut().node_library_root.join("lib/internal/http2/compat.js");
            let parent_path = context.host_mut().node_library_root.join("lib/http2.js");
            let parent = module_record(context, &parent_path, None, false)?;
            let compat = load(context, &path, Some(parent), EntryGoal::CommonJs)?;
            let module = context.object_rooted()?;
            for name in ["Http2ServerRequest", "Http2ServerResponse"] {
                let value = get(context, compat, name)?;
                set(context, module, name, value)?;
            }
            let binding = http2_binding(context)?;
            let constants = get(context, binding, "constants")?;
            set(context, module, "constants", constants)?;
            Ok(module)
        }
        BuiltinModule::Cluster => node_library_module(context, "cluster.js", "cluster.js"),
        BuiltinModule::Inspector
        | BuiltinModule::Repl
        | BuiltinModule::Wasi => context.object_rooted(),
        BuiltinModule::Sea => context.evaluate_script_rooted(
            "({ isSea: function isSea() { return false; } })",
            "node:sea.js",
        ),
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
    let directory = match context.string_text(filename)? {
        Some(filename) => Path::new(&filename).parent().unwrap_or(Path::new(".")).to_owned(),
        None => {
            let directory = get(context, parent, "path")?;
            let directory = context.string_text(directory)?
                .ok_or_else(|| RootedError::host("invalid parent module path"))?;
            PathBuf::from(directory)
        }
    };
    let node_lib = context.host_mut().node_library_root.join("lib");
    let internal_specifier = specifier.strip_prefix("node:").unwrap_or(specifier);
    let node_internal = (internal_specifier.starts_with("internal/")
        && (internal_specifier != "internal/timers" || internals_exposed(context)))
        || (internal_specifier.starts_with('_')
            && node_lib
                .join(internal_specifier)
                .with_extension("js")
                .is_file());
    if node_internal {
        if let Some((_, relative)) = crate::node_sources::BUNDLED_DEPENDENCIES
            .iter().find(|(name, _)| *name == internal_specifier)
        {
            let root = context.host_mut().node_library_root.clone();
            let path = root.join(relative);
            if path.is_file() || crate::node_sources::source(&path, &root).is_some() {
                return Ok(path);
            }
        }
    }
    let virtual_path = if node_internal {
        Some(node_lib.join(internal_specifier).with_extension("js"))
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        Some(directory.join(specifier).with_extension("js"))
    } else {
        None
    };
    if let Some(path) = virtual_path {
        if !path.is_file() {
            if let Some((canonical, _)) =
                crate::node_sources::resolve(&path, &context.host_mut().node_library_root)
            {
                return Ok(canonical);
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
    let (outcome, missing_package) = with_commonjs_resolver(|resolver| match resolve_kept(resolver, resolve_in_bases) {
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

/// Captured once by bootstrap's first module, before preloads or guest code.
/// The weak keys preserve Node's private first-parent relation independently of
/// the mutable public module.parent projection.
fn module_parent_api(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(api) = context.host_mut().shared_state().borrow().module_parent_api { return Ok(api); }
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
    context.host_mut().shared_state().borrow_mut().module_parent_api = Some(api);
    Ok(api)
}

fn delete_relative_resolution(context: &mut Context<'_>, cache: RootId, specifier: &str) -> Result<(), RootedError> {
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

pub(crate) fn attach_module_cache(context: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    use crate::host::node_host::CommonJsCache;
    let cache = commonjs_cache(context)?;
    set(context, module, "_cache", cache)?;
    let owner = context.retain(module)?;
    let previous = context.host_mut().shared_state().borrow_mut().commonjs_cache.replace(CommonJsCache::Module(owner));
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
            Self::NodeLibrary => Ok(context.host_mut().shared_state().borrow().builtin_cache.get(name).copied().unwrap_or_else(|| context.undefined())),
            Self::CommonJs => {
                let cache = commonjs_cache(context)?;
                get(context, cache, name)
            }
        }
    }

    fn insert(self, context: &mut Context<'_>, name: &str, module: RootId) -> Result<(), RootedError> {
        match self {
            Self::NodeLibrary => {
                let root = context.retain(module)?;
                let old = context.host_mut().shared_state().borrow_mut().builtin_cache.insert(name.to_owned(), root);
                if let Some(old) = old { context.release_root(old); }
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
                let old = context.host_mut().shared_state().borrow_mut().builtin_cache.remove(name);
                if let Some(old) = old { context.release_root(old); }
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
                    let error = context.type_error_rooted(&format!("Cannot delete property '{name}' of [object Object]"))?;
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
        let name = if context.truthy_rooted(filename)? { filename } else { get(context, cursor, "id")? };
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
    let parent = parent.unwrap_or_else(|| context.undefined());
    set(context, module, "parent", parent)?;
    let api = module_parent_api(context)?;
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
            .is_some_and(|value| value.is_undefined())
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

fn require_module_error(context: &mut Context<'_>, code: &'static str, message: &str) -> Result<RootedError, RootedError> {
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
    let module = match quench_runtime::Host::resolve_dynamic_import(context.host_mut(), &filename.to_string_lossy(), url.as_str()) {
        Ok(module) => module,
        Err(error) => return Err(throw_module_load_error(context, error)?),
    }.ok_or_else(|| RootedError::host("ES module source unavailable"))?;
    let namespace = match context.evaluate_module_sync_rooted(&module)? {
        quench_runtime::SynchronousModule::Namespace(namespace) => namespace,
        quench_runtime::SynchronousModule::AsyncGraph => {
            let mut units = format!("require() cannot be used on an ESM graph with top-level await. Use import() instead. To see where the top-level await comes from, use --experimental-print-required-tla.\nRequired module: {}", filename.display()).encode_utf16().collect::<Vec<_>>();
            let stack = if let Some(parent) = parent { Some(require_stack(context, parent)?) } else { None };
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
            crate::modules::node_errors_shared_vm::decorate_error(context, error, "ERR_REQUIRE_ASYNC_MODULE")?;
            if let Some(stack) = stack {
                let key = context.string_rooted("requireStack");
                context.define_data_property_rooted(error, key, stack, true, false, true)?;
            }
            return Err(context.throw(error));
        }
        quench_runtime::SynchronousModule::Evaluating => {
            let mut message = format!("Cannot require() ES Module {} in a cycle.", filename.display());
            if let Some(parent) = parent {
                let name = get(context, parent, "filename")?;
                message.push_str(&format!(" (from {})", context.to_string(name)?));
            }
            message.push_str(" A cycle involving require(esm) is not allowed to maintain invariants mandated by the ECMAScript specification. Try making at least part of the dependency in the graph lazily loaded.");
            return Err(require_module_error(context, "ERR_REQUIRE_CYCLE_MODULE", &message)?);
        }
    };
    let key = context.string_rooted("module.exports");
    if context.has_property_rooted(namespace, key)? {
        return context.get_property_rooted(namespace, key);
    }
    let default = context.string_rooted("default");
    let marker = context.string_rooted("__esModule");
    if context.has_property_rooted(namespace, default)? && !context.has_property_rooted(namespace, marker)? {
        let yes = context.boolean(true);
        context.module_namespace_with_export_rooted(namespace, "__esModule", yes)
    } else { Ok(namespace) }

}

/// Whether the pinned Node library is available (its primordials module stands for it).
pub(crate) fn node_library_present(context: &mut Context<'_>) -> bool {
    node_primordials_source(context).is_some()
}

/// The pinned Node library's primordials module, when the library is present.
fn node_primordials_source(context: &mut Context<'_>) -> Option<PathBuf> {
    let root = context.host_mut().node_library_root.clone();
    let path = root.join("lib/internal/per_context/primordials.js");
    (path.is_file() || crate::node_sources::source(&path, &root).is_some()).then_some(path)
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
        return Err(require_module_error(context, "ERR_REQUIRE_CYCLE_MODULE", &message)?);
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
    let cache = if crate::node_sources::source(filename, &context.host_mut().node_library_root).is_some() {
        ModuleCache::NodeLibrary
    } else { ModuleCache::CommonJs };
    let cached = cache.get(context, &key)?;
    if !context.rooted_value(cached).is_some_and(|value| value.is_undefined()) {
        if let Some(parent) = parent {
            transition_child(context, parent, cached, ChildTransition::Attach)?;
        }
        return get(context, cached, "exports");
    }
    // A file whose goal its text decides compiles as CommonJS first, as Node's loader does:
    // only a wrapper that does not compile asks the text for ES module syntax, so a CommonJS
    // file is parsed once, not again for detection (typescript.js: a 9 MB text's tree and
    // tables, the peak of loading it).
    let declared = match goal {
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
        if filename.extension().and_then(|extension| extension.to_str()) == Some("node") {
            // Module._extensions['.node']: process.dlopen(module, filename).
            let path = context.string_rooted(&filename.to_string_lossy());
            crate::modules::napi::dlopen(context, module, &[module, path])?;
            let exports = get(context, module, "exports")?;
            let loaded = context.boolean(true);
            set(context, module, "loaded", loaded)?;
            return Ok(exports);
        }
        let file_text: std::borrow::Cow<'static, str> = match std::fs::read(filename) {
            Ok(bytes) => String::from_utf8(bytes)
                .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
                .into(),
            Err(error) => crate::node_sources::source(filename, &context.host_mut().node_library_root)
                .ok_or_else(|| RootedError::host(error.to_string()))?
                .into(),
        };
        let file_text: std::borrow::Cow<'static, str> = match library_relative {
            Some(path) if path == Path::new("lib/internal/child_process.js") =>
                super::policy_projection::project(&file_text, &["setupChannel", "kChannelHandle"]).map_err(RootedError::host)?.into(),
            Some(path) if path == Path::new("lib/net.js") =>
                super::policy_projection::project(&file_text, &["BoundSocket"]).map_err(RootedError::host)?.into(),
            Some(path) if path == Path::new("lib/child_process.js") =>
                super::policy_projection::project(&file_text, &["_forkChild"]).map_err(RootedError::host)?.into(),
            Some(path) if path == Path::new("lib/internal/buffer.js") =>
                crate::modules::buffer_shared_vm::project_internal_buffer(&file_text)?.into(),
            Some(path) if path == Path::new("lib/buffer.js") =>
                crate::modules::buffer_shared_vm::project_buffer(&file_text)?.into(),
            Some(_) if is_primordials =>
                crate::modules::primordials_shared_vm::project_primordials(&file_text)?.into(),
            _ => file_text,
        };
        // An embedded library file is static text the compiled program can retain as is;
        // a projected one is new text.
        let embedded = match file_text {
            std::borrow::Cow::Borrowed(text) => Some(script_body(text)),
            std::borrow::Cow::Owned(_) => None,
        };
        let text = file_text.strip_prefix('\u{feff}').unwrap_or(&file_text);
        match filename
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("json") => {
                let exports = context.parse_json_rooted(text)?;
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
                        super::operation("nodeLibraryRequire"), "require", module,
                    )?;
                    for name in ["resolve", "main", "cache"] {
                        let value = get(context, public_require, name)?;
                        set(context, require, name, value)?;
                    }
                    require
                } else { public_require };
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
                let is_realm_bootstrap = library_relative
                    == Some(Path::new("lib/internal/bootstrap/realm.js"));
                let internal_binding = if is_realm_bootstrap {
                    r#"const getInternalBinding = (name) => {
  if (name === "builtins") return { builtinIds: [], compileFunction() {}, setInternalLoaders() {} };
  if (name === "module_wrap") return { ModuleWrap: class ModuleWrap {} };
  return {};
};
const getLinkedBinding = () => ({});
"#
                } else if library_relative == Some(Path::new("lib/internal/test/binding.js")) {
                    // This configured SDK entry exposes the engine's raw binding
                    // objects; it must not expose per-module library adapters.
                    "const internalBinding = __nativeInternalBinding;"
                } else if library_relative == Some(Path::new("lib/internal/per_context/domexception.js")) {
                    // This SDK per-context source has two explicit native symbol
                    // capabilities in Node's wrapper. Reuse existing symbol owners.
                    "const privateSymbols=__nativeInternalBinding('util').privateSymbols; const perIsolateSymbols=__nativeInternalBinding('symbols');"
                } else if in_node_internal {
                    node_internal_binding_source()
                } else {
                    ""
                };
                let prefix = if in_node_internal {
                    format!("(function (exports, require, module, __filename, __dirname, primordials, __nativeInternalBinding) {{ 'use strict'; {internal_binding}")
                } else {
                    format!("{WRAPPER_PREFIX}{internal_binding}")
                };
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
                        let wrapper = match evaluate_wrapper(context, &prefix, &wrapper_source, &key, is_main) {
                            Ok(wrapper) => wrapper,
                            Err(_) if declared.is_none()
                                && detect_source_kind(&wrapper_source[prefix.len()..wrapper_source.len() - suffix.len()])
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
            let debug = initial.map(|value| context.string_rooted(&value)).unwrap_or_else(|| context.undefined());
            let absent = context.undefined();
            context.call_rooted(initialize, absent, &[debug])?;
        }
        if library_relative == Some(Path::new("lib/internal/util/inspect.js")) {
            crate::modules::console_shared_vm::note_inspect_loaded(context)?;
        }
        if library_relative == Some(Path::new("lib/diagnostics_channel.js")) {
            crate::modules::console_shared_vm::note_channels_loaded(context)?;
        }
        if library_relative == Some(Path::new("lib/internal/console/global.js")) {
            // Node's bootstrap initializes the global console as it loads it; one loaded on
            // first use binds its streams before any module writes to it.
            let initialize = node_library_export(
                context,
                "internal/process/pre_execution.js",
                "internal/console/constructor.js",
                "initializeGlobalConsole",
            )?;
            let undefined = context.undefined();
            context.call_rooted(initialize, undefined, &[exports])?;
            crate::modules::console_shared_vm::adopt(context, module, exports)?;
        }
        let exports = get(context, module, "exports")?;
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
        if let Ok(exports)=&result { crate::modules::events_shared_vm::adopt_abort_converter(context,*exports)?; }
    }
    let is_fs_utilities = library_relative == Some(Path::new("lib/internal/fs/utils.js"));
    if is_fs_utilities {
        if let Ok(exports) = &result {
            crate::modules::fs_policy_shared_vm::adopt(context, *exports)?;
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

/// A CommonJS module's executed text: its file without a byte-order mark or a leading
/// `#!` line, which keeps the line terminator so positions stay on their lines.
fn script_body(text: &str) -> &str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.starts_with("#!") {
        text.find('\n').map_or("", |end| &text[end..])
    } else {
        text
    }
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
) -> Result<RootId, RootedError> {
    let (line_offset, column_offset) = quench_runtime::wrapper_origin(prefix)
        .ok_or_else(|| RootedError::host("CommonJS wrapper origin exceeds compiler positions"))?;
    if entry {
        context.evaluate_entry_script_with_origin_rooted(source, filename, line_offset, column_offset)
    } else {
        context.evaluate_specialized_script_with_origin_rooted(source, filename, line_offset, column_offset)
    }
}

pub(crate) fn compile_public(context: &mut Context<'_>, module: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let content = args.first().copied().unwrap_or_else(|| context.undefined());
    let filename = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let content = context.string_text(content)?.ok_or_else(|| RootedError::host("Module._compile content must be a string"))?;
    let name = context.string_text(filename)?.ok_or_else(|| RootedError::host("Module._compile filename must be a string"))?;
    let text = content.as_str();
    let text = if text.starts_with("#!") { text.find('\n').map_or("", |end| &text[end..]) } else { text };
    let source = [WRAPPER_PREFIX, text, WRAPPER_SUFFIX].concat();
    let wrapper = evaluate_wrapper(context, WRAPPER_PREFIX, &source, &name, false)?;
    let exports = get(context, module, "exports")?;
    let require = context.host_function_with_data(super::operation("require"), module)?;
    install_resolve(context, require, module)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let main = get(context, process, "mainModule")?;
    set(context, require, "main", main)?;
    let directory = crate::modules::path_shared_vm::host_dirname(context, filename)?;
    context.call_rooted(wrapper, exports, &[exports, require, module, filename, directory])?;
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

pub(crate) fn node_internal_binding_source() -> &'static str {
    r#"const internalBinding = (name) => {
  switch (String(name)) {
    case "pipe_wrap":
    case "tcp_wrap":
    case "stream_wrap":
    case "udp_wrap":
    case "os":
    case "timers":
    case "contextify":
    case "credentials":
    case "config":
    case "icu":
    case "encoding_binding":
    case "zlib":
    case "symbols":
    case "cares_wrap":
    case "http2":
    case "fs_event_wrap":
    case "errors":
    case "modules":
    case "report": return __nativeInternalBinding(name);
    case "process_methods": return __nativeInternalBinding(name);
    case "trace_events": return __nativeInternalBinding(name);
    case "util": return Object.assign({}, {
      privateSymbols: __nativeInternalBinding("util").privateSymbols,
      constants: __nativeInternalBinding("util").constants,
      getOwnNonIndexProperties: __nativeInternalBinding("util").getOwnNonIndexProperties,
      getPromiseDetails: __nativeInternalBinding("util").getPromiseDetails,
      markPromiseAsHandled: __nativeInternalBinding("util").markPromiseAsHandled,
      getProxyDetails: __nativeInternalBinding("util").getProxyDetails,
      previewEntries: __nativeInternalBinding("util").previewEntries,
      getConstructorName: __nativeInternalBinding("util").getConstructorName,
      getCallSites: __nativeInternalBinding("util").getCallSites,
      getCallerLocation: __nativeInternalBinding("util").getCallerLocation,
      getExternalValue: __nativeInternalBinding("util").getExternalValue,
      constructSharedArrayBuffer: (length) => new SharedArrayBuffer(length),
      guessHandleType: __nativeInternalBinding("util").guessHandleType,
      defineLazyProperties: __nativeInternalBinding("util").defineLazyProperties,
      isInsideNodeModules: __nativeInternalBinding("util").isInsideNodeModules,
      sleep: () => undefined,
    });
    case "types": return __nativeInternalBinding("types");
    case "constants": return {
      fs: __nativeInternalBinding("constants").fs,
      zlib: __nativeInternalBinding("constants").zlib,
      os: __nativeInternalBinding("constants").os || { signals: (require("os").constants || {}).signals || {}, errno: { EISDIR: -21 } },
      crypto: __nativeInternalBinding("constants").crypto,
    };
    case "options": return __nativeInternalBinding("options");
    case "string_decoder": return __nativeInternalBinding("string_decoder");
    case "buffer":
    case "url":
    case "blob":
    case "ipc_serdes": return __nativeInternalBinding(name);
    case "fs": return __nativeInternalBinding("fs");
    case "crypto": {
      const native = __nativeInternalBinding("crypto");
      if (native.getFipsCryptoGeneration) return native;
      const crypto = require("crypto");
      return Object.assign({}, crypto, __nativeInternalBinding("crypto_keys"), {
        setEngine: __nativeInternalBinding("crypto").setEngine,
        getFipsCrypto: () => crypto.getFips(),
        getCachedAliases: () => ({}),
        getOpenSSLSecLevelCrypto: () => 0,
        secureHeapUsed: () => undefined,
      });
    }
    case "mksnapshot": return {
      isBuildingSnapshotBuffer: new Uint8Array(1),
      setSerializeCallback: () => {},
      setDeserializeCallback: () => {},
      setDeserializeMainFunction: () => {},
    };
    case "process": return process;
    case "messaging": {
      if (__nativeInternalBinding("crypto").getFipsCryptoGeneration) return __nativeInternalBinding("messaging");
      return { DOMException: globalThis.DOMException };
    }
    case "performance": return __nativeInternalBinding("performance");
    case "uv": return __nativeInternalBinding("uv");
    case "cares_wrap":
    case "serdes":
    case "v8":
    case "heap_utils":
    case "internal_only_v8": return __nativeInternalBinding(name);
    default: return {};
  }
};
"#
}

/// Execute an unchanged configured Node library module in a fresh language realm.
/// The private CJS environment supplies host services without exposing globals.
/// Immutable code can be shared while each realm executes a fresh module body.
pub(crate) struct PreparedRealmModule {
    source: std::rc::Rc<str>,
    script: quench_runtime::PreparedScript,
}

pub(crate) fn node_library_module_in_realm(
    context: &mut Context<'_>,
    global: RootId,
    relative: &str,
    exports: RootId,
    primordials: RootId,
) -> Result<RootId, RootedError> {
    let root = context.host_mut().node_library_root.clone();
    let path = root.join("lib").join(relative);
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => crate::node_sources::source(&path, &root)
            .map(str::to_owned)
            .ok_or_else(|| RootedError::host(format!("{}: {error}", path.display())))?,
    };
    let module = context.object_rooted()?;
    set(context, module, "exports", exports)?;
    let require = create_require(context, &path)?;
    let name = context.string_rooted(&path.to_string_lossy());
    let directory = context.string_rooted(&path.parent().unwrap().to_string_lossy());
    let prefix = format!(
        "(function(exports, require, module, __filename, __dirname, primordials, __nativeInternalBinding, process) {{ 'use strict'; {}",
        node_internal_binding_source(),
    );
    let wrapper = format!("{prefix}{source}\n}})");
    let (line_offset, column_offset) = quench_runtime::wrapper_origin(prefix.as_str())
        .ok_or_else(|| RootedError::host("realm wrapper origin exceeds compiler positions"))?;
    let shared = context.host_mut().shared_state();
    let cached = shared.borrow().realm_module_programs.get(&path)
        .filter(|cached| cached.source.as_ref() == wrapper)
        .map(|cached| cached.script.clone());
    let script = match cached.filter(|script| context.prepared_script_is_current(script)) {
        Some(script) => script,
        None => {
            let script = context.prepare_specialized_script_in_realm_with_origin(
                global, &wrapper, &path.to_string_lossy(), line_offset, column_offset,
            )?;
            shared.borrow_mut().realm_module_programs.insert(path.clone(), PreparedRealmModule {
                source: std::rc::Rc::from(wrapper), script: script.clone(),
            });
            script
        }
    };
    let factory = context.evaluate_prepared_script_in_realm_rooted(global, &script)?;
    let native_binding =
        context.host_function(crate::host::shared_vm::operation("traceInternal"))?;
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("Node process is not initialized"))?;
    context.call_rooted(
        factory,
        exports,
        &[
            exports,
            require,
            module,
            name,
            directory,
            primordials,
            native_binding,
            process,
        ],
    )?;
    get(context, module, "exports")
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
