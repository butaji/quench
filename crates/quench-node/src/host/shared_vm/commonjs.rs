//! CommonJS host policy. Guest code, objects and calls belong to the shared VM.

use crate::host::{EntryGoal, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};

type Context<'a> = NativeContext<'a, NodeHost>;

// Node's public CommonJS wrapper; this is guest compilation input, not an API shim.
const WRAPPER_PREFIX: &str = "(function (exports, require, module, __filename, __dirname) { ";
const WRAPPER_SUFFIX: &str = "\n});";

/// Node's explicit extension/package parse-goal policy, before guest execution.
pub(crate) fn source_kind(path: &Path) -> Result<quench_runtime::SourceKind, String> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("mjs") => return Ok(quench_runtime::SourceKind::Module),
        Some("cjs") => return Ok(quench_runtime::SourceKind::Script),
        _ => {}
    }
    let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    // Node's upstream test checkout is a separate repository rooted at
    // `tests/node`. The compatibility workspace may itself be a module
    // package, but that package scope must not leak into the Node checkout.
    let node_tests_root = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join("tests/node")
        .canonicalize()
        .ok();
    if node_tests_root
        .as_ref()
        .is_some_and(|root| path.starts_with(root))
    {
        let mut directory = path.parent();
        while let Some(current) = directory {
            let package_json = current.join("package.json");
            if package_json.is_file() {
                let package: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&package_json).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let module = package.get("type").and_then(serde_json::Value::as_str)
                    == Some("module");
                return Ok(if module {
                    quench_runtime::SourceKind::Module
                } else {
                    quench_runtime::SourceKind::Script
                });
            }
            if node_tests_root.as_deref() == Some(current) {
                break;
            }
            directory = current.parent();
        }
        return Ok(quench_runtime::SourceKind::Script);
    }
    let resolution = oxc_resolver::Resolver::new(Default::default())
        .resolve(
            path.parent().unwrap_or(Path::new(".")),
            &path.to_string_lossy(),
        )
        .map_err(|error| error.to_string())?;
    let module = resolution
        .package_json()
        .and_then(|package| package.r#type.as_ref())
        .and_then(serde_json::Value::as_str)
        == Some("module");
    Ok(
        if path.extension().is_some_and(|extension| extension == "js") && module {
            quench_runtime::SourceKind::Module
        } else {
            quench_runtime::SourceKind::Script
        },
    )
}

pub(super) fn initialize(context: &mut Context<'_>) -> Result<(), RootedError> {
    let roots = context.host_mut().shared_state();
    if let Some(root) = roots.borrow_mut().assert_module.take() {
        context.release_root(root);
    }
    if let Some(root) = roots.borrow_mut().path_module.take() {
        context.release_root(root);
    }
    let previous = std::mem::take(&mut roots.borrow_mut().module_cache);
    for root in previous.into_values() {
        context.release_root(root);
    }
    let buffer_module = cached_builtin(context, BuiltinModule::Buffer)?;
    crate::modules::buffer_shared_vm::install_global(context, buffer_module)?;
    crate::modules::url_shared_vm::install_global(context)?;
    install_shared_web_globals(context, buffer_module)?;
    let console = cached_builtin(context, BuiltinModule::Console)?;
    let global = context.global_root()?;
    set(context, global, "console", console)?;
    if context.is_module()? {
        return Ok(());
    }
    if let Some(entry) = context.host_mut().commonjs_entry.clone() {
        let filename = std::fs::canonicalize(entry.path)
            .map_err(|error| RootedError::host(error.to_string()))?;
        load(context, &filename, None, entry.goal)?;
    } else {
        let filename = std::env::current_dir()
            .map_err(|error| RootedError::host(error.to_string()))?
            .join("[eval]");
        let module = module_record(context, &filename, None, false)?;
        let global = context.global_root()?;
        for name in ["exports", "require"] {
            let value = get(context, module, name)?;
            set(context, global, name, value)?;
        }
        set(context, global, "module", module)?;
        let filename = context.string_rooted(&filename.to_string_lossy());
        set(context, global, "__filename", filename)?;
        let cwd = std::env::current_dir().map_err(|error| RootedError::host(error.to_string()))?;
        let dirname = context.string_rooted(&cwd.to_string_lossy());
        set(context, global, "__dirname", dirname)?;
    }
    Ok(())
}

