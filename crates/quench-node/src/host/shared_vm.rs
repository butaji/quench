//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use rqj::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};

mod commonjs;
pub(crate) use commonjs::source_kind;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    rqj::host_functions![
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
        method "urlParse" (2) => crate::modules::url::shared_vm::parse,
        method "querystringParse" (1) => crate::modules::querystring::shared_vm::parse,
        method "osType" (0) => crate::modules::os::shared_vm::type_operation,
        method "netGetAutoSelectFamilyAttemptTimeout" (0) => crate::modules::net::shared_vm::get_timeout,
        method "netSetAutoSelectFamilyAttemptTimeout" (1) => crate::modules::net::shared_vm::set_timeout,
        method "fsReadFileSync" (2) => crate::modules::fs::shared_vm::read_file_sync,
        method "bufferEncode" (2) => crate::modules::buffer::shared_vm::encode,
        method "bufferDecode" (2) => crate::modules::buffer::shared_vm::decode,
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

impl rqj::Host for NodeHost {
    fn write_line(&mut self, text: &str) {
        let output = self.state().borrow().output.clone();
        match output {
            Some(output) => output(&format!("{text}\n")),
            None => rqj::Host::write_line(&mut SystemHost, text),
        }
    }

    fn clock_millis(&mut self) -> f64 {
        rqj::Host::clock_millis(&mut SystemHost)
    }

    fn functions(&self) -> &[HostFunction<Self>] {
        bindings()
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        crate::modules::process::shared_vm::initialize(context)?;
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
        commonjs::initialize(context)
    }
}
