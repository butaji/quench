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

#[cfg(test)]
mod parent_tests {
    use quench_runtime::{Host, HostFunction, HostFunctionId, NativeContext, RootId, RootedError};
    struct TestHost;
    impl Host for TestHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 { 0.0 }
        fn functions(&self) -> &[HostFunction<Self>] {
            quench_runtime::host_functions![global "makeNative" (3) => make, method "deprecated" (0) => super::deprecated_parent::<Self>]
        }
    }
    fn make(c: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
        let warning = c.null_object_rooted()?;
        c.assign_named_property_rooted(warning, "process", args[0])?;
        let mut methods = Vec::new();
        for (index, callback) in args[1..3].iter().enumerate() {
            let data = c.record_rooted(&["warning", "callback"], &[warning, *callback])?;
            let wrapper = c.host_constructor_named_with_data(HostFunctionId(1), "deprecated", data)?;
            let length = c.number(index as f64);
            let key = c.string_rooted("length");
            c.define_data_property_rooted(wrapper, key, length, false, false, true)?;
            c.assign_named_property_rooted(data, "wrapper", wrapper)?;
            methods.push(wrapper);
        }
        c.array_rooted(&methods)
    }
    #[test]
    fn parent_warning_is_shared_reentrant_and_committed_before_throw() {
        let program = quench_runtime::Engine::specialize(r#"
          function makeGuest(process, get, set) {
            let warned = false;
            const deprecate = (fn, length) => {
              const wrapper = function deprecated(...args) {
              if (!warned && (process.execArgv.includes('--pending-deprecation') || process.env.NODE_PENDING_DEPRECATION === '1')) {
                warned = true;
                process.emitWarning('module.parent is deprecated due to accuracy issues. Please use require.main to find program entry point instead.',
                  'DeprecationWarning','DEP0144',wrapper);
              }
              return Reflect.apply(fn, this, args);
              };
              Object.defineProperty(wrapper, 'length', {value:length});
              return wrapper;
            };
            return [deprecate(get,0), deprecate(set,1)];
          }
          function run(make, mode) {
            const log = [], receiver = {}, token = {}, flags = [], env = {};
            let pair;
            const process = {
              get execArgv() { log.push('flags'); return flags; },
              get env() { log.push('env'); return env; },
              emitWarning(message, type, code, wrapper) {
                log.push(['warning',this===process,message,type,code,wrapper===pair[0]]);
                if(mode==='throw') throw token;
                if(mode==='reenter') pair[1].call(receiver, 'inner');
              }
            };
            pair = make(process,
              function(...args) { log.push(['get',this===receiver,args]); return token; },
              function(...args) { log.push(['set',this===receiver,args]); return token; });
            log.push(pair[0].name,pair[0].length,pair[1].name,pair[1].length);
            log.push(pair[0].call(receiver, 'before')===token);
            if(mode==='env') env.NODE_PENDING_DEPRECATION='1';
            else flags.push('--pending-deprecation');
            try { log.push(pair[0].call(receiver,'warn')===token); }
            catch(e) { log.push(['throw',e===token]); }
            log.push(pair[1].call(receiver,'after')===token);
            return JSON.stringify(log);
          }
          for(const mode of ['flag','env','throw','reenter']) {
            const expected=run(makeGuest,mode), actual=run(makeNative,mode);
            if(actual!==expected) throw Error(actual+' != '+expected);
          }
        "#, "native-parent-warning.js").unwrap();
        quench_runtime::Runtime::new(TestHost).execute(&program).unwrap();
    }
}

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
    let process = get(context, global, "process")?;
    let warning = context.null_object_rooted()?;
    context.assign_named_property_rooted(warning, "process", process)?;
    let wrap = |context: &mut NativeContext<'_, NodeHost>, callback, operation| {
        let data = context.record_rooted(&["warning", "callback"], &[warning, callback])?;
        let wrapper = context.host_constructor_named_with_data(crate::host::shared_vm::operation(operation), "deprecated", data)?;
        context.assign_named_property_rooted(data, "wrapper", wrapper)?;
        Ok::<_, RootedError>(wrapper)
    };
    let parent_get = wrap(context, parent_get, "moduleParentDeprecated")?;
    let parent_set = wrap(context, parent_set, "moduleParentDeprecatedSet")?;
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
    let create_require = context.host_constructor_named(crate::host::shared_vm::operation("createRequire"), "createRequire")?;
    let paths = context.host_constructor_named(crate::host::shared_vm::operation("moduleNodePaths"), "")?;
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
    let process = get(context, global, "process")?;
    let path_module = crate::modules::path_shared_vm::module(context)?;
    let path_delimiter = get(context, path_module, "delimiter")?;
    let initialize = context.host_function_with_data(crate::host::shared_vm::operation("moduleCompileCacheInitialize"), module)?;
    context.define_lazy_data_properties_rooted(module, &["constants", "enableCompileCache", "flushCompileCache", "getCompileCacheDir"], true, initialize)?;
    super::module_paths_shared_vm::install(context, module, process, path_module, path_delimiter)?;
    let resolve_filename =
        context.host_function(crate::host::shared_vm::operation("_resolveFilename"))?;
    set(context, module, "_resolveFilename", resolve_filename)?;
    let find_package_json =
        context.host_function(crate::host::shared_vm::operation("findPackageJSON"))?;
    set(context, module, "findPackageJSON", find_package_json)?;
    Ok(module)
}