fn install_shared_web_globals(
    context: &mut Context<'_>,
    buffer_module: RootId,
) -> Result<(), RootedError> {
    let web_streams = crate::polyfills::bootstrap::web_streams::JS;
    let root =
        context.evaluate_script_rooted(web_streams, "node:bootstrap/shared-vm/web-streams.js")?;
    context.release_root(root);

    let web_apis = crate::polyfills::bootstrap::globals_extra::web_api_source();
    let root = context.evaluate_script_rooted(web_apis, "node:bootstrap/shared-vm/web-apis.js")?;
    context.release_root(root);
    crate::modules::buffer_shared_vm::install_blob_export(context, buffer_module)
}

pub(super) fn require(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Require)?;
    match BuiltinModule::from_specifier(&specifier) {
        Some(BuiltinModule::Process) => {
            return match context.host_mut().shared_state().borrow().process_module {
                Some(root) => Ok(root),
                _ => Err(RootedError::host(
                    "shared process module is not initialized",
                )),
            };
        }
        Some(BuiltinModule::InternalDgram) => {
            let dgram = cached_builtin(context, BuiltinModule::Dgram)?;
            context.release_root(dgram);
            return context.evaluate_script_rooted(
                r#"(() => {
  const dgram = globalThis["\0quench:dgram_module"];
  const stateSymbol = Object.getOwnPropertySymbols(dgram.createSocket("udp4"))
    .find((symbol) => symbol.description === "quench.dgram.state");
  const internals = globalThis[Symbol.for("quench.dgram.internals")];
  return {
    kStateSymbol: stateSymbol,
    _createSocketHandle(address, port, type, flags, fd) {
      if (fd !== undefined) {
        if (!globalThis.__quenchDgramUdpFds.has(fd)) return -9;
        const adopted = new internals.UDP(); adopted.fd = fd; return adopted;
      }
      const handle = new internals.UDP();
      if (address === null) return handle;
      return handle.bind(address, port, flags) < 0 ? -1 : handle;
    },
  };
})()"#,
                "internal/dgram.js",
            );
        }
        Some(BuiltinModule::InternalTestBinding) => {
            let dgram = cached_builtin(context, BuiltinModule::Dgram)?;
            context.release_root(dgram);
            return context.evaluate_script_rooted(
                r#"(() => {
  const internals = globalThis[Symbol.for("quench.dgram.internals")];
  return { internalBinding(name) {
    if (name === "udp_wrap") return { UDP: internals.UDP };
    if (name === "tcp_wrap") return { TCP: internals.TCP, constants: { SOCKET: 0 } };
    if (name === "uv") return {
      UV_UDP_REUSEADDR: 4, UV_UNKNOWN: -4094, UV_EBADF: -9,
      UV_EINVAL: -22, UV_ENOTSOCK: -88,
    };
    return {};
  } };
})()"#,
                "internal/test/binding.js",
            );
        }
        Some(BuiltinModule::InternalBlockList) => {
            return context.evaluate_script_rooted(
                "({ kHandle: Symbol.for('quench.internal.blocklist.handle') })",
                "internal/blocklist.js",
            );
        }
        Some(BuiltinModule::InternalSocketAddress) => {
            let module = context.evaluate_script_rooted(
                "({ kHandle: Symbol.for('quench.internal.socketaddress.handle') })",
                "internal/socketaddress.js",
            )?;
            let net = cached_builtin(context, BuiltinModule::Net)?;
            let constructor = get(context, net, "SocketAddress")?;
            context.release_root(net);
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
            return crate::modules::async_hooks_shared_vm::module(context);
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
            | BuiltinModule::Net
            | BuiltinModule::Http
            | BuiltinModule::Dgram
            | BuiltinModule::Os
            | BuiltinModule::Buffer
            | BuiltinModule::Stream
            | BuiltinModule::StreamPromises
            | BuiltinModule::Timers
            | BuiltinModule::TimersPromises
            | BuiltinModule::NodeTest
            | BuiltinModule::StringDecoder
            | BuiltinModule::WorkerThreads
            | BuiltinModule::Util
            | BuiltinModule::ChildProcess
            | BuiltinModule::Https
            | BuiltinModule::Http2
            | BuiltinModule::Vm
            | BuiltinModule::Inspector
            | BuiltinModule::Repl
            | BuiltinModule::Sea
            | BuiltinModule::Cluster
            | BuiltinModule::Wasi
            | BuiltinModule::TraceEvents),
        ) => {
            return cached_builtin(context, module);
        }
        None => {}
    }
    let parent = context.host_function_data()?;
    let filename = resolve_filename(context, &specifier, parent)?;
    load(context, &filename, Some(parent), EntryGoal::Node)
}

pub(super) fn resolve(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let specifier = specifier(context, args, Request::Resolve)?;
    if BuiltinModule::from_specifier(&specifier).is_some() {
        return Ok(context.string_rooted(&specifier));
    }
    let parent = context.host_function_data()?;
    let filename = resolve_filename(context, &specifier, parent)?;
    Ok(context.string_rooted(&filename.to_string_lossy()))
}

