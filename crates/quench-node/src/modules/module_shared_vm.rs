use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};


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
    // Both accessors share one warned flag: DEP0144 is emitted once.
    let warned = context.null_object_rooted()?;
    let mut deprecated = Vec::new();
    for target in [parent_get, parent_set] {
        let data = context.null_object_rooted()?;
        set(context, data, "target", target)?;
        set(context, data, "warned", warned)?;
        deprecated.push(context.host_function_named_with_data(
            crate::host::shared_vm::operation("moduleParentDeprecated"),
            "deprecated",
            data,
        )?);
    }
    let (parent_get, parent_set) = (deprecated[0], deprecated[1]);
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
    let create_require =
        context.host_function_named(crate::host::shared_vm::operation("createRequire"), "createRequire")?;
    let paths = context.host_function_named(crate::host::shared_vm::operation("moduleNodePaths"), "")?;
    let stat = context.host_function(crate::host::shared_vm::operation(
        "fsBindingInternalModuleStat",
    ))?;
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
    let constants = context.object_rooted()?;
    let status = context.object_rooted()?;
    for (name, value) in COMPILE_CACHE_STATUS {
        let value = context.number(*value);
        set(context, status, name, value)?;
    }
    set(context, constants, "compileCacheStatus", status)?;
    set(context, module, "constants", constants)?;
    for (name, operation) in [
        ("enableCompileCache", "moduleEnableCompileCache"),
        ("flushCompileCache", "moduleFlushCompileCache"),
        ("getCompileCacheDir", "moduleGetCompileCacheDir"),
    ] {
        let function = context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        set(context, module, name, function)?;
    }
    for (name, operation) in [
        ("_initPaths", "moduleInitPaths"),
        ("_resolveLookupPaths", "moduleResolveLookupPaths"),
        ("_load", "moduleLoad"),
    ] {
        let function = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, module)?;
        set(context, module, name, function)?;
    }
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
        } else if super::url_shared_vm::is_url_like(context, base_value)? {
            Some(context.to_string(base_value)?)
        } else {
            return Err(super::util_shared_vm::argument_type_error(
                context, base_value, "base", "string",
            )?);
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

/// `module.constants.compileCacheStatus`.
const COMPILE_CACHE_STATUS: &[(&str, f64)] = &[("FAILED", 0.0), ("ENABLED", 1.0), ("ALREADY_ENABLED", 2.0), ("DISABLED", 3.0)];
const COMPILE_CACHE_FAILED: f64 = 0.0;
const COMPILE_CACHE_DISABLED: f64 = 3.0;

fn process_env(context: &mut NativeContext<'_, NodeHost>, name: &str) -> Result<Option<String>, RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let env = get(context, process, "env")?;
    let value = get(context, env, name)?;
    if context.rooted_value(value).is_some_and(|value| value.is_undefined()) {
        return Ok(None);
    }
    context.to_string(value).map(Some)
}

/// `module.parent`'s deprecated accessors: DEP0144 once, under `--pending-deprecation` or
/// `NODE_PENDING_DEPRECATION=1`, then the accessor itself.
pub(crate) fn parent_deprecated(context: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let warned = get(context, data, "warned")?;
    let pending = context.host_mut().shared_state().borrow().exec_argv.iter().any(|flag| flag == "--pending-deprecation")
        || process_env(context, "NODE_PENDING_DEPRECATION")?.as_deref() == Some("1");
    if pending && !context.has_own_property_rooted(warned, "done")? {
        let yes = context.boolean(true);
        set(context, warned, "done", yes)?;
        let global = context.global_root()?;
        let process = get(context, global, "process")?;
        let emit = get(context, process, "emitWarning")?;
        let message = context.string_rooted("module.parent is deprecated due to accuracy issues. Please use require.main to find program entry point instead.");
        let options = context.object_rooted()?;
        let kind = context.string_rooted("DeprecationWarning");
        set(context, options, "type", kind)?;
        let code = context.string_rooted("DEP0144");
        set(context, options, "code", code)?;
        context.call_rooted(emit, process, &[message, options])?;
    }
    let target = get(context, data, "target")?;
    context.call_rooted(target, this, args)
}

fn compile_cache_result(context: &mut NativeContext<'_, NodeHost>, status: f64, message: Option<&str>) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    let status = context.number(status);
    set(context, result, "status", status)?;
    if let Some(message) = message {
        let message = context.string_rooted(message);
        set(context, result, "message", message)?;
    }
    Ok(result)
}

/// `module.enableCompileCache([directory | options])`: quench has no compile cache, so a
/// valid request reports `FAILED`, or `DISABLED` under `NODE_DISABLE_COMPILE_CACHE`.
pub(crate) fn enable_compile_cache(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = match args.first() {
        Some(options) => *options,
        None => context.undefined(),
    };
    let invalid = |context: &mut NativeContext<'_, NodeHost>| -> Result<RootedError, RootedError> {
        let error = context.type_error_rooted("The \"directory\" argument must be of type string.")?;
        let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
        set(context, error, "code", code)?;
        Ok(context.throw(error))
    };
    let undefined = context.rooted_value(options).is_some_and(|value| value.is_undefined());
    let null = context.rooted_value(options).is_some_and(|value| value.is_null());
    let object = context.is_object_rooted(options)?;
    if null || (!undefined && !object && context.string_text(options)?.is_none()) {
        return Err(invalid(context)?);
    }
    let directory = if object { get(context, options, "directory")? } else { options };
    if !context.rooted_value(directory).is_some_and(|value| value.is_undefined()) && context.string_text(directory)?.is_none() {
        return Err(invalid(context)?);
    }
    if process_env(context, "NODE_DISABLE_COMPILE_CACHE")?.is_some_and(|value| !value.is_empty()) {
        if process_env(context, "NODE_DEBUG_NATIVE")?.unwrap_or_default().contains("COMPILE_CACHE") {
            eprintln!("Disabled by NODE_DISABLE_COMPILE_CACHE");
        }
        return compile_cache_result(context, COMPILE_CACHE_DISABLED, None);
    }
    compile_cache_result(context, COMPILE_CACHE_FAILED, Some("Compile cache is not supported by this runtime."))
}