pub(crate) fn deprecated_parent<H: quench_runtime::Host>(context: &mut NativeContext<'_, H>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let warning = context.get_named_property_rooted(data, "warning")?;
    let warned = context.has_own_property_rooted(warning, "warned")?;
    if !warned {
        let process = context.get_named_property_rooted(warning, "process")?;
        let flags = context.get_named_property_rooted(process, "execArgv")?;
        let includes = context.get_named_property_rooted(flags, "includes")?;
        let flag = context.string_rooted("--pending-deprecation");
        let enabled = context.call_rooted(includes, flags, &[flag])?;
        let enabled = if context.truthy_rooted(enabled)? { true } else {
            let env = context.get_named_property_rooted(process, "env")?;
            let setting = context.get_named_property_rooted(env, "NODE_PENDING_DEPRECATION")?;
            let one = context.string_rooted("1");
            context.strict_equal_rooted(setting, one)?
        };
        if enabled {
            let yes = context.boolean(true);
            context.assign_named_property_rooted(warning, "warned", yes)?;
            let emit = context.get_named_property_rooted(process, "emitWarning")?;
            let message = context.string_rooted("module.parent is deprecated due to accuracy issues. Please use require.main to find program entry point instead.");
            let kind = context.string_rooted("DeprecationWarning");
            let code = context.string_rooted("DEP0144");
            let wrapper = context.get_named_property_rooted(data, "wrapper")?;
            context.call_rooted(emit, process, &[message, kind, code, wrapper])?;
        }
    }
    let callback = context.get_named_property_rooted(data, "callback")?;
    context.call_rooted(callback, receiver, args)
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

pub(crate) fn initialize_compile_cache_exports(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
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
            let wrap = crate::host::removed_javascript("node:module/compile-cache-validation.js")?;
            let this_value = context.undefined();
            let constants = get(context, helpers, "constants")?;
            value = context.call_rooted(wrap, this_value, &[value, constants, process])?;
        } else if name == "flushCompileCache" {
            // With no native cache backend there is nothing to flush. Preserve
            // the public API's void return and avoid exposing an internal
            // missing-binding TypeError.
            let wrap = crate::host::removed_javascript("node:module/compile-cache-flush.js")?;
            let this_value = context.undefined();
            value = context.call_rooted(wrap, this_value, &[value])?;
        } else if name == "getCompileCacheDir" {
            // The modules binding reports an active directory only when a
            // cache backend actually enabled one. NODE_COMPILE_CACHE alone
            // cannot make this runtime claim that a bytecode cache is active.
            let wrap = crate::host::removed_javascript("node:module/compile-cache-env.js")?;
            let this_value = context.undefined();
            value = context.call_rooted(wrap, this_value, &[value, process])?;
        }
        set(context, module, name, value)?;
    }
    Ok(context.undefined())
}
