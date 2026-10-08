//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use quench_runtime::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};

#[path = "shared_vm/commonjs.rs"]
mod commonjs;
pub(crate) use commonjs::source_kind;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    quench_runtime::host_functions![
        method "uptime" (0) => crate::modules::process::shared_vm::uptime,
        method "processCwd" (0) => crate::modules::process::shared_vm::cwd,
        method "processUmask" (0) => crate::modules::process::shared_vm::umask,
        method "nextTick" (1) => crate::modules::process::shared_vm::next_tick,
        method "on" (2) => crate::modules::process::shared_vm::on,
        method "require" (1) => commonjs::require,
        method "resolve" (2) => commonjs::resolve,
        method "assert" (1) => crate::modules::assert::shared_vm::ok,
        method "ok" (1) => crate::modules::assert::shared_vm::ok,
        method "strict" (1) => crate::modules::assert::shared_vm::ok,
        method "strictEqual" (2) => crate::modules::assert::shared_vm::strict_equal,
        method "notStrictEqual" (2) => crate::modules::assert::shared_vm::not_strict_equal,
        method "deepStrictEqual" (2) => crate::modules::assert::shared_vm::deep_strict_equal,
        method "fail" (1) => crate::modules::assert::shared_vm::fail,
        method "throws" (3) => crate::modules::assert::shared_vm::throws,
        method "pathJoin" (0) => crate::modules::path::shared_vm::join_operation,
        method "pathResolve" (0) => crate::modules::path::shared_vm::resolve_operation,
        method "pathRelative" (2) => crate::modules::path::shared_vm::relative_operation,
        method "pathBasename" (2) => crate::modules::path::shared_vm::basename_operation,
        method "pathDirname" (1) => crate::modules::path::shared_vm::dirname_operation,
        method "pathExtname" (1) => crate::modules::path::shared_vm::extname_operation,
        method "pathNormalize" (1) => crate::modules::path::shared_vm::normalize_operation,
        method "urlParse" (2) => crate::modules::url::shared_vm::parse,
        method "querystringParse" (1) => crate::modules::querystring::shared_vm::parse,
        method "osType" (0) => crate::modules::os::shared_vm::type_operation,
        method "osTotalmem" (0) => crate::modules::os::shared_vm::totalmem_operation,
        method "netGetAutoSelectFamilyAttemptTimeout" (0) => crate::modules::net::shared_vm::get_timeout,
        method "netSetAutoSelectFamilyAttemptTimeout" (1) => crate::modules::net::shared_vm::set_timeout,
        method "httpCreateServer" (1) => crate::modules::http::shared_vm::create_server,
        method "httpServerListen" (1) => crate::modules::http::shared_vm::server_listen,
        method "httpServerAddress" (0) => crate::modules::http::shared_vm::server_address,
        method "httpServerClose" (0) => crate::modules::http::shared_vm::server_close,
        method "httpAgentCreate" (1) => crate::modules::http::shared_vm::agent_create,
        method "httpAgentDestroy" (1) => crate::modules::http::shared_vm::agent_destroy,
        method "httpGet" (5) => crate::modules::http::shared_vm::get,
        method "httpResponseSetHeader" (3) => crate::modules::http::shared_vm::response_set_header,
        method "httpResponseRemoveHeader" (2) => crate::modules::http::shared_vm::response_remove_header,
        method "httpResponseEnd" (3) => crate::modules::http::shared_vm::response_end,
        method "cryptoHashDigest" (3) => crate::modules::crypto::shared_vm::hash_digest,
        method "fsStat" (2) => crate::modules::fs::shared_vm::stat,
        method "fsReadFileSync" (2) => crate::modules::fs::shared_vm::read_file_sync,
        method "bufferEncode" (2) => crate::modules::buffer::shared_vm::encode,
        method "bufferDecode" (2) => crate::modules::buffer::shared_vm::decode,
        method "bufferCanonicalEncoding" (1) => crate::modules::buffer::shared_vm::canonical_encoding,
        method "stringDecoderChunk" (4) => crate::modules::string_decoder::shared_vm::decode_chunk,
        global "fetch" (2) => crate::modules::fetch_shared_vm::fetch,
        method "fetchExecutor" (2) => crate::modules::fetch_shared_vm::executor,
        method "fetchComplete" (1) => crate::modules::fetch_shared_vm::complete,
        method "fetchResponseText" (0) => crate::modules::fetch_shared_vm::response_text,
        method "fetchResponseJson" (0) => crate::modules::fetch_shared_vm::response_json,
        method "fetchHeaderGet" (1) => crate::modules::fetch_shared_vm::header_get,
        method "fetchHeaderHas" (1) => crate::modules::fetch_shared_vm::header_has,
        global "queueMicrotask" (1) => crate::modules::timers::shared_vm::queue_microtask,
        global "setImmediate" (1) => crate::modules::timers::shared_vm::set_immediate,
        global "clearImmediate" (1) => crate::modules::timers::shared_vm::clear_immediate,
        global "setTimeout" (1) => crate::modules::timers::shared_vm::set_timeout,
        global "clearTimeout" (1) => crate::modules::timers::shared_vm::clear_timer,
        global "setInterval" (1) => crate::modules::timers::shared_vm::set_interval,
        global "clearInterval" (1) => crate::modules::timers::shared_vm::clear_timer,
        global "structuredClone" (1) => crate::modules::clone_shared_vm::structured_clone,
    ]
}

pub(crate) fn operation(name: &str) -> HostFunctionId {
    let index = bindings()
        .iter()
        .position(|binding| binding.name == name)
        .expect("registered Node operation");
    HostFunctionId(u32::try_from(index).expect("Node binding index fits u32"))
}

impl quench_runtime::Host for NodeHost {
    fn write_line(&mut self, text: &str) {
        quench_runtime::Host::write_line(&mut SystemHost, text)
    }

    fn clock_millis(&mut self) -> f64 {
        quench_runtime::Host::clock_millis(&mut SystemHost)
    }

    fn functions(&self) -> &[HostFunction<Self>] {
        bindings()
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        crate::modules::process::shared_vm::initialize(context)?;
        let error_stack = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::ERROR_STACK_TRACE,
            "node:bootstrap/shared-vm/error-stack-trace.js",
        )?;
        context.release_root(error_stack);
        let abort = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::ABORT,
            "node:bootstrap/shared-vm/abort.js",
        )?;
        context.release_root(abort);
        let event_target = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::EVENT_TARGET,
            "node:bootstrap/shared-vm/event-target.js",
        )?;
        context.release_root(event_target);
        let event_emitter = context.evaluate_script_rooted(
            crate::polyfills::bootstrap::event_emitter::JS,
            "node:bootstrap/event-emitter.js",
        )?;
        context.release_root(event_emitter);
        let web_streams_source = crate::polyfills::bootstrap::lookup("web-streams")
            .ok_or_else(|| RootedError::host("Node web-stream bootstrap is unavailable"))?;
        let web_streams =
            context.evaluate_script_rooted(web_streams_source, "node:bootstrap/web-streams.js")?;
        context.release_root(web_streams);
        let text_decoder = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::TEXT_DECODER,
            "node:bootstrap/shared-vm/text-decoder.js",
        )?;
        context.release_root(text_decoder);
        let blob = context.evaluate_script_rooted(
            crate::polyfills::shared_vm::BLOB,
            "node:bootstrap/shared-vm/blob.js",
        )?;
        context.release_root(blob);
        commonjs::initialize(context)
    }
}