/// `module.flushCompileCache()`: nothing to flush.
pub(crate) fn flush_compile_cache(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

/// `module.getCompileCacheDir()`: no cache directory is ever active.
pub(crate) fn get_compile_cache_dir(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

fn path_call(context: &mut NativeContext<'_, NodeHost>, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = crate::modules::path_shared_vm::module(context)?;
    let function = get(context, path, name)?;
    context.call_rooted(function, path, args)
}

/// `Module._initPaths()`: `globalPaths` from `HOME`, `NODE_PATH` and the executable's prefix.
pub(crate) fn init_paths(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let exec_path = get(context, process, "execPath")?;
    let (up, lib, node) = (context.string_rooted(".."), context.string_rooted("lib"), context.string_rooted("node"));
    let mut paths = vec![path_call(context, "resolve", &[exec_path, up, up, lib, node])?];
    if let Some(home) = process_env(context, "HOME")?.filter(|home| !home.is_empty()) {
        let home = context.string_rooted(&home);
        let modules = context.string_rooted(".node_modules");
        let libraries = context.string_rooted(".node_libraries");
        let first = path_call(context, "resolve", &[home, modules])?;
        let second = path_call(context, "resolve", &[home, libraries])?;
        paths.splice(0..0, [first, second]);
    }
    if let Some(node_path) = process_env(context, "NODE_PATH")?.filter(|path| !path.is_empty()) {
        let delimiter = if cfg!(windows) { ';' } else { ':' };
        let prefix: Vec<RootId> = node_path.split(delimiter).filter(|part| !part.is_empty()).map(|part| context.string_rooted(part)).collect();
        paths.splice(0..0, prefix);
    }
    let paths = context.array_rooted(&paths)?;
    set(context, module, "globalPaths", paths)?;
    Ok(context.undefined())
}

fn builtin_named(context: &mut NativeContext<'_, NodeHost>, module: RootId, request: &str) -> Result<bool, RootedError> {
    let names = get(context, module, "builtinModules")?;
    let names = context.array_like_values_rooted(names)?;
    let bare = request.strip_prefix("node:").unwrap_or(request);
    for name in names {
        let name = context.to_string(name)?;
        if name == request || name == bare {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `Module._resolveLookupPaths(request, parent)`.
pub(crate) fn resolve_lookup_paths(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let undefined = context.undefined();
    let request = args.first().copied().unwrap_or(undefined);
    let parent = args.get(1).copied().unwrap_or(undefined);
    let request = context.to_string(request)?;
    // `.`, `..`, `./…` and `../…` (and `.\…` on Windows) are relative requests.
    let mut characters = request.chars();
    let relative = characters.next() == Some('.')
        && (matches!(characters.next(), None | Some('.') | Some('/')) || (cfg!(windows) && request.starts_with(".\\")));
    if !relative && builtin_named(context, module, &request)? {
        return Ok(context.null());
    }
    let parent_object = context.is_object_rooted(parent)?;
    if !relative {
        let global_paths = get(context, module, "globalPaths")?;
        let mut paths = Vec::new();
        if parent_object {
            let parent_paths = get(context, parent, "paths")?;
            if context.is_object_rooted(parent_paths)? {
                paths.extend(context.array_like_values_rooted(parent_paths)?);
            }
        }
        if paths.is_empty() {
            paths = context.array_like_values_rooted(global_paths)?;
        } else {
            paths.extend(context.array_like_values_rooted(global_paths)?);
        }
        return if paths.is_empty() { Ok(context.null()) } else { context.array_rooted(&paths) };
    }
    let (id, filename) = if parent_object {
        (get(context, parent, "id")?, get(context, parent, "filename")?)
    } else {
        (undefined, undefined)
    };
    if !parent_object || !context.truthy_rooted(id)? || !context.truthy_rooted(filename)? {
        let here = context.string_rooted(".");
        return context.array_rooted(&[here]);
    }
    let directory = path_call(context, "dirname", &[filename])?;
    context.array_rooted(&[directory])
}

/// `Module._load(request, parent)`: through the parent's `require`, else a built-in or a
/// require made for the working directory.
pub(crate) fn load(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let undefined = context.undefined();
    let request = args.first().copied().unwrap_or(undefined);
    let parent = args.get(1).copied().unwrap_or(undefined);
    if context.is_object_rooted(parent)? {
        let require = get(context, parent, "require")?;
        if context.is_callable_rooted(require)? {
            return context.call_rooted(require, parent, &[request]);
        }
    }
    let request_text = context.to_string(request)?;
    let request = context.string_rooted(&request_text);
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    if builtin_named(context, module, &request_text)? {
        let get_builtin = get(context, process, "getBuiltinModule")?;
        let bare = context.string_rooted(request_text.strip_prefix("node:").unwrap_or(&request_text));
        return context.call_rooted(get_builtin, process, &[bare]);
    }
    let cwd = get(context, process, "cwd")?;
    let directory = context.call_rooted(cwd, process, &[])?;
    let entry = context.string_rooted("__quench_module_load__.js");
    let entry = path_call(context, "resolve", &[directory, entry])?;
    let create = get(context, module, "createRequire")?;
    let require = context.call_rooted(create, module, &[entry])?;
    context.call_rooted(require, undefined, &[request])
}
