//! `node:express` module loader. Returns a real Express-compatible
//! `createApplication` factory (embedded JS, parse-safe). Cached on the
//! host.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::{execute::VmError, value::Value};

use crate::host::HostState;

const SOURCE: &str = include_str!("express.js");

pub fn build(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(cached) = state.borrow().express_module.clone() {
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
    state.borrow_mut().express_module = Some(module.clone());
    Ok(module)
}
