//! Small compatibility namespaces whose state is supplied by the host layer.
use crate::host::HostState;
use quench_runtime::{execute::VmError, value::Value};
use std::cell::RefCell;
use std::rc::Rc;

pub fn v8(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    Ok(crate::modules::v8::build())
}
pub fn worker_threads(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    let exports = crate::modules::worker_threads::build(_state)?;
    // Node exposes the worker messaging constructors both from the module and
    // on the global object.  Bootstrap may have installed a lightweight
    // placeholder before this module is required; replace it with the
    // canonical implementation so global MessageChannel ports retain their
    // postMessage/close surface and identity.
    let global = quench_runtime::vm::current_global_object();
    for name in ["MessageChannel", "MessagePort"] {
        let value = quench_runtime::execute::get_property(&exports, name);
        if !matches!(value, Value::Undefined) {
            quench_runtime::execute::set_property_in_place(&global, name, value);
        }
    }
    Ok(exports)
}
pub fn async_hooks(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    Ok(crate::modules::async_hooks::build())
}

pub fn sea_is_sea(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(false))
}
