use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};

const REQUIRE_FACADES: &str = quench_js_check::checked_js!(
    r#"(requireNative, pathsNative) => {
      'use strict';
      return [
      function createRequire(filename) { return requireNative(filename); },
      function(from) { return pathsNative(from); }
    ]; }"#
);

pub(crate) const BUILTIN_MODULES: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_tls_common",
    "_tls_wrap",
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
    "node:sea",
    "node:sqlite",
    "node:test",
    "node:test/reporters",
];

const NODE_ONLY_BUILTINS: &[&str] = &["sea", "sqlite", "test", "test/reporters"];

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let weak_map = get(context, global, "WeakMap")?;
    let parents = context.construct_rooted(weak_map, weak_map, &[])?;
    let module = context.host_constructor_with_data(
        crate::host::shared_vm::operation("moduleConstruct"),
        parents,
    )?;
    name_function(context, module, "Module")?;
    crate::host::shared_vm::commonjs::attach_module_cache(context, module)?;
    set(context, module, "Module", module)?;
    let prototype = get(context, module, "prototype")?;
    let constructor = context.host_function_with_data(
        crate::host::shared_vm::operation("moduleConstructorGet"),
        module,
    )?;
    name_function(context, constructor, "get")?;
    accessor(context, prototype, "constructor", constructor, None)?;
    let parent_get = context.host_function_with_data(
        crate::host::shared_vm::operation("moduleParentGet"),
        parents,
    )?;
    let parent_set = context.host_function_with_data(
        crate::host::shared_vm::operation("moduleParentSet"),
        parents,
    )?;
    let native_parent_set = parent_set;
    let deprecate_parent = context.evaluate_script_rooted(
        r#"(get, set, process) => {
          let warned = false;
          const deprecate = (fn) => function deprecated(...args) {
            if (!warned && (process.execArgv.includes('--pending-deprecation') ||
                process.env.NODE_PENDING_DEPRECATION === '1')) {
              warned = true;
              process.emitWarning(
                'module.parent is deprecated due to accuracy issues. Please use require.main to find program entry point instead.',
                { type: 'DeprecationWarning', code: 'DEP0144' });
            }
            return Reflect.apply(fn, this, args);
          };
          return [deprecate(get), deprecate(set)];
        }"#,
        "node:module/parent-deprecation.js",
    )?;
    let process = get(context, global, "process")?;
    let undefined = context.undefined();
    let deprecated =
        context.call_rooted(deprecate_parent, undefined, &[parent_get, parent_set, process])?;
    let parent_get = get(context, deprecated, "0")?;
    let parent_set = get(context, deprecated, "1")?;
    name_function(context, parent_get, "deprecated")?;
    name_function(context, parent_set, "deprecated")?;
    accessor(context, prototype, "parent", parent_get, Some(parent_set))?;
    crate::host::shared_vm::commonjs::register_module_parent_accessors(
        context,
        prototype,
        native_parent_set,
    )?;
    for (name, operation) in [("require", "moduleRequire"), ("_compile", "moduleCompile")] {
        let method = context.host_constructor(crate::host::shared_vm::operation(operation))?;
        name_function(context, method, name)?;
        set(context, prototype, name, method)?;
    }
    let stream_iter = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--experimental-stream-iter");
    let experimental_bench = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--experimental-bench");
    let mut names = BUILTIN_MODULES
        .iter()
        .copied()
        .filter(|name| !name.starts_with("node:"))
        .chain(stream_iter.then_some("stream/iter"))
        .chain(stream_iter.then_some("zlib/iter"))
        .chain(experimental_bench.then_some("node:bench"))
        .chain(experimental_bench.then_some("node:bench/reporters"))
        .chain(
            BUILTIN_MODULES
                .iter()
                .copied()
                .filter(|name| name.starts_with("node:")),
        )
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    if context.host_mut().vfs_enabled() {
        names.push(context.string_rooted("node:vfs"));
    }
    let names = context.array_rooted(&names)?;
    set(context, module, "builtinModules", names)?;
    let is_builtin = context.host_function(crate::host::shared_vm::operation("isBuiltin"))?;
    set(context, module, "isBuiltin", is_builtin)?;
    let require_native =
        context.host_function(crate::host::shared_vm::operation("createRequire"))?;
    let paths_native =
        context.host_function(crate::host::shared_vm::operation("moduleNodePaths"))?;
    let stat = context.host_function(crate::host::shared_vm::operation(
        "fsBindingInternalModuleStat",
    ))?;
    let factory =
        context.evaluate_script_rooted(REQUIRE_FACADES, "node:module/require-facades.js")?;
    let undefined = context.undefined();
    let facades = context.call_rooted(factory, undefined, &[require_native, paths_native])?;
    let create_require = get(context, facades, "0")?;
    let paths = get(context, facades, "1")?;
    set(context, module, "createRequire", create_require)?;
    set(context, module, "_nodeModulePaths", paths)?;
    let global_paths = global_paths()
        .iter()
        .map(|path| context.string_rooted(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    let global_paths = context.array_rooted(&global_paths)?;
    set(context, module, "globalPaths", global_paths)?;
    let run_main = context.host_function(crate::host::shared_vm::operation("moduleRunMain"))?;
    set(context, module, "runMain", run_main)?;
    set(context, module, "_stat", stat)?;
    let process = get(context, global, "process")?;
    let path_module = crate::modules::path_shared_vm::module(context)?;
    let path_delimiter = get(context, path_module, "delimiter")?;
    // The public `node:module` builtin is constructed here rather than from
    // lib/module.js, so its compile-cache methods must explicitly forward to
    // the same internal helpers used by the Node library and module loaders.
    // Keeping a second native stub here shadowed those helpers with an
    // always-failed API and bypassed their diagnostics and directory state.
    let helpers = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "internal/modules/helpers.js",
        "internal/modules/helpers.js",
    )?;
    for name in [
        "constants",
        "enableCompileCache",
        "flushCompileCache",
        "getCompileCacheDir",
    ] {
        let key = context.string_rooted(name);
        let mut value = context.get_property_rooted(helpers, key)?;
        if name == "enableCompileCache" {
            // Node's native modules binding validates the cache directory
            // type. Quench does not yet implement native compile-cache entry
            // points, so preserve this observable validation at the public
            // API boundary rather than leaking a generic call error.
            let wrap = context.evaluate_script_rooted(
                r#"(nativeEnableCompileCache, constants, process) => function enableCompileCache(options) {
                  if (options === null ||
                      (options !== undefined && typeof options !== 'string' && typeof options !== 'object')) {
                    const error = new TypeError('The "directory" argument must be of type string.');
                    error.code = 'ERR_INVALID_ARG_TYPE';
                    throw error;
                  }
                  const directory = typeof options === 'object' ? options.directory : options;
                  if (directory !== undefined && typeof directory !== 'string') {
                    const error = new TypeError('The "directory" argument must be of type string.');
                    error.code = 'ERR_INVALID_ARG_TYPE';
                    throw error;
                  }
                  if (process.env.NODE_DISABLE_COMPILE_CACHE) {
                    if ((process.env.NODE_DEBUG_NATIVE || '').includes('COMPILE_CACHE')) {
                      process.stderr.write('Disabled by NODE_DISABLE_COMPILE_CACHE\n');
                    }
                    return { status: constants.compileCacheStatus.DISABLED };
                  }
                  // The API remains available in builds without the native
                  // cache backend. Report the documented failure result rather
                  // than trying to call an absent native binding or claiming
                  // that an on-disk cache was enabled.
                  if (typeof nativeEnableCompileCache !== 'function') {
                    return {
                      status: constants.compileCacheStatus.FAILED,
                      message: 'Compile cache is not supported by this runtime.',
                    };
                  }
                  return nativeEnableCompileCache(options);
                }"#,
                "node:module/compile-cache-validation.js",
            )?;
            let this_value = context.undefined();
            let constants = get(context, helpers, "constants")?;
            value = context.call_rooted(wrap, this_value, &[value, constants, process])?;
        } else if name == "flushCompileCache" {
            // With no native cache backend there is nothing to flush. Preserve
            // the public API's void return and avoid exposing an internal
            // missing-binding TypeError.
            let wrap = context.evaluate_script_rooted(
                r#"(nativeFlushCompileCache) => function flushCompileCache() {
                  if (typeof nativeFlushCompileCache === 'function') {
                    return nativeFlushCompileCache();
                  }
                }"#,
                "node:module/compile-cache-flush.js",
            )?;
            let this_value = context.undefined();
            value = context.call_rooted(wrap, this_value, &[value])?;
        } else if name == "getCompileCacheDir" {
            // The modules binding reports an active directory only when a
            // cache backend actually enabled one. NODE_COMPILE_CACHE alone
            // cannot make this runtime claim that a bytecode cache is active.
            let wrap = context.evaluate_script_rooted(
                r#"(nativeGetCompileCacheDir, process) => function getCompileCacheDir() {
                  if (process.env.NODE_DISABLE_COMPILE_CACHE) return undefined;
                  return typeof nativeGetCompileCacheDir === 'function'
                    ? nativeGetCompileCacheDir()
                    : undefined;
                }"#,
                "node:module/compile-cache-env.js",
            )?;
            let this_value = context.undefined();
            value = context.call_rooted(wrap, this_value, &[value, process])?;
        }
        set(context, module, name, value)?;
    }
    let initialize_paths = context.evaluate_script_rooted(
        "(Module, process, delimiter, path) => {\n\
          Module._initPaths = function _initPaths() {\n\
            const homeDir = process.env.HOME;\n\
            const nodePath = process.env.NODE_PATH;\n\
            let paths = [path.resolve(process.execPath, '..', '..', 'lib', 'node')];\n\
            if (homeDir) paths.unshift(path.resolve(homeDir, '.node_modules'), path.resolve(homeDir, '.node_libraries'));\n\
            if (nodePath) paths = nodePath.split(delimiter).filter(Boolean).concat(paths);\n\
            Module.globalPaths = paths;\n\
          };\n\
          Module._resolveLookupPaths = function _resolveLookupPaths(request, parent) {\n\
            request = String(request);\n\
            const relative = request[0] === '.' && (request.length === 1 || request[1] === '.' || request[1] === '/' || (path.sep === '\\\\' && request[1] === '\\\\'));\n\
            if (!relative && (Module.builtinModules.includes(request) || Module.builtinModules.includes(request.replace(/^node:/, '')))) return null;\n\
            if (!relative) {\n\
              const paths = parent?.paths?.length ? [...parent.paths, ...Module.globalPaths] : Module.globalPaths;\n\
              return paths.length ? [...paths] : null;\n\
            }\n\
            if (!parent || !parent.id || !parent.filename) return ['.'];\n\
            return [path.dirname(parent.filename)];\n\
          };\n\
          Module._load = function _load(request, parent) {\n\
            if (parent && typeof parent.require === 'function') return parent.require(request);\n\
            request = String(request);\n\
            if (Module.builtinModules.includes(request)) return process.getBuiltinModule(request.replace(/^node:/, ''));\n\
            const entry = path.resolve(process.cwd(), '__quench_module_load__.js');\n\
            return Module.createRequire(entry)(request);\n\
          };\n\
        }",
        "node:module/shared-path-facades.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(
        initialize_paths,
        undefined,
        &[module, process, path_delimiter, path_module],
    )?;
    let resolve_filename =
        context.host_function(crate::host::shared_vm::operation("_resolveFilename"))?;
    set(context, module, "_resolveFilename", resolve_filename)?;
    let find_package_json =
        context.host_function(crate::host::shared_vm::operation("findPackageJSON"))?;
    set(context, module, "findPackageJSON", find_package_json)?;
    Ok(module)
}

