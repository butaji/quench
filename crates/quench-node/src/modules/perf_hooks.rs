use std::cell::RefCell;
use std::rc::Rc;
use quench_runtime::{execute::VmError, value::Value};
use crate::host::HostState;
const SOURCE: &str = include_str!("perf_hooks.js");
pub fn build(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(v) = state.borrow().perf_hooks_module.clone() { return Ok(v); }
    let program = quench_runtime::reduce::reduce_global_script_source(SOURCE).map_err(|e| VmError::EvalError(e.join("; ")))?;
    let context = quench_runtime::vm::current_context(); let mut registers = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context))?;
    let module = quench_runtime::vm::call_value(&factory, &Value::Undefined, &[quench_runtime::host_api::object(vec![])])?;
    state.borrow_mut().perf_hooks_module = Some(module.clone()); Ok(module)
}
