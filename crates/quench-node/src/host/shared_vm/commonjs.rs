//! CommonJS host policy. Guest code, objects and calls belong to the shared VM.

use crate::host::{EntryGoal, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};

type Context<'a> = NativeContext<'a, NodeHost>;

// Node's public CommonJS wrapper; this is guest compilation input, not an API shim.
const WRAPPER_PREFIX: &str = "(function (exports, require, module, __filename, __dirname) { ";
const WRAPPER_SUFFIX: &str = "\n});";

pub(super) fn commonjs_resolver(addons_enabled: bool) -> oxc_resolver::Resolver {
    let mut condition_names = vec!["node".into()];
    if addons_enabled {
        condition_names.push("node-addons".into());
    }
    condition_names.extend(["require".into(), "default".into()]);
    oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        extensions: vec![".js".into(), ".json".into(), ".node".into()],
        main_files: vec!["index".into()],
        condition_names,
        ..Default::default()
    })
}

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
        return std::fs::canonicalize(path).map_err(|error| error.to_string());
    }
    commonjs_resolver(addons_enabled)
        .resolve(&cwd, &path.to_string_lossy())
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
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("mjs") => return Ok(quench_runtime::SourceKind::Module),
        Some("cjs") => return Ok(quench_runtime::SourceKind::Script),
        Some("json" | "node") => return Ok(quench_runtime::SourceKind::Script),
        Some(extension) if extension != "js" => return detect_source_kind(path),
        _ => {}
    }
    let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
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
        let text = match std::fs::read_to_string(&package) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{}: {error}", package.display())),
        };
        let package_json: serde_json::Value =
            serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text))
                .map_err(|_| format!("Invalid package config {}.", package.display()))?;
        let explicit = match package_json.get("type") {
            Some(serde_json::Value::String(kind)) if kind == "module" => {
                Some(quench_runtime::SourceKind::Module)
            }
            Some(serde_json::Value::String(kind)) if kind == "commonjs" => {
                Some(quench_runtime::SourceKind::Script)
            }
            Some(serde_json::Value::String(_)) | None => None,
            Some(_) => {
                return Err(format!(
                    "Invalid package config {}. \"type\" key must be a string",
                    package.display()
                ));
            }
        };
        return match explicit {
            Some(kind) => Ok(kind),
            None => detect_source_kind(&path),
        };
    }
    detect_source_kind(&path)
}

/// Node first accepts the CommonJS wrapper. Only syntax that cannot live in
/// that wrapper triggers ESM detection, including top-level await and lexical
/// declarations of wrapper parameters. OXC owns both grammar and early errors.
fn detect_source_kind(path: &Path) -> Result<quench_runtime::SourceKind, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let text = if text.starts_with("#!") {
        text.find('\n').map_or("", |newline| &text[newline..])
    } else {
        text
    };
    let wrapper = format!("{WRAPPER_PREFIX}{text}{WRAPPER_SUFFIX}");
    if valid_source(&wrapper, oxc_span::SourceType::cjs()) {
        return Ok(quench_runtime::SourceKind::Script);
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::unambiguous()).parse();
    if parsed.program.source_type.is_module() || valid_source(text, oxc_span::SourceType::mjs()) {
        Ok(quench_runtime::SourceKind::Module)
    } else {
        Ok(quench_runtime::SourceKind::Script)
    }
}