fn name_function(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    name: &str,
) -> Result<(), RootedError> {
    let key = context.string_rooted("name");
    let name = context.string_rooted(name);
    if context.define_data_property_rooted(function, key, name, false, false, true)? {
        Ok(())
    } else {
        Err(RootedError::host("cannot name Module callable"))
    }
}

fn accessor(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    getter: RootId,
    setter: Option<RootId>,
) -> Result<(), RootedError> {
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    if let Some(setter) = setter {
        set(context, descriptor, "set", setter)?;
    }
    let flag = context.boolean(false);
    set(context, descriptor, "configurable", flag)?;
    set(context, descriptor, "enumerable", flag)?;
    let key = context.string_rooted(name);
    context.define_property_descriptor_rooted(object, key, descriptor)?;
    Ok(())
}

pub(crate) fn construct(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .copied()
        .filter(|id| !context.rooted_value(*id).is_some_and(|v| v.is_undefined()))
        .unwrap_or_else(|| context.string_rooted(""));
    set(context, receiver, "id", id)?;
    let path = crate::modules::path_shared_vm::host_dirname(context, id)?;
    set(context, receiver, "path", path)?;
    let exports = context.object_rooted()?;
    // Exports is an own data property even when a derived prototype defines a setter.
    let key = context.string_rooted("exports");
    context.define_data_property_rooted(receiver, key, exports, true, true, true)?;
    let parent = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let parents = context.host_function_data()?;
    invoke(context, parents, "set", &[receiver, parent])?;
    if context
        .rooted_value(parent)
        .is_some_and(|v| !v.is_null() && !v.is_undefined())
    {
        let children = get(context, parent, "children")?;
        if context.truthy_rooted(children)? {
            let global = context.global_root()?;
            let array = get(context, global, "Array")?;
            let prototype = get(context, array, "prototype")?;
            let push = get(context, prototype, "push")?;
            context.call_rooted(push, children, &[receiver])?;
        }
    }
    let filename = context.null();
    set(context, receiver, "filename", filename)?;
    let loaded = context.boolean(false);
    set(context, receiver, "loaded", loaded)?;
    let children = context.array_rooted(&[])?;
    set(context, receiver, "children", children)?;
    Ok(context.undefined())
}

