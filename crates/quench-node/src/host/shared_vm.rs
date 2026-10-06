//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use rqj::{HostFunction, HostFunctionId, NativeContext, RootedError, SystemHost};

mod commonjs;
pub(crate) use commonjs::source_kind;

pub(crate) fn bindings() -> &'static [HostFunction<NodeHost>] {
    rqj::host_functions![
        method "uptime" (0) => crate::modules::process::shared_vm::uptime,
        method "nextTick" (1) => crate::modules::process::shared_vm::next_tick,
        method "on" (2) => crate::modules::process::shared_vm::on,
        method "require" (1) => commonjs::require,
        method "resolve" (2) => commonjs::resolve,
        method "assert" (1) => crate::modules::assert::shared_vm::ok,
        method "ok" (1) => crate::modules::assert::shared_vm::ok,
        method "strict" (1) => crate::modules::assert::shared_vm::ok,
        method "strictEqual" (2) => crate::modules::assert::shared_vm::strict_equal,
        method "throws" (3) => crate::modules::assert::shared_vm::throws,
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
        commonjs::initialize(context)
    }
}