fn valid_source(source: &str, source_type: oxc_span::SourceType) -> bool {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    parsed.diagnostics.is_empty()
        && oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program)
            .diagnostics
            .is_empty()
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
    context.scoped(crate::modules::url_shared_vm::install_global)?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_url_global");
    crate::host::resident_pages::release_read_only_pages();
    context.scoped(install_shared_web_globals)?;
    context.scoped(crate::modules::base64_shared_vm::install)?;
    let global = context.global_root()?;
    // The process.getBuiltinModule bootstrap needs access to the host's
    // builtin loader in both Script and Module goals. Keep it under a private
    // global key so ESM does not acquire a user-visible global `require`.
    let bootstrap_filename =
        inline_directory(context)?.join(context.host_mut().inline_source.label());
    let bootstrap_require = create_require(context, &bootstrap_filename)?;
    set(context, global, "\0quench:require", bootstrap_require)?;
    context.release_root(bootstrap_require);
    // Keep the host scheduler separate from Node's public queueMicrotask API.
    // Node's task_queues wrapper adds the AsyncResource lifecycle around each
    // callback, while internalBinding('task_queue') must remain a raw queue.
    let raw_queue_microtask = get(context, global, "queueMicrotask")?;
    let raw_queue_key = context.string_rooted("\0quench:queueMicrotaskRaw");
    context.define_data_property_rooted(
        global,
        raw_queue_key,
        raw_queue_microtask,
        false,
        false,
        true,
    )?;
    let require = get(context, global, "\0quench:require")?;
    let task_queues_name = context.string_rooted("internal/process/task_queues");
    let undefined = context.undefined();
    let task_queues = context.call_rooted(require, undefined, &[task_queues_name])?;
    let queue_microtask = get(context, task_queues, "queueMicrotask")?;
    set(context, global, "queueMicrotask", queue_microtask)?;
    let print_eval =
        context.host_function(crate::host::shared_vm::operation("nodePrintEvalScript"))?;
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
    // The SDK console reads its primordial snapshot, so initialize it after the
    // well-known symbols and original globals above, before any guest preloads.
    let console = cached_builtin(context, BuiltinModule::Console)?;
    set(context, global, "console", console)?;
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
            context.scoped(crate::modules::buffer_policy_shared_vm::initialize)?;
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
    let install_node_global_descriptors = context.evaluate_script_rooted(
        "() => {\n\
          for (const name of ['queueMicrotask', 'clearImmediate', 'clearInterval', 'clearTimeout', 'setImmediate', 'setInterval', 'setTimeout', 'structuredClone', 'fetch', 'crypto', 'performance']) {\n\
            const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);\n\
            if (descriptor && !descriptor.enumerable) Object.defineProperty(globalThis, name, { ...descriptor, enumerable: true });\n\
          }\n\
          if (!Object.getOwnPropertyDescriptor(globalThis, 'navigator')) {\n\
            const navigator = { userAgent: `Node.js/${process.versions.node.split('.')[0]}` };\n\
            Object.defineProperty(globalThis, 'navigator', { configurable: true, enumerable: true, get: Object.defineProperty(() => navigator, 'name', { value: 'get navigator' }) });\n\
          }\n\
          if (!Object.hasOwn(globalThis, Symbol.toStringTag)) {\n\
            Object.defineProperty(globalThis, Symbol.toStringTag, { value: 'global', configurable: true });\n\
          }\n\
        }",
        "node:bootstrap/global-descriptors.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install_node_global_descriptors, undefined, &[])?;
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
    let require = get(context, global, "\0quench:require")?;
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
    let install = context.evaluate_script_rooted(
        "(path, stream) => Object.defineProperties(globalThis, { __nodePath: { configurable: true, value: path }, __nodeStream: { configurable: true, value: stream }, __nodeFdPaths: { configurable: true, value: {} }, __quenchVfsFdHandles: { configurable: true, value: new Map() } })",
        "node:bootstrap/vfs-globals.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install, undefined, &[path, stream])?;
    let vfs_constructor = node_library_export(
        context,
        "vfs.js",
        "internal/vfs/file_system.js",
        "VirtualFileSystem",
    )?;
    let install_loader_projection = context.evaluate_script_rooted(
        r###"((VirtualFileSystem) => {
  const mounted = globalThis.__quenchVfsMountedLayers || new Set();
  Object.defineProperty(globalThis, '__quenchVfsMountedLayers', {
    configurable: true, value: mounted,
  });
  const prototype = VirtualFileSystem.prototype;
  const wrapped = Symbol.for('quench.node.vfs.loaderProjection');
  if (!prototype[wrapped]) {
    const mount = prototype.mount;
    const unmount = prototype.unmount;
    Object.defineProperty(prototype, 'mount', {
      configurable: true, writable: true,
      value: function mountWithLoaderProjection(...args) {
        const result = Reflect.apply(mount, this, args);
        mounted.add(this);
        return result;
      },
    });
    Object.defineProperty(prototype, 'unmount', {
      configurable: true, writable: true,
      value: function unmountWithLoaderProjection(...args) {
        const mountPoint = this.mountPoint;
        const result = Reflect.apply(unmount, this, args);
        mounted.delete(this);
        if (typeof mountPoint === 'string') {
          const require = globalThis['\0quench:require'];
          if (typeof require === 'function') {
            const Module = require('module');
            const prefix = `${path.resolve(mountPoint)}${path.sep}`;
            const cache = Module && Module._cache;
            if (cache && typeof cache === 'object') {
              for (const filename of Object.keys(cache)) {
                if (filename === mountPoint || filename.startsWith(prefix)) delete cache[filename];
              }
            }
            const pathCache = Module && Module._pathCache;
            if (pathCache && typeof pathCache === 'object') {
              for (const key of Object.keys(pathCache)) {
                const filename = pathCache[key];
                if (typeof filename === 'string' &&
                    (filename === mountPoint || filename.startsWith(prefix))) delete pathCache[key];
              }
            }
          }
        }
        return result;
      },
    });
    Object.defineProperty(prototype, wrapped, { value: true });
  }
  const path = globalThis.__nodePath;
  const findLayer = (filename) => {
    const candidate = path.resolve(String(filename));
    return [...mounted].reverse().find((layer) => {
      if (!layer.mounted || typeof layer.mountPoint !== 'string') return false;
      const mountPoint = path.resolve(layer.mountPoint);
      return candidate === mountPoint || candidate.startsWith(`${mountPoint}${path.sep}`);
    });
  };
  const moduleType = (filename) => {
    const candidate = path.resolve(String(filename));
    const layer = findLayer(candidate);
    if (!layer) return undefined;
    const mountPoint = path.resolve(layer.mountPoint);
    let directory = path.dirname(candidate);
    for (;;) {
      const packagePath = path.join(directory, 'package.json');
      try {
        if (layer.statSync(packagePath).isFile()) {
          const pkg = JSON.parse(layer.readFileSync(packagePath, 'utf8'));
          return pkg && pkg.type === 'module' ? 'module' : 'commonjs';
        }
      } catch (cause) {
        if (cause && cause.code !== 'ENOENT' && cause.code !== 'ENOTDIR') {
          const error = new Error(`Invalid package config ${packagePath}. ${cause.message}`);
          error.code = 'ERR_INVALID_PACKAGE_CONFIG';
          error.path = packagePath;
          throw error;
        }
      }
      if (directory === mountPoint) break;
      const parent = path.dirname(directory);
      if (parent === directory ||
          !(parent === mountPoint || parent.startsWith(`${mountPoint}${path.sep}`))) break;
      directory = parent;
    }
    return 'commonjs';
  };
  const resolveModulePath = (base, request) => {
    if (typeof base !== 'string' || typeof request !== 'string') return undefined;
    const candidate = path.isAbsolute(request)
      ? path.resolve(request)
      : path.resolve(base, request);
    const layer = findLayer(candidate);
    if (!layer) return undefined;
    const isFile = (filename) => {
      try { return layer.statSync(filename).isFile(); } catch { return false; }
    };
    const isDirectory = (filename) => {
      try { return layer.statSync(filename).isDirectory(); } catch { return false; }
    };
    const extensions = ['.js', '.json', '.node', '.mjs', '.cjs'];
    const resolveFile = (filename) => {
      if (isFile(filename)) return filename;
      if (path.extname(filename)) return undefined;
      for (const extension of extensions) {
        const withExtension = `${filename}${extension}`;
        if (isFile(withExtension)) return withExtension;
      }
      return undefined;
    };
    const resolveDirectory = (directory, depth = 0) => {
      if (!isDirectory(directory) || depth > 4) return undefined;
      const packagePath = path.join(directory, 'package.json');
      if (isFile(packagePath)) {
        let pkg;
        try { pkg = JSON.parse(layer.readFileSync(packagePath, 'utf8')); }
        catch (cause) {
          const error = new Error(`Invalid package config ${packagePath}. ${cause.message}`);
          error.code = 'ERR_INVALID_PACKAGE_CONFIG';
          error.path = packagePath;
          throw error;
        }
        if (typeof pkg.main === 'string' && pkg.main.length > 0) {
          const main = path.resolve(directory, pkg.main);
          const file = resolveFile(main);
          if (file) return file;
          const nested = resolveDirectory(main, depth + 1);
          if (nested) return nested;
        }
      }
      for (const extension of extensions) {
        const index = path.join(directory, `index${extension}`);
        if (isFile(index)) return index;
      }
      return undefined;
    };
    return resolveFile(candidate) || resolveDirectory(candidate);
  };
  Object.defineProperty(globalThis, '__quenchVfsResolveModulePath', {
    configurable: true, value: resolveModulePath,
  });
  Object.defineProperty(globalThis, '__quenchVfsModuleType', {
    configurable: true, value: moduleType,
  });
  Object.defineProperty(globalThis, '__quenchVfsReadModuleSource', {
    configurable: true,
    value(filename) {
      const candidate = path.resolve(String(filename));
      const layer = findLayer(candidate);
      return layer ? layer.readFileSync(candidate, 'utf8') : undefined;
    },
  });
})"###,
        "node:bootstrap/vfs-module-loader-projection.js",
    )?;
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
const WEB_API_GLOBALS: &[&str] = &["__quenchBlobEnsureReadable", "Blob", "File", "CustomEvent"];
const FETCH_API_GLOBALS: &[&str] = &[
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
    evaluate_polyfill_block(
        context,
        crate::polyfills::bootstrap::globals_extra::web_api_source(),
        "node:bootstrap/shared-vm/web-apis.js",
    )?;
    evaluate_polyfill_block(
        context,
        crate::polyfills::bootstrap::broadcast_channel::JS,
        "node:bootstrap/shared-vm/broadcast-channel.js",
    )?;
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
    static getSchemeOnlyModuleNames() {{ return []; }}
    static getAllBuiltinModuleIds() {{ return Array.from(ids); }}
    static allowRequireByUsers(id) {{ ids.add(id); this.map.set(id, new BuiltinModule(id)); }}
    static allowOptionGatedBuiltins(getOptionValue) {{
      for (const [id, option] of [
        ["bench", "--experimental-bench"],
        ["bench/reporters", "--experimental-bench"],
        ["dtls", "--experimental-dtls"],
        ["ffi", "--experimental-ffi"],
        ["sqlite", "--experimental-sqlite"],
        ["quic", "--experimental-quic"],
        ["stream/iter", "--experimental-stream-iter"],
        ["vfs", "--experimental-vfs"],
        ["zlib/iter", "--experimental-stream-iter"],
      ]) {{
        if (getOptionValue(option)) this.allowRequireByUsers(id);
      }}
    }}
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
            let util = cached_builtin(context, BuiltinModule::Util)?;
            return get(context, util, "types");
        }
        Some(BuiltinModule::InternalUrl) => {
            return cached_builtin(context, BuiltinModule::InternalUrl);
        }
        Some(BuiltinModule::Internal("_http_agent")) => {
            return cached_builtin(context, BuiltinModule::Internal("_http_agent"));
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
            return node_library_module(context, "dgram.js", "dgram.js");
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
    let bundled = crate::node_sources::BUNDLED_DEPENDENCIES
        .iter()
        .any(|(_, relative)| filename == root.join(relative));
    let goal = if filename.starts_with(root.join("lib")) || bundled {
        EntryGoal::CommonJs
    } else {
        EntryGoal::Node
    };
    if crate::node_sources::source(&filename, &context.host_mut().node_library_root).is_some() {
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

/// A require capability installed only in a configured SDK module wrapper.
/// Public module.filename mutation cannot grant access to the timer adapter.
pub(super) fn require_node_library(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if let Some(value) = args.first() {
        if context
            .string_text(*value)?
            .is_some_and(|name| name.strip_prefix("node:").unwrap_or(&name) == "internal/timers")
        {
            return cached_builtin(context, BuiltinModule::InternalTimers);
        }
    }
    require(context, receiver, args)
}

pub(super) fn resolve(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Resolve)?;
    let parent = context.host_function_data()?;
    if let Some(resolved) = resolve_with_paths_option(context, &specifier, parent, args)? {
        return Ok(resolved);
    }
    resolve_request(context, &specifier, parent)
}

/// Node's `require.resolve(request, { paths })` treats each path as a lookup
/// root, including relative entries such as `..` and `./subdir`. The ordinary
/// resolver is relative to the requiring module, so consult explicit roots
/// first and fall back to that behavior when the option is absent or empty.
fn resolve_with_paths_option(
    context: &mut Context<'_>,
    specifier: &str,
    _parent: RootId,
    args: &[RootId],
) -> Result<Option<RootId>, RootedError> {
    if args.len() < 2 || requested_builtin(context, specifier).is_some() {
        return Ok(None);
    }
    let options = args[1];
    if !context.is_object_rooted(options)? {
        return Ok(None);
    }
    let paths_key = context.string_rooted("paths");
    let paths = context.get_property_rooted(options, paths_key)?;
    if !context.is_array_rooted(paths)? {
        return Ok(None);
    }
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(paths, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default()
        .max(0.0) as usize;
    if length == 0 {
        return Ok(None);
    }
    let cwd = context.host_mut().shared_state().borrow().cwd.path();
    let addons_enabled = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--addons");
    let resolver = commonjs_resolver(addons_enabled);
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        let path_root = context.get_property_rooted(paths, key)?;
        let Some(path_text) = context.string_text(path_root)? else {
            continue;
        };
        let path = Path::new(&path_text);
        let base = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        if let Ok(resolution) = resolver.resolve(&base, specifier) {
            return Ok(Some(context.string_rooted(
                &resolution.full_path().to_string_lossy(),
            )));
        }
    }
    Ok(None)
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
    resolve_request(context, &specifier, parent)
}

fn resolve_request(
    context: &mut Context<'_>,
    specifier: &str,
    parent: RootId,
) -> Result<RootId, RootedError> {
    if requested_builtin(context, specifier).is_some() {
        return Ok(context.string_rooted(specifier));
    }
    let filename = resolve_filename(context, specifier, parent)?;
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
        "internal/buffer",
        BuiltinModule::Internal("internal/buffer"),
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
    ("_stream_duplex", BuiltinModule::Internal("_stream_duplex")),
    (
        "node:_stream_duplex",
        BuiltinModule::Internal("_stream_duplex"),
    ),
    (
        "_stream_passthrough",
        BuiltinModule::Internal("_stream_passthrough"),
    ),
    (
        "node:_stream_passthrough",
        BuiltinModule::Internal("_stream_passthrough"),
    ),
    (
        "_stream_readable",
        BuiltinModule::Internal("_stream_readable"),
    ),
    (
        "node:_stream_readable",
        BuiltinModule::Internal("_stream_readable"),
    ),
    (
        "_stream_transform",
        BuiltinModule::Internal("_stream_transform"),
    ),
    (
        "node:_stream_transform",
        BuiltinModule::Internal("_stream_transform"),
    ),
    ("_stream_wrap", BuiltinModule::Internal("_stream_wrap")),
    ("node:_stream_wrap", BuiltinModule::Internal("_stream_wrap")),
    (
        "_stream_writable",
        BuiltinModule::Internal("_stream_writable"),
    ),
    (
        "node:_stream_writable",
        BuiltinModule::Internal("_stream_writable"),
    ),
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
    if matches!(
        builtin,
        BuiltinModule::Stream | BuiltinModule::StreamPromises
    ) {
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

    let module = context.with_startup_intrinsics(|context| build_builtin(context, builtin))?;
    let retained = context.retain(module)?;
    let previous = roots
        .borrow_mut()
        .builtin_cache
        .insert(key.clone(), retained);
    if let Some(previous) = previous {
        context.release_root(previous);
    }
    if matches!(
        builtin,
        BuiltinModule::Crypto | BuiltinModule::Events | BuiltinModule::Fs
    ) {
        let policy = context.with_startup_intrinsics(|context| {
            if matches!(builtin, BuiltinModule::Fs) {
                crate::modules::fs_shared_vm::initialize_directory_policy(context, module)?;
                return crate::modules::fs_shared_vm::initialize_glob_policy(context, module);
            }
            if matches!(builtin, BuiltinModule::Events) {
                let listener = get(context, module, "addAbortListener")?;
                let data = context.array_rooted(&[module, listener])?;
                let initialize = context.host_function_with_data(
                    crate::host::shared_vm::operation("eventsOnInitialize"),
                    data,
                )?;
                return context.define_lazy_data_properties_rooted(
                    module,
                    &["on"],
                    true,
                    initialize,
                );
            }
            let setter =
                node_library_export(context, "crypto.js", "internal/crypto/util.js", "setEngine")?;
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

pub(crate) fn events_on_initialize(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
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
    owner: &str,
    relative: &str,
) -> Result<RootId, RootedError> {
    if relative == "internal/histogram.js" {
        return cached_builtin(context, BuiltinModule::PerfHooks);
    }
    let root = context.host_mut().node_library_root.clone();
    let parent = module_record(context, &root.join("lib").join(owner), None, false)?;
    context.with_startup_intrinsics(|context| {
        load(
            context,
            &root.join("lib").join(relative),
            Some(parent),
            EntryGoal::CommonJs,
        )
    })
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
    let require_key = context.string_rooted("\0quench:require");
    let require = context.get_property_rooted(global, require_key)?;
    let factory = context.evaluate_script_rooted(
        r#"(enqueue, require) => {
          const { AsyncResource } = require("async_hooks");
          function queueMicrotask(callback) {
            // Delegate invalid inputs to the host operation so its established
            // validation and error wording remain intact.
            if (typeof callback !== "function") return enqueue(callback);
            const resource = new AsyncResource("Microtask", {
              requireManualDestroy: true,
            });
            resource.callback = callback;
            enqueue(() => {
              resource.runInAsyncScope(() => {
                try { resource.callback(); }
                finally { resource.emitDestroy(); }
              });
            });
          }
          return { queueMicrotask };
        }"#,
        "node:internal/process/task_queues-quench.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[raw_enqueue, require])
}

fn build_builtin(context: &mut Context<'_>, builtin: BuiltinModule) -> Result<RootId, RootedError> {
    match builtin {
        BuiltinModule::Internal("_http_agent") => {
            // Share the projected internal export with every loader surface.
            let http = cached_builtin(context, BuiltinModule::Http)?;
            let agent = get(context, http, "Agent")?;
            let module = context.null_object_rooted()?;
            set(context, module, "Agent", agent)?;
            Ok(module)
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
            context.evaluate_script_rooted(
                r#"(() => {
                  const support = Object.freeze({
                    enabled: false,
                    nodeModules: false,
                    generatedCode: false,
                  });
                  const noop = () => undefined;
                  return {
                    getSourceMapsSupport: () => support,
                    findSourceMap: noop,
                    getSourceLine: noop,
                    maybeCacheSourceMap: noop,
                    maybeCacheGeneratedSourceMap: noop,
                    sourceMapCacheToObject: () => ({}),
                    setSourceMapsSupport: noop,
                  };
                })()"#,
                "internal/source_map/source_map_cache-quench.js",
            )
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
        BuiltinModule::Internal("internal/buffer") => {
            crate::modules::buffer_constructor_shared_vm::internal_module(context)
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
        BuiltinModule::Internal("_http_incoming") => {
            let root = context.host_mut().node_library_root.clone();
            let parent = module_record(context, &root.join("lib/http.js"), None, false)?;
            load(
                context,
                &root.join("lib/_http_incoming.js"),
                Some(parent),
                EntryGoal::CommonJs,
            )
        }
        BuiltinModule::Internal("_http_common") => {
            crate::modules::http_common_shared_vm::module(context)
        }
        BuiltinModule::Internal("_http_outgoing") => {
            node_library_module(context, "http.js", "_http_outgoing.js")
        }
        BuiltinModule::Internal("_http_server") => {
            let module = context.object_rooted()?;
            let interval = node_library_export(
                context,
                "_http_server.js",
                "_http_server.js",
                "kConnectionsCheckingInterval",
            )?;
            set(context, module, "kConnectionsCheckingInterval", interval)?;
            Ok(module)
        }
        BuiltinModule::Internal(_) => context.object_rooted(),
        BuiltinModule::Fs => crate::modules::fs_shared_vm::module(context),
        BuiltinModule::InternalUrl => {
            let public = cached_builtin(context, BuiltinModule::Url)?;
            let internal = context.object_rooted()?;
            for key in context.enumerable_own_keys_rooted(public, true, true)? {
                let value = context.get_property_rooted(public, key)?;
                context.define_data_property_rooted(internal, key, value, true, true, true)?;
            }
            // Node's internal web-worker implementation calls URLParse;
            // the public URL constructor exposes the same parser as URL.parse.
            let url = get(context, public, "URL")?;
            let url_parse = get(context, url, "parse")?;
            set(context, internal, "URLParse", url_parse)?;
            let is_url = context.host_function(crate::host::shared_vm::operation("urlIsURL"))?;
            let key = context.string_rooted("name");
            let name = context.string_rooted("isURL");
            context.define_data_property_rooted(is_url, key, name, false, false, true)?;
            set(context, internal, "isURL", is_url)?;
            for (name, operation) in [
                ("isURLInstance", "urlIsURLInstance"),
                ("getURLHref", "urlGetURLHref"),
            ] {
                let function = context
                    .host_function_named(crate::host::shared_vm::operation(operation), name)?;
                set(context, internal, name, function)?;
            }

            let convert =
                context.host_function(crate::host::shared_vm::operation("toPathIfFileURL"))?;
            set(context, internal, "toPathIfFileURL", convert)?;
            Ok(internal)
        }
        BuiltinModule::UtilTypes => {
            let util = cached_builtin(context, BuiltinModule::Util)?;
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
        BuiltinModule::Http => crate::modules::http_shared_vm::module(context),
        #[cfg(target_os = "linux")]
        BuiltinModule::Os => node_library_module(context, "os.js", "os.js"),
        #[cfg(not(target_os = "linux"))]
        BuiltinModule::Os => crate::modules::os_shared_vm::module(context),
        BuiltinModule::Buffer => {
            let module = crate::modules::buffer_shared_vm::module(context)?;
            crate::modules::buffer_shared_vm::install_blob_export(context, module)?;
            Ok(module)
        }
        BuiltinModule::Stream => {
            let module = node_library_module(context, "stream.js", "stream.js")?;
            // Node's stream internals normalize typed-array chunks to Buffer
            // instances through Stream._uint8ArrayToBuffer(). The configured
            // Node library's FastBuffer and Quench's public Buffer use separate
            // prototypes, so preserve the byte view while returning Quench's
            // public Buffer class at this compatibility boundary.
            let global = context.global_root()?;
            let buffer = get(context, global, "Buffer")?;
            let wrapper = context.evaluate_script_rooted(
                "((Buffer) => function _uint8ArrayToBuffer(chunk) { return Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength); })",
                "node:stream/uint8array-buffer-adapter.js",
            )?;
            let undefined = context.undefined();
            let converter = context.call_rooted(wrapper, undefined, &[buffer])?;
            set(context, module, "_uint8ArrayToBuffer", converter)?;
            Ok(module)
        }
        BuiltinModule::StreamPromises => {
            node_library_module(context, "stream.js", "stream/promises.js")
        }
        BuiltinModule::StreamIter => {
            node_library_module(context, "stream/iter.js", "stream/iter.js")
        }
        BuiltinModule::ZlibIter => node_library_module(context, "zlib/iter.js", "zlib/iter.js"),
        BuiltinModule::WebStreams => node_library_module(context, "stream/web.js", "stream/web.js"),
        BuiltinModule::StringDecoder => {
            node_library_module(context, "string_decoder.js", "string_decoder.js")
        }
        BuiltinModule::Timers => {
            let promises = cached_builtin(context, BuiltinModule::TimersPromises)?;
            crate::modules::timers_shared_vm::timers_module(context, promises)
        }
        BuiltinModule::TimersPromises => crate::modules::timers_shared_vm::promises_module(context),
        BuiltinModule::InternalTimers => crate::modules::timers_shared_vm::internal_module(context),
        BuiltinModule::NodeTest => crate::modules::test_shared_vm::module(context),
        BuiltinModule::Vm => crate::modules::vm_shared_vm::module(context),
        BuiltinModule::WorkerThreads => crate::modules::worker_threads_shared_vm::module(context),
        BuiltinModule::Url => crate::modules::url_shared_vm::module(context),
        BuiltinModule::Querystring => crate::modules::querystring_shared_vm::module(context),
        BuiltinModule::Readline => node_library_module(context, "readline.js", "readline.js"),
        BuiltinModule::StreamConsumers => {
            crate::modules::web_stream_consumers_shared_vm::module(context)
        }
        BuiltinModule::Events => crate::modules::events_shared_vm::module(context),
        BuiltinModule::Console => crate::modules::console_shared_vm::module(context),
        BuiltinModule::Tty => crate::modules::tty_shared_vm::module(context),
        BuiltinModule::Dns => {
            #[cfg(quench_node_cares)]
            {
                let utils = node_library_module(context, "dns.js", "internal/dns/utils.js")?;
                let initialize = get(context, utils, "initializeDns")?;
                let undefined = context.undefined();
                context.call_rooted(initialize, undefined, &[])?;
                node_library_module(context, "dns.js", "dns.js")
            }
            #[cfg(not(quench_node_cares))]
            {
                crate::modules::dns_shared_vm::module(context)
            }
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
            let codes =
                node_library_export(context, "internal/errors.js", "internal/errors.js", "codes")?;
            let constructor = get(context, codes, "ERR_SOCKET_BUFFER_SIZE")?;
            let undefined = context.undefined();
            context.call_rooted(root, undefined, &[constructor])?;
            context.release_root(root);
            let global = context.global_root()?;
            get(context, global, "\0quench:dgram_module")
        }
        BuiltinModule::PerfHooks => crate::modules::perf_hooks::module(context),
        BuiltinModule::Bench => node_library_module(context, "bench.js", "bench.js"),
        BuiltinModule::BenchReporters => {
            node_library_module(context, "bench/reporters.js", "bench/reporters.js")
        }
        BuiltinModule::Zlib => {
            let module = node_library_module(context, "zlib.js", "zlib.js")?;
            // Node's zlib implementation creates FastBuffer instances from
            // internal/buffer. This runtime's public Buffer constructor owns a
            // separate realm prototype, so link the native fast-buffer class
            // to the public Buffer prototype before zlib starts returning it.
            let fast_buffer = crate::modules::buffer_constructor_shared_vm::fast_buffer(context)?;
            let fast_buffer_prototype = get(context, fast_buffer, "prototype")?;
            let buffer_module = cached_builtin(context, BuiltinModule::Buffer)?;
            let buffer_constructor = get(context, buffer_module, "Buffer")?;
            let buffer_prototype = get(context, buffer_constructor, "prototype")?;
            if !context.set_prototype_rooted(fast_buffer_prototype, buffer_prototype)? {
                return Err(RootedError::host(
                    "cannot connect FastBuffer to the public Buffer prototype",
                ));
            }
            // Native Node's FastBuffer is a separate class in this host. Zlib
            // convenience APIs expose those instances directly, so normalize
            // only their result values through the public Buffer constructor.
            let normalize = context.evaluate_script_rooted(
                r#"((zlib, Buffer) => {
                  const methods = [
                    'deflate', 'deflateRaw', 'gzip', 'gunzip', 'inflate',
                    'inflateRaw', 'unzip', 'brotliCompress', 'brotliDecompress',
                    'zstdCompress', 'zstdDecompress',
                  ];
                  for (const name of methods) {
                    for (const key of [name, `${name}Sync`]) {
                      const original = zlib[key];
                      if (typeof original !== 'function') continue;
                      const wrapped = function(...args) {
                        const last = args.length - 1;
                        if (last >= 0 && typeof args[last] === 'function') {
                          const callback = args[last];
                          args[last] = function(...callbackArgs) {
                            if (callbackArgs.length > 1 &&
                                callbackArgs[1] !== undefined &&
                                ArrayBuffer.isView(callbackArgs[1])) {
                              callbackArgs[1] = Buffer.from(callbackArgs[1]);
                            }
                            return Reflect.apply(callback, this, callbackArgs);
                          };
                        }
                        const result = Reflect.apply(original, this, args);
                        return ArrayBuffer.isView(result) ? Buffer.from(result) : result;
                      };
                      Object.defineProperty(wrapped, 'name', { value: original.name });
                      Object.defineProperty(wrapped, 'length', { value: original.length });
                      zlib[key] = wrapped;
                    }
                  }
                })"#,
                "node:zlib/normalize-buffer-results.js",
            )?;
            let undefined = context.undefined();
            context.call_rooted(normalize, undefined, &[module, buffer_constructor])?;
            // Node exposes the constants table as an immutable snapshot. The
            // shared host's native-binding projection can materialize its
            // properties as ordinary writable data properties, so freeze the
            // public table after loading the Node-owned zlib implementation.
            let constants = get(context, module, "constants")?;
            let global = context.global_root()?;
            let object = get(context, global, "Object")?;
            let freeze = get(context, object, "freeze")?;
            context.call_rooted(freeze, object, &[constants])?;
            Ok(module)
        }
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
        BuiltinModule::Http2Binding => {
            let binding = context.object_rooted()?;
            // Native constant values and attributes from Node v26.12.0's HTTP/2
            // binding (src/node_http2.cc), shared by public exports and internals.
            let descriptors =
                context.parse_json_rooted(include_str!("../../modules/http2_constants.json"))?;
            let constants = context.object_rooted()?;
            for key in context.own_keys_rooted(descriptors)? {
                let descriptor = context.get_property_rooted(descriptors, key)?;
                context.define_property_descriptor_rooted(constants, key, descriptor)?;
            }
            set(context, binding, "constants", constants)?;
            crate::modules::http2_shared_vm::install_binding(context, binding)?;
            let helpers = context.evaluate_script_rooted(
                r#"(() => ({
                  Http2Session: function Http2Session() {
                    const error = new TypeError('Illegal constructor');
                    error.code = 'ERR_CONSTRUCT_CALL_INVALID';
                    throw error;
                  },
                  Http2Stream: class Http2Stream {
                    constructor() {
                      const error = new TypeError('Illegal constructor');
                      error.code = 'ERR_CONSTRUCT_CALL_INVALID';
                      throw error;
                    }
                  },
                  nghttp2ErrorString(code) {
                    const messages = {
                      '-501': 'Invalid argument', '-502': 'Out of buffer space',
                      '-503': 'Unsupported SPDY version', '-504': 'Operation would block',
                      '-505': 'Protocol error', '-506': 'Invalid frame octets',
                      '-507': 'EOF', '-508': 'Data transfer deferred',
                      '-509': 'No more Stream ID available', '-510': 'Stream was already closed or invalid',
                      '-511': 'Stream is closing', '-512': 'The transmission is not allowed for this stream',
                      '-513': 'Stream ID is invalid', '-514': 'Invalid stream state',
                      '-515': 'Another DATA frame has already been deferred',
                      '-516': 'request HEADERS is not allowed', '-517': 'GOAWAY has already been sent',
                      '-518': 'Invalid header block', '-519': 'Invalid state',
                      '-520': 'Unknown error code',
                      '-521': 'The user callback function failed due to the temporal error',
                      '-522': 'The length of the frame is invalid',
                      '-523': 'Header compression/decompression error', '-524': 'Flow control error',
                      '-525': 'Insufficient buffer size given to function',
                      '-526': 'Callback was paused by the application',
                      '-527': 'Too many inflight SETTINGS', '-528': 'Server push is disabled by peer',
                      '-529': 'DATA or HEADERS frame has already been submitted for the stream',
                      '-530': 'The current session is closing',
                    };
                    return messages[code] || 'Unknown error code';
                  },

                  optionsBuffer: new Float64Array(15),
                  settingsBuffer: new Float64Array(8),
                  sessionState: new Float64Array(9),
                  streamState: new Float64Array(6),
                }))()"#,
                "node:http2-binding-compat",
            )?;
            for name in [
                "Http2Session",
                "Http2Stream",
                "nghttp2ErrorString",
                "optionsBuffer",
                "settingsBuffer",
                "sessionState",
                "streamState",
            ] {
                let value = get(context, helpers, name)?;
                set(context, binding, name, value)?;
            }
            Ok(binding)
        }
        BuiltinModule::Http2 => {
            let root = context.host_mut().node_library_root.clone();
            let path = root.join("lib/internal/http2/compat.js");
            let parent_path = root.join("lib/http2.js");
            let parent = module_record(context, &parent_path, None, false)?;
            let compat = load(context, &path, Some(parent), EntryGoal::CommonJs)?;
            // Share Node's own error constructor with the HTTP/2 session
            // adapter so parser failures retain the public NghttpError type.
            let util_path = root.join("lib/internal/http2/util.js");
            let util = load(context, &util_path, Some(parent), EntryGoal::CommonJs)?;
            let nghttp_error = get(context, util, "NghttpError")?;
            let socket_symbol = get(context, util, "kSocket")?;
            let module = context.object_rooted()?;
            for name in ["Http2ServerRequest", "Http2ServerResponse"] {
                let value = get(context, compat, name)?;
                set(context, module, name, value)?;
            }
            let binding = http2_binding(context)?;
            let constants = get(context, binding, "constants")?;
            set(context, module, "constants", constants)?;
            // These helpers are pure JavaScript in Node's public HTTP/2 API.
            // Keep them usable in lightweight hosts that do not provide the
            // native session implementation.
            let helpers = context.evaluate_script_rooted(
                r#"(() => {
                  const fields = [
                    ['headerTableSize', 1, 4096],
                    ['enablePush', 2, true],
                    ['maxConcurrentStreams', 3, 0xffffffff],
                    ['initialWindowSize', 4, 65535],
                    ['maxFrameSize', 5, 16384],
                    ['maxHeaderListSize', 6, 65535],
                    ['enableConnectProtocol', 8, false],
                  ];
                  function http2Error(code, name, message) {
                    const error = new globalThis[name](message);
                    error.code = code;
                    return error;
                  }
                  function receivedArg(value) {
                    if (value == null) return ` Received ${value}`;
                    if (typeof value === 'function') return ` Received function ${value.name}`;
                    if (typeof value === 'object')
                      return ` Received an instance of ${value.constructor?.name || 'Object'}`;
                    const inspected = typeof value === 'string' ? `'${value}'` : String(value);
                    return ` Received type ${typeof value} (${inspected})`;
                  }
                  function validateServerOptions(options, allowHandlerOverload) {
                    if (allowHandlerOverload && typeof options === 'function')
                      return {};
                    if (options === undefined)
                      return {};
                    if (options === null || typeof options !== 'object') {
                      throw http2Error('ERR_INVALID_ARG_TYPE', 'TypeError',
                        `The \"options\" argument must be of type object.${receivedArg(options)}`);
                    }
                    const settings = options.settings;
                    if (settings !== undefined && (settings === null || typeof settings !== 'object')) {
                      throw http2Error('ERR_INVALID_ARG_TYPE', 'TypeError',
                        `The \"options.settings\" property must be of type object.${receivedArg(settings)}`);
                    }
                    for (const key of ['maxSessionInvalidFrames', 'maxSessionRejectedStreams']) {
                      const value = options[key];
                      if (value !== undefined && !Number.isInteger(value)) {
                        throw http2Error('ERR_OUT_OF_RANGE', 'RangeError',
                          `The value of \"options.${key}\" is out of range. It must be an integer. Received ${value}`);
                      }
                      if (value !== undefined && (value < 0 || value > 0xffffffff)) {
                        throw http2Error('ERR_OUT_OF_RANGE', 'RangeError',
                          `The value of \"options.${key}\" is out of range. It must be >= 0 && <= 4294967295. Received ${value}`);
                      }
                    }
                    return options;
                  }
                  function createServer(options, handler) {
                    validateServerOptions(options, true);
                    throw new Error('HTTP/2 server sessions are not available in this runtime');
                  }
                  function createSecureServer(options, handler) {
                    validateServerOptions(options, false);
                    throw new Error('HTTP/2 server sessions are not available in this runtime');
                  }
                  function connect(authority, options, listener) {
                    if (typeof options === 'function') {
                      listener = options;
                      options = undefined;
                    }
                    throw new Error('HTTP/2 client sessions are not available in this runtime');
                  }
                  Object.defineProperty(connect, Symbol.for('nodejs.util.promisify.custom'), {
                    value(authority, options) {
                      return new Promise((resolve, reject) => {
                        let host;
                        try {
                          const target = typeof authority === 'string' ? new URL(authority) : authority;
                          host = target.hostname || target.host || 'localhost';
                        } catch (error) {
                          reject(error);
                          return;
                        }
                        const lookup = options?.lookup;
                        if (typeof lookup !== 'function') {
                          reject(new Error('HTTP/2 client sessions are not available in this runtime'));
                          return;
                        }
                        let called = false;
                        const callback = (error) => {
                          if (called) return;
                          called = true;
                          if (error) reject(error);
                          else reject(new Error('HTTP/2 client sessions are not available in this runtime'));
                        };
                        try {
                          lookup(host, options, callback);
                        } catch (error) {
                          reject(error);
                        }
                      });
                    },
                  });
                  function getDefaultSettings() {
                    // Node returns a null-prototype settings record and keeps
                    // the historical maxHeaderSize alias alongside
                    // maxHeaderListSize. The packed wire format still has a
                    // single setting ID for those two names, so expose the
                    // alias here without adding another entry to `fields`.
                    const values = Object.fromEntries(fields.map(([key, , value]) => [key, value]));
                    const ordered = Object.create(null);
                    for (const key of [
                      'headerTableSize', 'enablePush', 'initialWindowSize',
                      'maxFrameSize', 'maxConcurrentStreams',
                      'maxHeaderSize', 'maxHeaderListSize', 'enableConnectProtocol',
                    ]) {
                      ordered[key] = key === 'maxHeaderSize'
                        ? values.maxHeaderListSize
                        : values[key];
                    }
                    return ordered;
                  }
                  function getPackedSettings(settings = {}) {
                    if (settings === null || typeof settings !== 'object')
                      throw http2Error('ERR_INVALID_ARG_TYPE', 'TypeError', 'The "settings" argument must be of type object.');
                    const records = [];
                    for (const [key, id, fallback] of fields) {
                      const alias = key === 'maxHeaderListSize' &&
                        !Object.prototype.hasOwnProperty.call(settings, key) &&
                        Object.prototype.hasOwnProperty.call(settings, 'maxHeaderSize');
                      if (!Object.prototype.hasOwnProperty.call(settings, key) && !alias) continue;
                      const value = alias ? settings.maxHeaderSize : settings[key];
                      const settingName = alias ? 'maxHeaderSize' : key;
                      if (typeof fallback === 'boolean') {
                        if (typeof value !== 'boolean')
                          throw http2Error('ERR_HTTP2_INVALID_SETTING_VALUE', 'TypeError', `Invalid value for setting "${settingName}": ${value}`);
                        records.push([id, value ? 1 : 0]);
                        continue;
                      }
                      const valid = Number.isInteger(value) && value >= 0 && value <= 0xffffffff &&
                        (key !== 'initialWindowSize' || value <= 0x7fffffff) &&
                        (key !== 'maxFrameSize' || (value >= 16384 && value <= 0xffffff));
                      if (!valid)
                        throw http2Error('ERR_HTTP2_INVALID_SETTING_VALUE', 'RangeError', `Invalid value for setting "${settingName}": ${value}`);
                      records.push([id, value]);
                    }
                    const custom = settings.customSettings;
                    if (custom != null) {
                      if (typeof custom !== 'object')
                        throw http2Error('ERR_INVALID_ARG_TYPE', 'TypeError', 'The "settings.customSettings" property must be of type object.');
                      const entries = Object.entries(custom);
                      if (entries.length > 10)
                        throw http2Error('ERR_HTTP2_TOO_MANY_CUSTOM_SETTINGS', 'RangeError', 'HTTP/2 allows a maximum of 10 custom settings');
                      for (const [rawId, value] of entries) {
                        const id = Number(rawId);
                        if (!/^\d+$/.test(rawId) || !Number.isInteger(id) || id < 0 || id > 0xffff ||
                            !Number.isInteger(value) || value < 0 || value > 0xffffffff)
                          throw http2Error('ERR_HTTP2_INVALID_SETTING_VALUE', 'RangeError', `Invalid value for setting "${rawId}": ${value}`);
                        records.push([id, value]);
                      }
                    }
                    const out = Buffer.alloc(records.length * 6);
                    records.forEach(([id, value], i) => {
                      out.writeUInt16BE(id, i * 6);
                      out.writeUInt32BE(value, i * 6 + 2);
                    });
                    return out;
                  }
                  function getUnpackedSettings(buf, options = {}) {
                    if (!ArrayBuffer.isView(buf) || buf.length === undefined)
                      throw http2Error('ERR_INVALID_ARG_TYPE', 'TypeError', `The "buf" argument must be an instance of Buffer or TypedArray.${receivedArg(buf)}`);
                    if (buf.length % 6 !== 0)
                      throw http2Error('ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH', 'RangeError', 'Packed settings length must be a multiple of six');
                    const result = {};
                    const ids = new Map(fields.map(([key, id]) => [id, key]));
                    for (let offset = 0; offset < buf.length; offset += 6) {
                      const id = ((buf[offset] & 0xff) << 8) | (buf[offset + 1] & 0xff);
                      const value = (((buf[offset + 2] & 0xff) * 0x1000000) +
                        ((buf[offset + 3] & 0xff) << 16) +
                        ((buf[offset + 4] & 0xff) << 8) + (buf[offset + 5] & 0xff)) >>> 0;
                      const key = ids.get(id);
                      if (key === 'enablePush' || key === 'enableConnectProtocol') result[key] = value !== 0;
                      else if (key) result[key] = value;
                      else (result.customSettings ||= {})[id] = value;
                      if (id === 6) result.maxHeaderSize = value;
                    }
                    if (options != null && options.validate) {
                      const packed = getPackedSettings(result);
                      void packed;
                    }
                    return result;
                  }
                  return { getDefaultSettings, getPackedSettings, getUnpackedSettings,
                    createServer, createSecureServer, connect };
                })()"#,
                "node:http2/helpers.js",
            )?;
            for name in [
                "getDefaultSettings",
                "getPackedSettings",
                "getUnpackedSettings",
                "createServer",
                "createSecureServer",
                "connect",
            ] {
                let value = get(context, helpers, name)?;
                set(context, module, name, value)?;
            }
            // The protocol engine is exposed through small JavaScript adapters
            // over Quench's existing net/TLS sockets. Keep these wrappers out
            // of Node's native-dependent `internal/http2/core.js` path while
            // preserving the standard EventEmitter and stream surfaces.
            let net = cached_builtin(context, BuiltinModule::Net)?;
            let tls = cached_builtin(context, BuiltinModule::Tls)?;
            let stream = cached_builtin(context, BuiltinModule::Stream)?;
            let duplex = get(context, stream, "Duplex")?;
            let events = cached_builtin(context, BuiltinModule::Events)?;
            let event_emitter = get(context, events, "EventEmitter")?;
            let diagnostics_channel =
                crate::modules::diagnostics_channel_shared_vm::module(context)?;
            let server_factory = context.evaluate_script_rooted(
                include_str!("../../modules/http2_shared_vm/server.js"),
                "node:http2-server-compat.js",
            )?;
            let undefined = context.undefined();
            let server_exports = context.call_rooted(
                server_factory,
                undefined,
                &[
                    binding,
                    net,
                    tls,
                    duplex,
                    event_emitter,
                    compat,
                    diagnostics_channel,
                    nghttp_error,
                    socket_symbol,
                ],
            )?;
            for name in ["createServer", "createSecureServer"] {
                let value = get(context, server_exports, name)?;
                set(context, module, name, value)?;
            }
            let client_factory = context.evaluate_script_rooted(
                include_str!("../../modules/http2_shared_vm/client.js"),
                "node:http2-client-compat.js",
            )?;
            let client_exports = context.call_rooted(
                client_factory,
                undefined,
                &[
                    binding,
                    net,
                    tls,
                    duplex,
                    event_emitter,
                    diagnostics_channel,
                    nghttp_error,
                    socket_symbol,
                ],
            )?;
            let connect = get(context, client_exports, "connect")?;
            set(context, module, "connect", connect)?;
            // Node exports the same symbol used by internal/http2/util.js so
            // sensitive headers survive the public API -> internal helper path.
            let util = node_library_module(context, "http2.js", "internal/http2/util.js")?;
            let sensitive_headers = get(context, util, "kSensitiveHeaders")?;
            set(context, module, "sensitiveHeaders", sensitive_headers)?;
            Ok(module)
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
    let filename = get(context, parent, "filename")?;
    let directory = match context.string_text(filename)? {
        Some(filename) => Path::new(&filename)
            .parent()
            .unwrap_or(Path::new("."))
            .to_owned(),
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
            && node_lib
                .join(internal_specifier)
                .with_extension("js")
                .is_file());
    if node_internal {
        if let Some((_, relative)) = crate::node_sources::BUNDLED_DEPENDENCIES
            .iter()
            .find(|(name, _)| *name == internal_specifier)
        {
            let root = context.host_mut().node_library_root.clone();
            let path = root.join(relative);
            if path.is_file() || crate::node_sources::source(&path, &root).is_some() {
                return Ok(path);
            }
        }
    }
    if !node_internal {
        if let Some(path) = resolve_vfs_module_path(context, &directory, specifier)? {
            return Ok(path);
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
    let (base, resolved_specifier) = if node_internal {
        (node_lib.as_path(), format!("./{internal_specifier}"))
    } else {
        (directory.as_path(), specifier.to_owned())
    };
    if let Some(path) = exact_file_path(base, &resolved_specifier) {
        return std::fs::canonicalize(path).map_err(|error| RootedError::host(error.to_string()));
    }
    let addons_enabled = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--addons");
    match commonjs_resolver(addons_enabled).resolve(base, &resolved_specifier) {
        Ok(resolution) => Ok(resolution.full_path()),
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
        Err(error) => Err(RootedError::host(error.to_string())),
    }
}

fn resolve_vfs_module_path(
    context: &mut Context<'_>,
    base: &Path,
    specifier: &str,
) -> Result<Option<PathBuf>, RootedError> {
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
    let global = context.global_root()?;
    let read_source = get(context, global, "__quenchVfsReadModuleSource")?;
    if !context.is_callable_rooted(read_source)? {
        return Ok(None);
    }
    let filename = context.string_rooted(&filename.to_string_lossy());
    let receiver = context.undefined();
    let source = context.call_rooted(read_source, receiver, &[filename])?;
    Ok(context.string_text(source)?)
}

fn vfs_module_type(
    context: &mut Context<'_>,
    filename: &Path,
) -> Result<Option<String>, RootedError> {
    let global = context.global_root()?;
    let module_type = get(context, global, "__quenchVfsModuleType")?;
    if !context.is_callable_rooted(module_type)? {
        return Ok(None);
    }
    let filename = context.string_rooted(&filename.to_string_lossy());
    let receiver = context.undefined();
    let result = context.call_rooted(module_type, receiver, &[filename])?;
    Ok(context.string_text(result)?)
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
    let resolve = context.host_function_with_data(super::operation("resolve"), module)?;
    let resolve_paths = context.host_function_with_data(super::operation("resolvePaths"), module)?;
    set(context, resolve, "paths", resolve_paths)?;
    set(context, require, "resolve", resolve)?;
    let main = get(context, process, "mainModule")?;
    set(context, require, "main", main)?;
    let cache = commonjs_cache(context)?;
    set(context, require, "cache", cache)?;
    set(context, module, "require", require)?;
    Ok(module)
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

fn require_es_module(
    context: &mut Context<'_>,
    filename: &Path,
    parent: Option<RootId>,
) -> Result<RootId, RootedError> {
    let url = url::Url::from_file_path(filename)
        .map_err(|_| RootedError::host("invalid ES module filename"))?;
    let module = if let Some(source) = read_vfs_module_source(context, filename)? {
        // VFS providers live in the guest realm, so the NodeHost's ordinary
        // file resolver cannot read their source. A direct ESM module with no
        // host-side graph lookup can still be evaluated from the projected
        // bytes; static dependencies continue through the host resolver.
        let bytes = source.as_bytes().to_vec();
        quench_runtime::ModuleSource {
            name: filename.to_string_lossy().into_owned(),
            source,
            bytes,
        }
    } else {
        quench_runtime::Host::resolve_dynamic_import(
            context.host_mut(),
            &filename.to_string_lossy(),
            url.as_str(),
        )
        .map_err(RootedError::host)?
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
    let cache =
        if crate::node_sources::source(filename, &context.host_mut().node_library_root).is_some() {
            ModuleCache::NodeLibrary
        } else {
            ModuleCache::CommonJs
        };
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
    let vfs_source = read_vfs_module_source(context, filename)?;
    let vfs_type = vfs_module_type(context, filename)?;
    let extension = filename.extension().and_then(|extension| extension.to_str());
    let is_esm = goal == EntryGoal::Node
        && match extension {
            Some("mjs") => true,
            Some("js") => match vfs_type {
                Some(module_type) => module_type == "module",
                None if vfs_source.is_some() => false,
                None => {
                    source_kind(filename).map_err(RootedError::host)?
                        == quench_runtime::SourceKind::Module
                }
            },
            _ => source_kind(filename).map_err(RootedError::host)?
                == quench_runtime::SourceKind::Module,
        };
    let is_main = parent.is_none() && context.host_mut().commonjs_entry.is_some();
    let module = module_record(context, filename, parent, is_main)?;
    if let Some(parent) = parent {
        transition_child(context, parent, module, ChildTransition::Attach)?;
    }
    cache.insert(context, &key, module)?;
    if let Some((directory, specifier)) = resolution {
        let name = context.string_rooted(&key);
        set(context, directory, specifier, name)?;
    }
    if is_esm {
        let retained = context.retain(module)?;
        roots
            .borrow_mut()
            .active_esm_requires
            .insert(key.clone(), retained);
    }
    let result = (|| {
        if is_esm {
            let exports = require_es_module(context, filename, parent)?;
            set(context, module, "exports", exports)?;
            let loaded = context.boolean(true);
            set(context, module, "loaded", loaded)?;
            return Ok(exports);
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
        let file_text = if let Some(source) = vfs_source {
            source
        } else {
            match std::fs::read(filename) {
                Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|error| {
                    String::from_utf8_lossy(error.as_bytes()).into_owned()
                }),
                Err(error) => {
                    crate::node_sources::source(filename, &context.host_mut().node_library_root)
                        .ok_or_else(|| RootedError::host(error.to_string()))?
                        .to_owned()
                }
            }
        };
        let file_text = match library_relative {
            Some(path) if path == Path::new("lib/internal/per_context/primordials.js") => {
                // Node's stream-iterator library consumes this helper from
                // primordials, while this pinned source doesn't export it.
                // Add it before the primordials object is frozen.
                file_text.replace(
                    "ObjectSetPrototypeOf(primordials, null);",
                    "primordials.PromiseWithResolvers = primordials.Promise.withResolvers.bind(primordials.Promise);\nObjectSetPrototypeOf(primordials, null);",
                )
            }
            Some(path) if path == Path::new("lib/internal/streams/duplexify.js") => {
                // Quench's global Blob is supplied by the host rather than
                // internal/blob.js, so it does not carry Node's private blob
                // handle symbol. Preserve Node's brand check and accept a Blob
                // from the active global realm as well.
                file_text.replace(
                    "if (isBlob(body)) {",
                    "if (isBlob(body) || (typeof globalThis.Blob === 'function' && body instanceof globalThis.Blob)) {",
                )
            }
            Some(path) if path == Path::new("lib/internal/test_runner/utils.js") => {
                // Keep Node's public test-runner error contract stable across
                // engines: V8 reports invalid RegExp flags with the flag list,
                // while the host engine's native message is less specific.
                // Validate flags before constructing the RegExp so
                // convertStringToRegExp can still wrap pattern syntax errors.
                file_text.replace(
                    "  try {\n    return new RegExp(pattern, flags);",
                    "  if ([...flags].some((flag, index) =>\n      !'dgimsuvy'.includes(flag) || flags.indexOf(flag) !== index) ||\n      (flags.includes('u') && flags.includes('v'))) {\n    throw new ERR_INVALID_ARG_VALUE(\n      name, str,\n      `is an invalid regular expression. Invalid flags supplied to RegExp constructor '${flags}'`,\n    );\n  }\n\n  try {\n    return new RegExp(pattern, flags);",
                )
            }
            Some(path) if path == Path::new("lib/internal/child_process.js") => {
                super::policy_projection::project(
                    &file_text,
                    &["setupChannel", "kChannelHandle", "getValidStdio"],
                )
                .map_err(RootedError::host)?
            }
            Some(path) if path == Path::new("lib/internal/child_process/serialization.js") => {
                super::policy_projection::project(&file_text, &["advanced", "json"])
                    .map_err(RootedError::host)?
            }
            Some(path) if path == Path::new("lib/net.js") => super::policy_projection::project(
                &file_text,
                &["BoundSocket", "_TransferredBoundSocket"],
            )
            .map_err(RootedError::host)?,
            Some(path) if path == Path::new("lib/child_process.js") => {
                super::policy_projection::project(&file_text, &["_forkChild"])
                    .map_err(RootedError::host)?
            }
            _ => file_text,
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
                let text = if text.starts_with("#!") {
                    text.find('\n').map_or("", |end| &text[end..])
                } else {
                    text
                };
                let primordial_seed = if is_primordials {
                    Some(context.object_rooted()?)
                } else {
                    None
                };
                let text: std::borrow::Cow<'_, str> = if is_primordials {
                    format!("primordials ??= {{}};\n{text}\nmodule.exports = primordials;\n").into()
                } else {
                    text.into()
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
                let is_realm_bootstrap =
                    library_relative == Some(Path::new("lib/internal/bootstrap/realm.js"));
                let internal_binding = if is_realm_bootstrap {
                    r#"const getInternalBinding = (name) => {
  if (name === "builtins") return { builtinIds: [], compileFunction() {}, setInternalLoaders() {} };
  if (name === "module_wrap") return { ModuleWrap: class ModuleWrap {} };
  return {};
};
const getLinkedBinding = () => ({});
"#
                } else if library_relative == Some(Path::new("lib/internal/test/binding.js")) {
                    // Node's test binding exposes the same internal bindings as
                    // engine code. Route those lookups through Quench's adapters
                    // too, so the narrow compatibility methods missing from the
                    // host engine remain available to the Node test suite.
                    node_internal_binding_source()
                } else if library_relative
                    == Some(Path::new("lib/internal/per_context/domexception.js"))
                {
                    // This SDK per-context source has two explicit native symbol
                    // capabilities in Node's wrapper. Reuse existing symbol owners.
                    "const privateSymbols=__nativeInternalBinding('util').privateSymbols; const perIsolateSymbols=__nativeInternalBinding('symbols');"
                } else if in_node_internal {
                    node_internal_binding_source()
                } else {
                    ""
                };
                let prefix = if in_node_internal {
                    format!(
                        "(function (exports, require, module, __filename, __dirname, primordials, __nativeInternalBinding) {{ 'use strict'; {internal_binding}"
                    )
                } else {
                    format!("{WRAPPER_PREFIX}{internal_binding}")
                };
                let wrapper_source = format!("{prefix}{text}{WRAPPER_SUFFIX}");
                // The wrapper holds the text now, so the file is released before compiling it.
                drop(text);
                drop(file_text);
                let wrapper = evaluate_wrapper(context, &prefix, &wrapper_source, &key)?;
                // The compiled program keeps its own source; the module body runs inside this
                // call, so the wrapper is released before it.
                drop(wrapper_source);
                if parent.is_none() {
                    crate::host::resident_pages::release_read_only_pages();
                    crate::host::resident_pages::quench_entry_compiled();
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
        let loaded = context.boolean(true);
        set(context, module, "loaded", loaded)?;
        Ok(exports)
    })();
    if is_esm {
        if let Some(retained) = roots.borrow_mut().active_esm_requires.remove(&key) {
            context.release_root(retained);
        }
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
            crate::modules::fs_policy_shared_vm::adopt(context, *exports)?;
            let state = get(context, *exports, "vfsState")?;
            let global = context.global_root()?;
            set(context, global, "\0quench:vfsState", state)?;
        }
    }
    result
}

fn evaluate_wrapper(
    context: &mut Context<'_>,
    prefix: &str,
    source: &str,
    filename: &str,
) -> Result<RootId, RootedError> {
    let line_offset =
        u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).map_err(|_| {
            RootedError::host("CommonJS wrapper line origin exceeds compiler positions")
        })?;
    let column_offset = u32::try_from(
        prefix
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count(),
    )
    .map_err(|_| RootedError::host("CommonJS wrapper column origin exceeds compiler positions"))?;
    context.evaluate_specialized_script_with_origin_rooted(
        source,
        filename,
        line_offset,
        column_offset,
    )
}

pub(crate) fn compile_public(
    context: &mut Context<'_>,
    module: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let content = args.first().copied().unwrap_or_else(|| context.undefined());
    let filename = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let content = context
        .string_text(content)?
        .ok_or_else(|| RootedError::host("Module._compile content must be a string"))?;
    let name = context
        .string_text(filename)?
        .ok_or_else(|| RootedError::host("Module._compile filename must be a string"))?;
    let text = content.as_str();
    let text = if text.starts_with("#!") {
        text.find('\n').map_or("", |end| &text[end..])
    } else {
        text
    };
    let source = format!("{WRAPPER_PREFIX}{text}{WRAPPER_SUFFIX}");
    let wrapper = evaluate_wrapper(context, WRAPPER_PREFIX, &source, &name)?;
    let exports = get(context, module, "exports")?;
    let require = context.host_function_with_data(super::operation("require"), module)?;
    let resolve = context.host_function_with_data(super::operation("resolve"), module)?;
    let resolve_paths = context.host_function_with_data(super::operation("resolvePaths"), module)?;
    set(context, resolve, "paths", resolve_paths)?;
    set(context, require, "resolve", resolve)?;
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
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
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
  // Node may keep small, length-constructed views on-heap until their
  // `buffer` property is observed. Quench stores an ArrayBuffer for every
  // view, so remember the observable transition in this compatibility
  // binding and use V8's 64-byte on-heap cutoff for fresh small views.
  const kArrayBufferViewsWithBuffer = Symbol.for("quench.node.arrayBufferViewsWithBuffer");
  const arrayBufferViewsWithBuffer = globalThis[kArrayBufferViewsWithBuffer] ||
    (globalThis[kArrayBufferViewsWithBuffer] = new WeakSet());
  const kTrackedArrayBufferGetter = Symbol.for("quench.node.trackedArrayBufferGetter");
  const trackArrayBufferGetter = (prototype) => {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, "buffer");
    if (!descriptor || typeof descriptor.get !== "function" || descriptor.get[kTrackedArrayBufferGetter]) return;
    const getter = descriptor.get;
    const trackedGetter = function() {
      arrayBufferViewsWithBuffer.add(this);
      return Reflect.apply(getter, this, []);
    };
    Object.defineProperty(trackedGetter, kTrackedArrayBufferGetter, { value: true });
    Object.defineProperty(prototype, "buffer", {
      ...descriptor,
      get: trackedGetter,
    });
  };
  trackArrayBufferGetter(Object.getPrototypeOf(Uint8Array.prototype));
  trackArrayBufferGetter(DataView.prototype);
  switch (String(name)) {
    case "heap_utils": return {
      // Heap graph inspection is an embedder-only testing hook. Preserve the
      // identity semantics of the native graph builder for values supplied
      // by the test harness, even when the host does not expose V8's graph
      // traversal API.
      buildEmbedderGraph(...values) {
        const nodes = [];
        for (const value of values) {
          if (!nodes.some((node) => Object.is(node.value, value))) nodes.push({ value });
        }
        return nodes;
      },
    };
    case "js_stream": return {
      JSStream: class JSStream {
        constructor() { this.__quenchJSStream = true; }
        readBuffer(buffer) {
          if (this.__quenchOwner) this.__quenchOwner.push(buffer);
          else this.onread?.(0, buffer);
        }
        emitEOF() {
          if (this.__quenchOwner) this.__quenchOwner.push(null);
          else this.onread?.(-4095);
        }
        readStart() { return this.onreadstart?.() ?? 0; }
        readStop() { return this.onreadstop?.() ?? 0; }
        shutdown(req) { return this.onshutdown?.(req) ?? 0; }
        finishWrite(req, status) {
          req.oncomplete?.(status ? Object.assign(new Error("write failed"), { errno: status }) : undefined);
        }
        finishShutdown(req, status) { req.oncomplete?.(status); }
      },
    };
    case "pipe_wrap":
    case "tcp_wrap":
    case "stream_wrap":
    case "tty_wrap":
    case "udp_wrap":
    case "os":
    case "timers":
    case "config":
    case "icu":
    case "encoding_binding":
    case "zlib":
    case "symbols":
    case "cares_wrap":
    case "fs_event_wrap":
    case "errors":
    case "report": return __nativeInternalBinding(name);
    case "v8": {
      let native;
      try { native = __nativeInternalBinding(name); } catch { /* V8 internals are not native here. */ }
      native ||= Object.create(null);
      return Object.assign(Object.create(native), {
        // The host VM doesn't publish V8's process-local hash seed. Keep the
        // internal testing binding useful for child-process uniqueness checks
        // by exposing a stable, process-unique fallback value.
        getHashSeed: typeof native.getHashSeed === "function" ? native.getHashSeed : () => process.pid >>> 0,
      });
    }
    case "modules": {
      let native;
      try { native = __nativeInternalBinding(name); } catch { /* Package traversal is not native here. */ }
      native ||= Object.create(null);
      const compileCacheStatuses = ["FAILED", "ENABLED", "ALREADY_ENABLED", "DISABLED"];
      const compileCachePath = require("path");
      const permissionFailure = (directory, readOnly) => {
        const permission = process.permission;
        if (!permission || typeof permission.has !== "function") return undefined;
        const resolved = compileCachePath.resolve(directory);
        if (!readOnly && !permission.has("fs.write", resolved)) {
          return `Skipping compile cache because write permission for ${resolved} is not granted`;
        }
        if (!permission.has("fs.read", resolved)) {
          return `Skipping compile cache because read permission for ${resolved} is not granted`;
        }
        return undefined;
      };
      const debugCompileCache = (message) => {
        if ((process.env.NODE_DEBUG_NATIVE || "").includes("COMPILE_CACHE")) {
          process.stderr.write(`${message}\n`);
        }
      };
      // NODE_COMPILE_CACHE is enabled by Node during startup. Quench has no
      // V8 cache backend, but it must still honor the permission-model failure
      // path instead of silently touching a denied directory or logging fake
      // cache reads/writes.
      if (process.env.NODE_COMPILE_CACHE && !process.env.NODE_DISABLE_COMPILE_CACHE) {
        const failure = permissionFailure(
          process.env.NODE_COMPILE_CACHE,
          process.env.NODE_COMPILE_CACHE_READONLY === "1",
        );
        if (failure) debugCompileCache(failure);
      }
      const enableCompileCache = (directory, _portable, readOnly) => {
        if (process.env.NODE_DISABLE_COMPILE_CACHE) return [3];
        const selected = process.env.NODE_COMPILE_CACHE || directory;
        if (typeof selected !== "string" || selected.length === 0) return [0, "Invalid compile cache directory"];
        const failure = permissionFailure(selected, readOnly);
        if (failure) {
          debugCompileCache(failure);
          return [0, failure];
        }
        // This runtime does not expose V8's compiled-code cache API. Keep the
        // Node public binding shape and report the documented failure result;
        // never claim that a marker file is a usable bytecode cache.
        return [0, "Compile cache is not supported by this runtime"];
      };
      native = Object.assign(Object.create(native), {
        compileCacheStatus: compileCacheStatuses,
        enableCompileCache,
        getCompileCacheDir: () => undefined,
        flushCompileCache: () => undefined,
      });
      const fs = require("fs");
      const path = require("path");
      const { fileURLToPath } = require("url");
      const readPackage = (jsonPath) => {
        try {
          const source = fs.readFileSync(jsonPath, "utf8").replace(/^\uFEFF/, "");
          const value = JSON.parse(source);
          const plain = (field) => value[field] === undefined ? undefined :
            (typeof value[field] === "string" ? value[field] : JSON.stringify(value[field]));
          return [value.name, value.main, value.type || "none", plain("imports"), plain("exports"), jsonPath];
        } catch (error) {
          if (error && error.code === "ENOENT") return undefined;
          throw error;
        }
      };
      // Node's internal/vfs/setup.js parses package.json bytes obtained from a
      // virtual provider through this internal binding entry point. The host
      // engine does not expose it, so provide the same tuple shape consumed by
      // Node's package_json_reader and preserve its invalid-config error code.
      const parsePackageJSON = (content, jsonPath, isESM, base, specifier) => {
        let value;
        try {
          value = JSON.parse(Buffer.from(content).toString("utf8").replace(/^\uFEFF/, ""));
        } catch (cause) {
          const from = base == null ? "" : ` while importing ${base}`;
          const message = cause && cause.message ? cause.message : String(cause);
          const error = new Error(`Invalid package config ${jsonPath}${from}. ${message}`);
          error.code = "ERR_INVALID_PACKAGE_CONFIG";
          error.path = jsonPath;
          if (base !== undefined) error.base = base;
          if (specifier !== undefined) error.specifier = specifier;
          throw error;
        }
        const plain = (field) => value[field] === undefined ? undefined :
          (typeof value[field] === "string" ? value[field] : JSON.stringify(value[field]));
        return [value.name, value.main, value.type || "none", plain("imports"), plain("exports"), jsonPath];
      };
      const nearest = (checkPath) => {
        let current = String(checkPath);
        if (!current.endsWith(path.sep)) current = path.dirname(current);
        else current = current.slice(0, -path.sep.length) || path.sep;
        for (;;) {
          const parent = path.dirname(current);
          if (parent === current || path.basename(current) === "node_modules") return undefined;
          const found = readPackage(path.join(current, "package.json"));
          if (found !== undefined) return found;
          current = parent;
        }
      };
      const scope = (resolved) => {
        let filename = String(resolved);
        if (filename.startsWith("file:")) filename = fileURLToPath(filename);
        const result = nearest(filename);
        return result === undefined ? path.join(path.dirname(filename), "package.json") : result;
      };
      return Object.assign(Object.create(native), {
        parsePackageJSON: typeof native.parsePackageJSON === "function" ? native.parsePackageJSON : parsePackageJSON,
        readPackageJSON: typeof native.readPackageJSON === "function" ? native.readPackageJSON : (jsonPath) => readPackage(String(jsonPath)),
        getNearestParentPackageJSON: typeof native.getNearestParentPackageJSON === "function" ? native.getNearestParentPackageJSON : nearest,
        getNearestParentPackageJSONType: typeof native.getNearestParentPackageJSONType === "function" ? native.getNearestParentPackageJSONType : (checkPath) => nearest(checkPath)?.[2],
        getPackageScopeConfig: typeof native.getPackageScopeConfig === "function" ? native.getPackageScopeConfig : scope,
        getPackageType: typeof native.getPackageType === "function" ? native.getPackageType : (resolved) => {
          const result = scope(resolved);
          return Array.isArray(result) ? result[2] : "none";
        },
      });
    }
    case "contextify": {
      const native = __nativeInternalBinding(name);
      return Object.assign(Object.create(native), {
        compileFunctionForCJSLoader: typeof native.compileFunctionForCJSLoader === "function" ? native.compileFunctionForCJSLoader :
          (code, filename) => native.compileFunction(code, filename, undefined, undefined, undefined, false, undefined, undefined,
            ["exports", "require", "module", "__filename", "__dirname"]),
        containsModuleSyntax: typeof native.containsModuleSyntax === "function" ? native.containsModuleSyntax : () => false,
      });
    }
    case "http2": {
      const binding = __nativeInternalBinding(name) || Object.create(null);
      if (binding.optionsBuffer !== undefined) {
        if (typeof binding.setCallbackFunctions !== "function") {
          binding.setCallbackFunctions = (...callbacks) => {
            globalThis[Symbol.for("quench.node.http2CallbackFunctions")] = callbacks;
          };
        }
        return binding;
      }
      const stateKey = Symbol.for("quench.node.http2BindingState");
      const state = globalThis[stateKey] || (globalThis[stateKey] = {
        optionsBuffer: new Float64Array(15),
        settingsBuffer: new Float64Array(8),
        sessionState: new Float64Array(9),
        streamState: new Float64Array(6),
      });
      if (state.binding) return state.binding;
      // Keep native binding exports available through the prototype while
      // supplying the typed state buffers expected by Node's JS helpers.
      const augmented = Object.assign(Object.create(binding), state);
      // Node's internal/http2/core registers callbacks consumed by the native
      // nghttp2 session wrapper. Quench's public HTTP/2 adapter dispatches its
      // own events, but the registration API must still exist when Node's
      // internal core module is loaded by compatibility tests.
      if (typeof augmented.setCallbackFunctions !== "function") {
        augmented.setCallbackFunctions = (...callbacks) => {
          state.callbackFunctions = callbacks;
        };
      }
      state.binding = augmented;
      return augmented;
    }
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
      getSharedArrayBufferGrowable:
        __nativeInternalBinding("util").getSharedArrayBufferGrowable,
      arrayBufferViewHasBuffer(view) {
        if (!ArrayBuffer.isView(view)) return false;
        if (arrayBufferViewsWithBuffer.has(view)) return true;
        return view.byteLength > 64;
      },
      constructSharedArrayBuffer: (length) => new SharedArrayBuffer(length),
      guessHandleType: __nativeInternalBinding("util").guessHandleType,
      defineLazyProperties: __nativeInternalBinding("util").defineLazyProperties,
      sleep: () => undefined,
    });
    case "types": return __nativeInternalBinding("types");
    case "block_list": return { AF_INET: 2, AF_INET6: 10 };
    case "constants": {
      const native = __nativeInternalBinding("constants");
      // Node exposes internal constants as a null-prototype tree, including
      // the stable top-level internal and trace namespaces. Clone recursively
      // so embedding-provided constants do not leak Object.prototype into the
      // Node internalBinding surface.
      const nullPrototypeCopy = (value) => {
        if (value === null || typeof value !== "object") return value;
        const copy = Object.create(null);
        for (const key of Object.keys(value)) copy[key] = nullPrototypeCopy(value[key]);
        return copy;
      };
      const crypto = Object.assign({}, native.crypto, {
        POINT_CONVERSION_COMPRESSED: 2,
        POINT_CONVERSION_UNCOMPRESSED: 4,
      });
      return Object.assign(Object.create(null), {
        fs: nullPrototypeCopy(native.fs),
        zlib: nullPrototypeCopy(native.zlib),
        os: nullPrototypeCopy(native.os || { signals: (require("os").constants || {}).signals || {}, errno: { EISDIR: -21 } }),
        crypto: nullPrototypeCopy(crypto),
        internal: Object.create(null),
        trace: Object.create(null),
      });
    }
    case "http_parser": return __nativeInternalBinding(name);
    case "options": return __nativeInternalBinding("options");
    case "credentials": {
      let native;
      try { native = __nativeInternalBinding(name); } catch { /* No native credentials binding. */ }
      native ||= Object.create(null);
      if (typeof native.safeGetenv === "function") return native;
      return Object.assign(Object.create(native), {
        safeGetenv(name) { return process.env[String(name)]; },
      });
    }
    case "module_wrap": {
      let native;
      try { native = __nativeInternalBinding(name); } catch { /* V8 module hooks are owned by Quench. */ }
      native ||= Object.create(null);
      return Object.assign(Object.create(native), {
        ModuleWrap: native.ModuleWrap || class ModuleWrap {},
        // Quench's module runtime owns import-meta and dynamic-import dispatch;
        // these callbacks are only registration hooks for Node's V8 wrapper.
        setImportMetaResolveInitializer() {},
        setImportModuleDynamicallyCallback() {},
        setInitializeImportMetaObjectCallback() {},
      });
    }
    case "worker": {
      let native;
      try { native = __nativeInternalBinding(name); } catch { /* No native worker binding. */ }
      native ||= Object.create(null);
      return Object.assign(Object.create(native), {
        isMainThread: typeof native.isMainThread === "boolean"
          ? native.isMainThread
          : process.getBuiltinModule("worker_threads").isMainThread,
        resourceLimits: native.resourceLimits || new Float64Array(4),
        getEnvMessagePort: typeof native.getEnvMessagePort === "function"
          ? native.getEnvMessagePort
          : () => {
            const port = require("worker_threads").parentPort;
            return {
              postMessage(message) { return port.__quenchInternalPost(message); },
            };
          },
      });
    }
    case "diagnostics_channel": {
      // Node's bootstrap re-links native diagnostic channels after startup.
      // Preserve any native implementation and fill the lifecycle hooks that
      // the shared diagnostics-channel implementation does not provide.
      const key = Symbol.for("quench.node.diagnosticsChannelBinding");
      if (globalThis[key]) return globalThis[key];
      let native;
      try { native = __nativeInternalBinding(name); } catch { /* No native binding is registered. */ }
      native ||= Object.create(null);
      const binding = Object.create(native);
      binding.subscribers = Array.isArray(native.subscribers) ? native.subscribers : [];
      binding.linkNativeChannel = typeof native.linkNativeChannel === "function" ?
        (...args) => Reflect.apply(native.linkNativeChannel, native, args) :
        (callback) => {
          if (typeof callback !== "function") throw new TypeError("callback must be a function");
        };
      binding.notifyChannelActive = typeof native.notifyChannelActive === "function" ?
        (...args) => Reflect.apply(native.notifyChannelActive, native, args) : () => {};
      binding.notifyChannelInactive = typeof native.notifyChannelInactive === "function" ?
        (...args) => Reflect.apply(native.notifyChannelInactive, native, args) : () => {};
      return globalThis[key] = binding;
    }
    case "string_decoder": return __nativeInternalBinding("string_decoder");
    case "buffer": return Object.assign({}, require("buffer"), __nativeInternalBinding("buffer"));
    case "fs": {
      // All Node internal modules, including internal/test/binding, must see
      // one fs binding object. Tests and core mutate the same binding methods
      // (for example readdir) to exercise Dirent fallback behavior.
      const fsBindingKey = Symbol.for("quench.node.internalBinding.fs");
      if (globalThis[fsBindingKey]) return globalThis[fsBindingKey];
      const native = __nativeInternalBinding("fs");
      if (typeof native.openFileHandle === "function") {
        Object.defineProperty(globalThis, fsBindingKey, {
          configurable: true,
          value: native,
        });
        return native;
      }
      const binding = Object.create(native);
      Object.defineProperty(binding, "openFileHandle", {
        value(path, flags, mode, req, context) {
          if (req !== undefined || !context || typeof context !== "object") return undefined;
          const fs = process.getBuiltinModule("fs");
          const fd = fs.openSync(path, flags, mode);
          setTimeout(() => {
            try { fs.closeSync(fd); } catch {}
            const error = new Error(
              `A FileHandle object was closed during garbage collection. This used to be allowed with a deprecation warning but is now considered an error. Please close FileHandle objects explicitly. File descriptor: ${fd} (${String(path)})`
            );
            error.code = "ERR_INVALID_STATE";
            process.emit("uncaughtException", error);
          }, 0);
          return undefined;
        },
      });
      Object.defineProperty(globalThis, fsBindingKey, {
        configurable: true,
        value: binding,
      });
      return binding;
    }
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
      isBuildingSnapshotBuffer: [false],
      setSerializeCallback: () => {},
      setDeserializeCallback: () => {},
      setDeserializeMainFunction: () => {},
    };
    case "process": return process;
    case "ipc_serdes": {
      // The native Node binding uses V8's serializer. Keep the paired-process
      // advanced IPC path available in this VM with a tagged graph encoding;
      // unlike JSON framing it preserves cycles and the common structured
      // values used by child_process and cluster.
      function encodeGraph(root, BufferCtor) {
        const nodes = [];
        const seen = new Map();
        function encode(value) {
          if (value === undefined) return { $type: "undefined" };
          if (typeof value === "bigint") return { $type: "bigint", value: String(value) };
          if (typeof value === "number") {
            if (Number.isNaN(value)) return { $type: "number", value: "NaN" };
            if (value === Infinity) return { $type: "number", value: "Infinity" };
            if (value === -Infinity) return { $type: "number", value: "-Infinity" };
            if (Object.is(value, -0)) return { $type: "number", value: "-0" };
          }
          if (value === null || (typeof value !== "object" && typeof value !== "function")) return value;
          if (seen.has(value)) return { $ref: seen.get(value) };
          const id = nodes.length;
          seen.set(value, id);
          nodes.push(null);
          // Node's V8 serializer classifies Buffers by comparing the value's
          // constructor with the stable Buffer constructor passed by the
          // child_process serializer. `Buffer.isBuffer()` ignores an
          // instance's reassigned constructor and therefore disagrees with
          // advanced IPC serialization for modified Buffer/Uint8Array values.
          if (value.constructor === BufferCtor) {
            nodes[id] = {
              type: "Buffer",
              data: BufferCtor.from(value.buffer, value.byteOffset, value.byteLength).toString("base64"),
            };
          } else if (ArrayBuffer.isView(value)) {
            // Preserve typed-array payloads as bytes. Encoding large views as
            // one JSON number per element makes advanced IPC prohibitively
            // expensive (the child-process regression fixture sends 32 MiB).
            nodes[id] = {
              type: "View",
              name: value.constructor?.name || "Uint8Array",
              length: value.length,
              byteLength: value.byteLength,
              data: Buffer.from(value.buffer, value.byteOffset, value.byteLength).toString("base64"),
            };
          } else if (value instanceof ArrayBuffer) {
            nodes[id] = { type: "ArrayBuffer", data: Buffer.from(new Uint8Array(value)).toString("base64") };
          } else if (Array.isArray(value)) {
            nodes[id] = { type: "Array", values: value.map(encode) };
          } else if (value instanceof Map) {
            nodes[id] = { type: "Map", values: Array.from(value, ([key, item]) => [encode(key), encode(item)]) };
          } else if (value instanceof Set) {
            nodes[id] = { type: "Set", values: Array.from(value, encode) };
          } else if (value instanceof Date) {
            nodes[id] = { type: "Date", value: value.getTime() };
          } else if (value instanceof RegExp) {
            nodes[id] = { type: "RegExp", source: value.source, flags: value.flags };
          } else if (typeof DOMException === "function" && value instanceof DOMException) {
            // V8 clones DOMException as an Error carrying its stack, rather
            // than preserving the DOMException name/message/code fields.
            nodes[id] = { type: "DOMException", stack: value.stack };
          } else if (value instanceof Error) {
            // V8 preserves the built-in Error fields, cause, and AggregateError
            // members, but drops arbitrary enumerable properties.
            nodes[id] = {
              type: "Error",
              name: value.name,
              message: value.message,
              stack: value.stack,
              hasCause: Object.hasOwn(value, "cause"),
              cause: Object.hasOwn(value, "cause") ? encode(value.cause) : undefined,
              errors: value instanceof AggregateError ? encode(value.errors) : undefined,
            };
          } else {
            nodes[id] = { type: "Object", props: Object.keys(value).map(key => [key, encode(value[key])]) };
          }
          return { $ref: id };
        }
        return { root: encode(root), nodes };
      }
      function serialize(value, BufferCtor = Buffer) {
        const payload = BufferCtor.from(JSON.stringify(encodeGraph(value, BufferCtor)), "utf8");
        // `internal/child_process/serialization.js` advanced mode writes the
        // ipc_serdes result directly to the pipe. Node's binding includes a
        // four-byte big-endian payload length; without it the receiver keeps
        // buffering forever while waiting for the declared message size.
        const frame = BufferCtor.allocUnsafe(payload.length + 4);
        frame.writeUInt32BE(payload.length, 0);
        payload.copy(frame, 4);
        return frame;
      }
      function deserialize(bytes) {
        const serialized = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes);
        // The internal binding also accepts V8's wire format. Validate the
        // host-object tag before rejecting wire formats this graph codec does
        // not otherwise decode, matching ipc_serdes' coded validation error.
        if (serialized[0] === 0xff && serialized[1] === 0x0f && serialized[2] === 0x5c && serialized[3] > 1) {
          const error = new Error("Invalid host object tag");
          error.code = "ERR_INVALID_STATE";
          throw error;
        }
        const graph = JSON.parse(serialized.toString("utf8"));
        const values = graph.nodes.map(node => {
          switch (node.type) {
            case "Buffer": return Buffer.from(node.data, "base64");
            case "View": {
              const ctor = globalThis[node.name];
              const bytes = Buffer.from(node.data, "base64");
              if (ctor === DataView) {
                const buffer = new ArrayBuffer(node.byteLength);
                new Uint8Array(buffer).set(bytes);
                return new DataView(buffer);
              }
              if (typeof ctor === "function") {
                const view = new ctor(node.length);
                new Uint8Array(view.buffer, view.byteOffset, view.byteLength).set(bytes);
                return view;
              }
              return new Uint8Array(bytes);
            }
            case "ArrayBuffer": return Buffer.from(node.data, "base64").buffer;
            case "Array": return [];
            case "Map": return new Map();
            case "Set": return new Set();
            case "Date": return new Date(node.value);
            case "RegExp": return new RegExp(node.source, node.flags);
            case "Error": {
              const Ctor = ({ Error, TypeError, RangeError, SyntaxError, ReferenceError, URIError, EvalError })[node.name] || Error;
              return node.name === "AggregateError" ? new AggregateError([], node.message) : new Ctor(node.message);
            }
            case "DOMException": {
              const error = new Error();
              delete error.message;
              return error;
            }
            default: return {};
          }
        });
        function decode(value) {
          if (value === null || typeof value !== "object") return value;
          if (Object.hasOwn(value, "$ref")) return values[value.$ref];
          if (value.$type === "undefined") return undefined;
          if (value.$type === "bigint") return BigInt(value.value);
          if (value.$type === "number") {
            if (value.value === "NaN") return NaN;
            if (value.value === "Infinity") return Infinity;
            if (value.value === "-Infinity") return -Infinity;
            return -0;
          }
          return value;
        }
        graph.nodes.forEach((node, index) => {
          const target = values[index];
          if (node.type === "Array") {
            node.values.forEach((item, position) => { target[position] = decode(item); });
          } else if (node.type === "Map") {
            for (const [key, item] of node.values) target.set(decode(key), decode(item));
          } else if (node.type === "Set") {
            for (const item of node.values) target.add(decode(item));
          } else if (node.type === "Error") {
            Object.defineProperty(target, "stack", { value: node.stack, writable: true, configurable: true });
            if (node.hasCause)
              Object.defineProperty(target, "cause", { value: decode(node.cause), writable: true, configurable: true });
            if (node.name === "AggregateError")
              Object.defineProperty(target, "errors", { value: decode(node.errors), writable: true, configurable: true });
          } else if (node.type === "DOMException") {
            Object.defineProperty(target, "stack", { value: node.stack, writable: true, configurable: true });
          } else if (node.type === "Object") {
            for (const [key, item] of node.props || []) Object.defineProperty(target, key, { value: decode(item), enumerable: true, writable: true, configurable: true });
          }
        });
        return decode(graph.root);
      }
      return { serialize, deserialize };
    }
    case "messaging": {
      const native = __nativeInternalBinding("messaging");
      if (native.DOMException) return native;
      return Object.assign({}, native, { DOMException: globalThis.DOMException });
    }
    case "performance": return __nativeInternalBinding("performance");
    case "uv": return __nativeInternalBinding("uv");
    case "async_wrap": {
      // Node's internal/async_hooks.js expects this engine-owned binding even
      // when it is loaded from the configured Node library source. Quench
      // keeps async identity in its host adapter, so project the public API
      // into the narrow subset consumed by the Node source module.
      const hooks = globalThis["\0quench:require"]("async_hooks");
      const async_hook_fields = new Uint32Array(9);
      const async_id_fields = new Float64Array(4);
      const execution_async_resources = [];
      const async_ids_stack = new Float64Array(128);
      const constants = {
        kInit: 0, kBefore: 1, kAfter: 2, kDestroy: 3,
        kPromiseResolve: 4, kTotals: 5, kCheck: 6,
        kExecutionAsyncId: 0, kTriggerAsyncId: 1,
        kAsyncIdCounter: 2, kDefaultTriggerAsyncId: 3,
        kStackLength: 7, kUsesExecutionAsyncResource: 8,
      };
      async_id_fields[constants.kExecutionAsyncId] = hooks.executionAsyncId();
      async_id_fields[constants.kTriggerAsyncId] = hooks.triggerAsyncId();
      return {
        constants, async_hook_fields, async_id_fields,
        execution_async_resources, async_ids_stack,
        Providers: Object.freeze({}),
        setCallbackTrampoline() {},
        registerDestroyHook() {},
        queueDestroyAsyncId(asyncId) {
          hooks["\0quench:async_hooks:queueDestroyAsyncId"](asyncId);
        },
        pushAsyncContext(asyncId, triggerAsyncId, resource) {
          async_id_fields[constants.kExecutionAsyncId] = asyncId;
          async_id_fields[constants.kTriggerAsyncId] = triggerAsyncId;
          execution_async_resources.push(resource);
        },
        popAsyncContext() {
          execution_async_resources.pop();
          async_id_fields[constants.kExecutionAsyncId] = hooks.executionAsyncId();
          async_id_fields[constants.kTriggerAsyncId] = hooks.triggerAsyncId();
        },
        executionAsyncResource() {
          return execution_async_resources[execution_async_resources.length - 1] ?? hooks.executionAsyncResource();
        },
        clearAsyncIdStack() {
          async_id_fields[constants.kExecutionAsyncId] = hooks.executionAsyncId();
          async_id_fields[constants.kTriggerAsyncId] = hooks.triggerAsyncId();
          execution_async_resources.length = 0;
        },
      };
    }
    case "task_queue": return {
      enqueueMicrotask: globalThis["\0quench:queueMicrotaskRaw"] ?? queueMicrotask,
    };
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
    let line_offset = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
        .map_err(|_| RootedError::host("realm wrapper line origin exceeds compiler positions"))?;
    let column_offset = u32::try_from(
        prefix
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count(),
    )
    .map_err(|_| RootedError::host("realm wrapper column origin exceeds compiler positions"))?;
    let shared = context.host_mut().shared_state();
    let cached = shared
        .borrow()
        .realm_module_programs
        .get(&path)
        .filter(|cached| cached.source.as_ref() == wrapper)
        .map(|cached| cached.script.clone());
    let script = match cached.filter(|script| context.prepared_script_is_current(script)) {
        Some(script) => script,
        None => {
            let script = context.prepare_specialized_script_in_realm_with_origin(
                global,
                &wrapper,
                &path.to_string_lossy(),
                line_offset,
                column_offset,
            )?;
            shared.borrow_mut().realm_module_programs.insert(
                path.clone(),
                PreparedRealmModule {
                    source: std::rc::Rc::from(wrapper),
                    script: script.clone(),
                },
            );
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
    let env = get(context, process, "env")?;
    let worker_id = get(context, env, "NODE_UNIQUE_ID")?;
    if !context.truthy_rooted(worker_id)? {
        return Ok(());
    }
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
