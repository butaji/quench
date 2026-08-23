//! `node:trace_events` embedded module loader.
use crate::host::HostState;
use quench_runtime::{execute::VmError, value::Value};
use std::cell::RefCell;
use std::rc::Rc;
const SOURCE: &str = include_str!("trace_events.js");
pub fn build(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(cached) = state.borrow().trace_events_module.clone() {
        return Ok(cached);
    }
    let program = quench_runtime::reduce::reduce_global_script_source(SOURCE)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
    })?;
    let deps = quench_runtime::host_api::object(vec![]);
    let module = quench_runtime::vm::call_value(&factory, &Value::Undefined, &[deps])?;
    state.borrow_mut().trace_events_module = Some(module.clone());
    Ok(module)
}