fn invoke(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(context, receiver, name)?;
    context.call_rooted(function, receiver, args)
}

pub(crate) fn constructor_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.host_function_data()
}

pub(crate) fn parent_get(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let parents = context.host_function_data()?;
    invoke(context, parents, "get", &[receiver])
}

pub(crate) fn parent_set(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let parent = args.first().copied().unwrap_or_else(|| context.undefined());
    let parents = context.host_function_data()?;
    invoke(context, parents, "set", &[receiver, parent])?;
    Ok(context.undefined())
}

pub(crate) fn require(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::require_for_module(context, receiver, args)
}

pub(crate) fn compile(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::compile_public(context, receiver, args)
}

pub(crate) fn find_package_json(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(specifier_value) = args.first().copied() else {
        return Err(coded_module_error(
            context,
            true,
            "ERR_MISSING_ARGS",
            "The \"specifier\" argument must be specified",
        )?);
    };
    let specifier = if context.is_symbol_rooted(specifier_value)? {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            specifier_value,
            "specifier",
            "string",
        )?);
    } else {
        match context.to_string(specifier_value) {
            Ok(specifier) => specifier,
            Err(error) => {
                if let Some(exception) = error.exception {
                    context.release_root(exception);
                }
                return Err(super::util_shared_vm::argument_type_error(
                    context,
                    specifier_value,
                    "specifier",
                    "string",
                )?);
            }
        }
    };
    let base = if let Some(base_value) = args.get(1).copied() {
        if context
            .rooted_value(base_value)
            .is_some_and(|value| value.is_undefined())
        {
            None
        } else if let Some(base) = context.string_text(base_value)? {
            Some(base)
        } else {
            if super::url_shared_vm::is_url_like(context, base_value)? {
                Some(context.to_string(base_value)?)
            } else {
                return Err(super::util_shared_vm::argument_type_error(
                    context, base_value, "base", "string",
                )?);
            }
        }
    } else {
        None
    };

    let base_path = match base.as_deref() {
        Some(base) => match find_package_base(base) {
            Ok(path) => Some(path),
            Err(BasePathError::InvalidUrl) => return Err(invalid_url(context)?),
            Err(BasePathError::InvalidScheme) => return Err(invalid_url_scheme(context)?),
        },
        None => None,
    };
    let package_json = if let Some(path) = file_url_path(&specifier) {
        if !path.exists() {
            return Err(module_not_found(
                context,
                &path,
                base_path.as_ref().map(|base| base.path.as_path()),
            )?);
        }
        package_scope(&path)
    } else if has_url_scheme(&specifier) {
        return if specifier.starts_with("file:") {
            Err(invalid_url(context)?)
        } else {
            Err(invalid_url_scheme(context)?)
        };
    } else if PathBuf::from(&specifier).is_absolute() {
        match base_path.as_ref() {
            Some(base) => {
                let path = PathBuf::from(specifier);
                if !path.exists() {
                    return Err(module_not_found(context, &path, Some(&base.path))?);
                }
                package_scope(&path)
            }
            None => return Err(unsupported_resolve_request(context, &specifier)?),
        }
    } else if is_bare_specifier(&specifier) {
        let Some(base_path) = base_path else {
            return Err(unsupported_resolve_request(context, &specifier)?);
        };
        match resolve_package_json(&base_path, &specifier) {
            Some(path) => Some(path),
            None => return Err(package_not_found(context, &specifier, &base_path.path)?),
        }
    } else {
        let Some(base_path) = base_path else {
            return Err(unsupported_resolve_request(context, &specifier)?);
        };
        let base_dir = if base_path.is_directory {
            base_path.path.clone()
        } else {
            base_path
                .path
                .parent()
                .unwrap_or(&base_path.path)
                .to_path_buf()
        };
        let resolved = base_dir.join(&specifier);
        if resolved.exists() {
            package_scope(&resolved)
        } else {
            return Err(module_not_found(context, &resolved, Some(&base_path.path))?);
        }
    };

    match package_json {
        Some(path) => {
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            Ok(context.string_rooted(&path.to_string_lossy()))
        }
        None => Ok(context.undefined()),
    }
}