#[derive(Clone, Copy)]
enum BuiltinModule {
    Process,
    Assert,
    AssertStrict,
    Path,
    PathPosix,
    PathWin32,
    Fs,
    FsPromises,
    Net,
    Http,
    Os,
    Buffer,
    Stream,
    StreamPromises,
    WebStreams,
    StringDecoder,
    WorkerThreads,
    Util,
    Timers,
    TimersPromises,
    NodeTest,
    ChildProcess,
    Url,
    Querystring,
    Readline,
    StreamConsumers,
    Events,
    Console,
    Tty,
    Crypto,
    Tls,
    V8,
    Module,
    AsyncHooks,
    DiagnosticsChannel,
    Dns,
    Dgram,
    InternalDgram,
    InternalTestBinding,
    InternalBlockList,
    InternalSocketAddress,
    Https,
    Http2,
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
            Self::Fs => Some("fs"),
            Self::FsPromises => Some("fs/promises"),
            Self::Net => Some("net"),
            Self::Http => Some("http"),
            Self::Os => Some("os"),
            Self::Buffer => Some("buffer"),
            Self::Stream => Some("stream"),
            Self::StreamPromises => Some("stream/promises"),
            Self::WebStreams => Some("stream/web"),
            Self::StringDecoder => Some("string_decoder"),
            Self::WorkerThreads => Some("worker_threads"),
            Self::Util => Some("util"),
            Self::Timers => Some("timers"),
            Self::TimersPromises => Some("timers/promises"),
            Self::NodeTest => Some("node:test"),
            Self::ChildProcess => Some("child_process"),
            Self::Url => Some("url"),
            Self::Querystring => Some("querystring"),
            Self::Readline => Some("readline"),
            Self::StreamConsumers => Some("stream/consumers"),
            Self::Events => Some("events"),
            Self::Console => Some("console"),
            Self::Tty => Some("tty"),
            Self::Crypto => Some("crypto"),
            Self::Tls => Some("tls"),
            Self::V8 => Some("v8"),
            Self::Module => Some("module"),
            Self::Dns => Some("dns"),
            Self::Dgram => Some("dgram"),
            Self::Https => Some("https"),
            Self::Http2 => Some("http2"),
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
            Self::InternalDgram
            | Self::InternalTestBinding
            | Self::InternalBlockList
            | Self::InternalSocketAddress => None,
        }
    }
}

const BUILTIN_SPECIFIERS: &[(&str, BuiltinModule)] = &[
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
    ("stream/promises", BuiltinModule::StreamPromises),
    ("node:stream/promises", BuiltinModule::StreamPromises),
    ("stream/web", BuiltinModule::WebStreams),
    ("node:stream/web", BuiltinModule::WebStreams),
    ("stream/consumers", BuiltinModule::StreamConsumers),
    ("node:stream/consumers", BuiltinModule::StreamConsumers),
    ("timers", BuiltinModule::Timers),
    ("node:timers", BuiltinModule::Timers),
    ("timers/promises", BuiltinModule::TimersPromises),
    ("node:timers/promises", BuiltinModule::TimersPromises),
    ("node:test", BuiltinModule::NodeTest),
    ("string_decoder", BuiltinModule::StringDecoder),
    ("node:string_decoder", BuiltinModule::StringDecoder),
    ("worker_threads", BuiltinModule::WorkerThreads),
    ("node:worker_threads", BuiltinModule::WorkerThreads),
    ("util", BuiltinModule::Util),
    ("node:util", BuiltinModule::Util),
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
    ("tls", BuiltinModule::Tls),
    ("node:tls", BuiltinModule::Tls),
    ("v8", BuiltinModule::V8),
    ("node:v8", BuiltinModule::V8),
    ("module", BuiltinModule::Module),
    ("node:module", BuiltinModule::Module),
    ("async_hooks", BuiltinModule::AsyncHooks),
    ("node:async_hooks", BuiltinModule::AsyncHooks),
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
    ("internal/test/binding", BuiltinModule::InternalTestBinding),
    ("internal/blocklist", BuiltinModule::InternalBlockList),
    ("internal/socketaddress", BuiltinModule::InternalSocketAddress),
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
    let canonical = builtin
        .cache_key()
        .ok_or_else(|| RootedError::host("builtin module has no canonical cache key"))?;
    let key = format!("\0builtin:{canonical}");
    let roots = context.host_mut().shared_state();
    if let Some(module) = roots.borrow().module_cache.get(&key).copied() {
        return Ok(module);
    }

    let module = build_builtin(context, builtin)?;
    let retained = context.retain(module)?;
    roots.borrow_mut().module_cache.insert(key, retained);
    Ok(module)
}

