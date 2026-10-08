use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::PathBuf;

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
    let names = BUILTIN_MODULES
        .iter()
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    let names = context.array_rooted(&names)?;
    set(context, module, "builtinModules", names)?;
    let is_builtin = context.host_function(crate::host::shared_vm::operation("moduleIsBuiltin"))?;
    set(context, module, "isBuiltin", is_builtin)?;
    let create_require =
        context.host_function(crate::host::shared_vm::operation("moduleCreateRequire"))?;
    set(context, module, "createRequire", create_require)?;
    Ok(module)
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
    let result = args
        .first()
        .and_then(|argument| context.string_text(*argument).ok().flatten())
        .is_some_and(|specifier| {
            if let Some(name) = specifier.strip_prefix("node:") {
                BUILTIN_MODULES.contains(&name) || NODE_ONLY_BUILTINS.contains(&name)
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