enum BasePathError {
    InvalidUrl,
    InvalidScheme,
}

struct PackageBase {
    path: PathBuf,
    is_directory: bool,
}

fn find_package_base(base: &str) -> Result<PackageBase, BasePathError> {
    if has_url_scheme(base) {
        let url = url::Url::parse(base).map_err(|_| BasePathError::InvalidUrl)?;
        if url.scheme() != "file" {
            return Err(BasePathError::InvalidScheme);
        }
        return Ok(PackageBase {
            path: url.to_file_path().map_err(|_| BasePathError::InvalidUrl)?,
            is_directory: url.path().ends_with('/'),
        });
    }
    let path = PathBuf::from(base);
    if !path.is_absolute() {
        return Err(BasePathError::InvalidUrl);
    }
    Ok(PackageBase {
        path,
        is_directory: base.ends_with(std::path::MAIN_SEPARATOR),
    })
}

fn file_url_path(value: &str) -> Option<PathBuf> {
    let url = url::Url::parse(value).ok()?;
    (url.scheme() == "file")
        .then(|| url.to_file_path().ok())
        .flatten()
}

fn has_url_scheme(value: &str) -> bool {
    value.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    })
}

fn is_bare_specifier(value: &str) -> bool {
    !value.starts_with('.') && !value.starts_with('/') && !PathBuf::from(value).is_absolute()
}

