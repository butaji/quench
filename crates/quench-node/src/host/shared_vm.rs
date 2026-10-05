//! The existing Node host adapted to the shared VM; Node policy stays in modules.

use super::NodeHost;
use rqj::{HostFunction, NativeContext, RootedError, SystemHost};

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
        crate::modules::process::shared_vm::bindings()
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        crate::modules::process::shared_vm::initialize(context)
    }
}
