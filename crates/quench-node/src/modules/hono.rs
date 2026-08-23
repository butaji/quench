//! Embedded Hono-compatible application factory.
use std::cell::RefCell;
use std::rc::Rc;

use crate::host::HostState;
use quench_runtime::{execute::VmError, value::Value};

const SOURCE: &str = include_str!("hono.js");

pub fn build(_state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(SOURCE)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
    })?;
    quench_runtime::vm::call_value(&factory, &Value::Undefined, &[])
}