fn resolve_package_json(base: &PackageBase, specifier: &str) -> Option<PathBuf> {
    let package_name = if specifier.starts_with('@') {
        let mut parts = specifier.split('/');
        let scope = parts.next()?;
        let name = parts.next()?;
        format!("{scope}/{name}")
    } else {
        specifier.split('/').next()?.to_owned()
    };
    let mut directory = if base.is_directory {
        base.path.clone()
    } else {
        base.path.parent()?.to_path_buf()
    };
    loop {
        let package = directory.join("node_modules").join(&package_name);
        let manifest = package.join("package.json");
        if manifest.is_file() {
            return Some(manifest);
        }
        if !directory.pop() {
            return None;
        }
    }
}

fn package_scope(path: &std::path::Path) -> Option<PathBuf> {
    let mut directory = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        let manifest = directory.join("package.json");
        if manifest.is_file() {
            return Some(manifest);
        }
        if !directory.pop() {
            return None;
        }
    }
}

fn coded_module_error(
    context: &mut NativeContext<'_, NodeHost>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = if type_error {
        context.type_error_rooted(message)?
    } else {
        context.error_rooted(message)?
    };
    let code = context.string_rooted(code);
    set(context, error, "code", code)?;
    Ok(context.throw(error))
}

fn invalid_url(context: &mut NativeContext<'_, NodeHost>) -> Result<RootedError, RootedError> {
    coded_module_error(context, true, "ERR_INVALID_URL", "Invalid URL")
}