pub(crate) fn stream_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    cached_builtin(context, BuiltinModule::Stream)
}

fn build_builtin(context: &mut Context<'_>, builtin: BuiltinModule) -> Result<RootId, RootedError> {
    match builtin {
        BuiltinModule::Fs => crate::modules::fs_shared_vm::module(context),
        BuiltinModule::FsPromises => {
            let fs = cached_builtin(context, BuiltinModule::Fs)?;
            get(context, fs, "promises")
        }
        BuiltinModule::Net => crate::modules::net_shared_vm::module(context),
        BuiltinModule::Http => crate::modules::http_shared_vm::module(context),
        BuiltinModule::Os => crate::modules::os_shared_vm::module(context),
        BuiltinModule::Buffer => crate::modules::buffer_shared_vm::module(context),
        BuiltinModule::Stream => {
            let decoder = cached_builtin(context, BuiltinModule::StringDecoder)?;
            crate::modules::stream_shared_vm::module(context, decoder)
        }
        BuiltinModule::StreamPromises => {
            let stream = cached_builtin(context, BuiltinModule::Stream)?;
            get(context, stream, "promises")
        }
        BuiltinModule::WebStreams => {
            let global = context.global_root()?;
            get(context, global, "__quenchWebStreams")
        }
        BuiltinModule::StringDecoder => crate::modules::string_decoder_shared_vm::module(context),
        BuiltinModule::Timers => {
            let promises = cached_builtin(context, BuiltinModule::TimersPromises)?;
            crate::modules::timers_shared_vm::timers_module(context, promises)
        }
        BuiltinModule::TimersPromises => crate::modules::timers_shared_vm::promises_module(context),
        BuiltinModule::NodeTest => crate::modules::test_shared_vm::module(context),
        BuiltinModule::WorkerThreads => {
            context.evaluate_script_rooted(
                "({ isMainThread: true, Worker: class Worker { constructor() { throw Object.assign(new Error('Worker threads are unavailable in this runtime'), { code: 'ERR_WORKER_UNSUPPORTED_OPERATION' }); } } })",
                "node:worker_threads.js",
            )
        }
        BuiltinModule::Url => crate::modules::url_shared_vm::module(context),
        BuiltinModule::Querystring => crate::modules::querystring_shared_vm::module(context),
        BuiltinModule::Readline => crate::modules::readline_shared_vm::module(context),
        BuiltinModule::StreamConsumers => {
            crate::modules::web_stream_consumers_shared_vm::module(context)
        }
        BuiltinModule::Events => crate::modules::events_shared_vm::module(context),
        BuiltinModule::Console => crate::modules::console_shared_vm::module(context),
        BuiltinModule::Tty => crate::modules::tty_shared_vm::module(context),
        BuiltinModule::Dns => crate::modules::dns_shared_vm::module(context),
        BuiltinModule::Dgram => {
            let dgram_tail = crate::polyfills::bootstrap::dgram_tail::JS
                .split("globalThis.require = (specifier) =>")
                .next()
                .unwrap_or(crate::polyfills::bootstrap::dgram_tail::JS);
            let source = format!(
                "(() => {{\n{}\n{}\n{}\n{}\n}})();",
                crate::polyfills::bootstrap::dgram_head::JS,
                crate::polyfills::bootstrap::dgram::JS,
                crate::polyfills::bootstrap::membership::JS,
                dgram_tail,
            );
            let root = context.evaluate_script_rooted(&source, "node:dgram/bootstrap.js")?;
            context.release_root(root);
            let global = context.global_root()?;
            get(context, global, "\0quench:dgram_module")
        }
        BuiltinModule::PerfHooks => crate::modules::perf_hooks::module(context),
        BuiltinModule::Zlib => {
            let stream = cached_builtin(context, BuiltinModule::Stream)?;
            crate::modules::zlib_shared_vm::module(context, stream)
        }
        BuiltinModule::Crypto => crate::modules::crypto_shared_vm::module(context),
        BuiltinModule::Tls => crate::modules::tls_shared_vm::module(context),
        BuiltinModule::V8 => crate::modules::v8_shared_vm::module(context),
        BuiltinModule::Module => crate::modules::module_shared_vm::module(context),
        BuiltinModule::AsyncHooks | BuiltinModule::DiagnosticsChannel => Err(RootedError::host(
            "stateful builtin passed to generic shared module builder",
        )),
        BuiltinModule::Util => crate::modules::util_shared_vm::module(context),
        BuiltinModule::ChildProcess => crate::modules::child_process_shared_vm::module(context),
        // Fastify imports both alternatives at module initialization. Its
        // selected HTTP/1 path does not access the HTTPS transport.
        BuiltinModule::Https => context.evaluate_script_rooted(
            "({ request: function request() { throw new Error('HTTPS transport is unavailable'); }, get: function get() { throw new Error('HTTPS transport is unavailable'); } })",
            "node:https.js",
        ),
        BuiltinModule::Http2
        | BuiltinModule::Vm
        | BuiltinModule::Inspector
        | BuiltinModule::Repl
        | BuiltinModule::Cluster
        | BuiltinModule::Wasi
        | BuiltinModule::TraceEvents => context.object_rooted(),
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
        | BuiltinModule::InternalSocketAddress => Err(RootedError::host(
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
    let filename = context
        .string_text(filename)?
        .ok_or_else(|| RootedError::host("invalid parent module filename"))?;
    let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        extensions: vec![".js".into(), ".json".into(), ".node".into()],
        main_files: vec!["index".into()],
        condition_names: vec!["node".into(), "require".into(), "default".into()],
        ..Default::default()
    });
    match resolver.resolve(
        Path::new(&filename).parent().unwrap_or(Path::new(".")),
        specifier,
    ) {
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
    let loaded = context.boolean(false);
    set(context, module, "loaded", loaded)?;
    let children = context.array_rooted(&[])?;
    set(context, module, "children", children)?;
    let paths = directory
        .ancestors()
        .filter(|path| path.file_name().is_none_or(|name| name != "node_modules"))
        .map(|path| context.string_rooted(&path.join("node_modules").to_string_lossy()))
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
    let resolve = context.host_function_with_data(super::operation("resolve"), module)?;
    set(context, require, "resolve", resolve)?;
    let main = get(context, process, "mainModule")?;
    set(context, require, "main", main)?;
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

fn load(
    context: &mut Context<'_>,
    filename: &Path,
    parent: Option<RootId>,
    goal: EntryGoal,
) -> Result<RootId, RootedError> {
    let key = filename.to_string_lossy().into_owned();
    let roots = context.host_mut().shared_state();
    let cached = roots.borrow().module_cache.get(&key).copied();
    if let Some(module) = cached {
        if let Some(parent) = parent {
            transition_child(context, parent, module, ChildTransition::Attach)?;
        }
        return get(context, module, "exports");
    }
    let is_main = parent.is_none() && context.host_mut().commonjs_entry.is_some();
    let module = module_record(context, filename, parent, is_main)?;
    if let Some(parent) = parent {
        transition_child(context, parent, module, ChildTransition::Attach)?;
    }
    let retained = context.retain(module)?;
    roots
        .borrow_mut()
        .module_cache
        .insert(key.clone(), retained);
    let result = (|| {
        let bytes =
            std::fs::read(filename).map_err(|error| RootedError::host(error.to_string()))?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match filename
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("json") => {
                let exports = context.parse_json_rooted(text)?;
                set(context, module, "exports", exports)?;
            }
            Some("mjs" | "node") => {
                return Err(RootedError::host(
                    "this module format is not implemented on the shared VM",
                ));
            }
            _ => {
                if goal == EntryGoal::Node
                    && source_kind(filename).map_err(RootedError::host)?
                        == quench_runtime::SourceKind::Module
                {
                    return Err(RootedError::host(
                        "requiring an ES module is not implemented on the shared VM",
                    ));
                }
                let text = if text.starts_with("#!") {
                    text.find('\n').map_or("", |end| &text[end..])
                } else {
                    text
                };
                let wrapper = context.evaluate_specialized_script_rooted(
                    &format!("{WRAPPER_PREFIX}{text}{WRAPPER_SUFFIX}"),
                    &key,
                )?;
                let exports = get(context, module, "exports")?;
                let require = get(context, module, "require")?;
                let name = context.string_rooted(&key);
                let directory = context.string_rooted(
                    &filename
                        .parent()
                        .unwrap_or(Path::new("."))
                        .to_string_lossy(),
                );
                context.call_rooted(
                    wrapper,
                    exports,
                    &[exports, require, module, name, directory],
                )?;
            }
        }
        let loaded = context.boolean(true);
        set(context, module, "loaded", loaded)?;
        get(context, module, "exports")
    })();
    if result.is_err() {
        roots.borrow_mut().module_cache.remove(&key);
        context.release_root(retained);
        if let Some(parent) = parent {
            transition_child(context, parent, module, ChildTransition::Detach)?;
        }
    }
    result
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
