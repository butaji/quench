use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};

const BUILTIN_MODULES: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_duplex",
    "_stream_passthrough",
    "_stream_readable",
    "_stream_transform",
    "_stream_wrap",
    "_stream_writable",
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

const NODE_ONLY_BUILTINS: &[&str] = &[
    "sea",
    "sqlite",
    "test",
    "test/reporters",
];

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let mut names = BUILTIN_MODULES
        .iter()
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    if vfs_enabled(context) {
        names.push(context.string_rooted("node:vfs"));
    }
    let names = context.array_rooted(&names)?;
    set(context, module, "builtinModules", names)?;
    let is_builtin = context.host_function(crate::host::shared_vm::operation("isBuiltin"))?;
    set(context, module, "isBuiltin", is_builtin)?;
    let create_require =
        context.host_function(crate::host::shared_vm::operation("createRequire"))?;
    set(context, module, "createRequire", create_require)?;
    let find_package_json =
        context.host_function(crate::host::shared_vm::operation("findPackageJSON"))?;
    set(context, module, "findPackageJSON", find_package_json)?;
    Ok(module)
}

pub(crate) fn find_package_json(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = args
        .first()
        .copied()
        .map(|root| url_or_string_text(context, root))
        .transpose()?
        .flatten();
    let Some(specifier) = specifier else {
        return Err(invalid_url(context)?);
    };
    let base = args
        .get(1)
        .copied()
        .map(|root| url_or_string_text(context, root))
        .transpose()?
        .flatten();

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

fn url_or_string_text(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Option<String>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(Some(text));
    }
    if !context.is_object_rooted(value)? {
        return Ok(None);
    }

    let global = context.global_root()?;
    let constructor = get(context, global, "URL")?;
    let prototype = get(context, constructor, "prototype")?;
    let to_string = get(context, prototype, "toString")?;
    match context.call_rooted(to_string, value, &[]) {
        Ok(url) => context.string_text(url),
        Err(error) => {
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
            Ok(None)
        }
    }
}

fn file_url_path(value: &str) -> Option<PathBuf> {
    let url = url::Url::parse(value).ok()?;
    (url.scheme() == "file").then(|| url.to_file_path().ok()).flatten()
}

fn has_url_scheme(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')))
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

fn invalid_url(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootedError, RootedError> {
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
        &format!("Cannot find module '{}' imported from {from}", path.display()),
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
    let filename = args
        .first()
        .copied()
        .map(|root| context.string_text(root))
        .transpose()?
        .flatten()
        .and_then(|filename| {
            if filename.starts_with("file:") {
                url::Url::parse(&filename)
                    .ok()
                    .filter(|url| url.scheme() == "file")
                    .and_then(|url| url.to_file_path().ok())
            } else {
                let path = PathBuf::from(filename);
                path.is_absolute().then_some(path)
            }
        });
    let Some(filename) = filename else {
        let error = context.type_error_rooted(
            "The argument 'filename' must be a file URL or an absolute path string",
        )?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    };
    crate::host::shared_vm::commonjs::create_require(context, &filename)
}

pub(crate) fn is_builtin(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let vfs_enabled = vfs_enabled(context);
    let result = args
        .first()
        .and_then(|argument| context.string_text(*argument).ok().flatten())
        .is_some_and(|specifier| {
            if specifier == "node:vfs" {
                return vfs_enabled;
            }
            if let Some(name) = specifier.strip_prefix("node:") {
                BUILTIN_MODULES.contains(&name) || NODE_ONLY_BUILTINS.contains(&name)
            } else {
                BUILTIN_MODULES.contains(&specifier.as_str())
            }
        });
    Ok(context.boolean(result))
}

fn vfs_enabled(context: &mut NativeContext<'_, NodeHost>) -> bool {
    context
        .host_mut()
        .shared_state()
        .borrow()
        .exec_argv
        .iter()
        .any(|argument| argument == "--experimental-vfs")
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
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