fn invalid_url_scheme(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootedError, RootedError> {
    coded_module_error(
        context,
        true,
        "ERR_INVALID_URL_SCHEME",
        "The URL must be of scheme file",
    )
}

fn unsupported_resolve_request(
    context: &mut NativeContext<'_, NodeHost>,
    specifier: &str,
) -> Result<RootedError, RootedError> {
    coded_module_error(
        context,
        true,
        "ERR_UNSUPPORTED_RESOLVE_REQUEST",
        &format!(
            "Failed to resolve module specifier \"{specifier}\" from \"data:\": Invalid relative URL or base scheme is not hierarchical."
        ),
    )
}

fn module_not_found(
    context: &mut NativeContext<'_, NodeHost>,
    path: &std::path::Path,
    base: Option<&Path>,
) -> Result<RootedError, RootedError> {
    let from = base
        .map(|base| base.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data:".to_owned());
    coded_module_error(
        context,
        false,
        "ERR_MODULE_NOT_FOUND",
        &format!(
            "Cannot find module '{}' imported from {from}",
            path.display()
        ),
    )
}

fn package_not_found(
    context: &mut NativeContext<'_, NodeHost>,
    specifier: &str,
    base: &Path,
) -> Result<RootedError, RootedError> {
    coded_module_error(
        context,
        false,
        "ERR_MODULE_NOT_FOUND",
        &format!(
            "Cannot find package '{specifier}' imported from {}",
            base.display()
        ),
    )
}

pub(crate) fn create_require(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    // Node deliberately tests this URL-like shape before string coercion.
    // Getter order and exceptions here are outside the conversion catch.
    let url_like = super::url_shared_vm::is_url_like(context, input)?;
    let text = context.string_text(input)?;
    let filename = if url_like
        || text
            .as_ref()
            .is_some_and(|text| !Path::new(text).is_absolute())
    {
        let conversion = (|| {
            let text = context.to_string(input)?;
            let text = context.string_rooted(&text);
            let undefined = context.undefined();
            let filename = super::url_shared_vm::file_url_to_path(context, undefined, &[text])?;
            context.to_string(filename)
        })();
        match conversion {
            Ok(filename) => filename,
            Err(error) => {
                if let Some(exception) = error.exception {
                    context.release_root(exception);
                }
                return Err(super::util_shared_vm::argument_value_error(
                    context,
                    input,
                    "filename",
                    "must be a file URL object, file URL string, or absolute path string",
                )?);
            }
        }
    } else if let Some(text) = text {
        text
    } else {
        return Err(super::util_shared_vm::argument_value_error(
            context,
            input,
            "filename",
            "must be a file URL object, file URL string, or absolute path string",
        )?);
    };
    // A trailing separator gives the require a directory, rather than its parent.
    let trailing_separator = filename.ends_with('/') || (cfg!(windows) && filename.ends_with('\\'));
    let filename = if trailing_separator {
        PathBuf::from(filename).join("noop.js")
    } else {
        PathBuf::from(filename)
    };
    crate::host::shared_vm::commonjs::create_require(context, &filename)
}

/// One projection of ancestor search directories, shared with CJS module records.
pub(crate) fn node_search_paths(directory: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    directory
        .ancestors()
        .filter(|path| path.file_name().is_none_or(|name| name != "node_modules"))
        .map(|path| path.join("node_modules"))
}

/// Node's `modulePaths` (lib/internal/modules/cjs/loader.js `initPaths`): `NODE_PATH`'s
/// entries as given, then `$HOME/.node_modules`, `$HOME/.node_libraries` and
/// `$PREFIX/lib/node`, the executable being `$PREFIX/bin/node`. Bare requires search them after
/// the requester's `node_modules` chain; `Module.globalPaths` is a copy.
pub(crate) fn global_paths() -> Vec<PathBuf> {
    let node_path = std::env::var_os("NODE_PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(&node_path).filter(|path| !path.as_os_str().is_empty()).collect();
    if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
        let home = std::path::absolute(home).unwrap_or_default();
        paths.extend([home.join(".node_modules"), home.join(".node_libraries")]);
    }
    let executable = crate::shared_run::executable_path().unwrap_or_default();
    let prefix = executable.ancestors().nth(EXECUTABLE_PREFIX_DEPTH).unwrap_or(Path::new("/"));
    paths.push(prefix.join("lib").join("node"));
    paths
}

/// `$PREFIX/bin/node`: the prefix is the executable's second ancestor.
const EXECUTABLE_PREFIX_DEPTH: usize = 2;

pub(crate) fn node_module_paths(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = super::path_shared_vm::module(context)?;
    let resolve = get(context, path, "resolve")?;
    let resolved = context.call_rooted(resolve, path, &[input])?;
    let resolved = context.to_string(resolved)?;
    let roots = node_search_paths(Path::new(&resolved))
        .map(|path| context.string_rooted(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}

pub(crate) fn is_builtin(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let vfs_enabled = context.host_mut().vfs_enabled();
    let stream_iter = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--experimental-stream-iter");
    let experimental_bench = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--experimental-bench");
    let result = args
        .first()
        .and_then(|argument| context.string_text(*argument).ok().flatten())
        .is_some_and(|specifier| {
            // These experimental modules are intentionally only exposed with
            // the `node:` prefix. `isBuiltin('bench')` must remain false even
            // when `node:bench` is enabled, like Node's public builtin list.
            if matches!(specifier.as_str(), "node:bench" | "node:bench/reporters") {
                return experimental_bench;
            }
            if specifier == "node:vfs" {
                return vfs_enabled;
            }
            if matches!(
                specifier.as_str(),
                "stream/iter" | "node:stream/iter" | "zlib/iter" | "node:zlib/iter"
            ) {
                return stream_iter;
            }
            if let Some(name) = specifier.strip_prefix("node:") {
                BUILTIN_MODULES.contains(&specifier.as_str())
                    || BUILTIN_MODULES.contains(&name)
                    || NODE_ONLY_BUILTINS.contains(&name)
            } else {
                BUILTIN_MODULES.contains(&specifier.as_str())
            }
        });
    Ok(context.boolean(result))
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install module.{name}")))
    }
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}
