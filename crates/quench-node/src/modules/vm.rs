//! `vm` module — minimal `runInNewContext`/`runInContext` that evaluate
//! source text as a classic script through the runtime's reducer.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn run_in_new_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let program = quench_runtime::reduce::reduce_global_script_source(&source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = Vec::new();
    quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
}

pub fn build() -> Value {
    let run = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT);
    crate::host::namespace_object(vec![
        ("runInNewContext", run.clone()),
        ("runInContext", run),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
