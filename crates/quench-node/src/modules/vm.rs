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
    let mut context = quench_runtime::vm::current_context();
    if let Some(sandbox @ Value::Object(_)) = args.get(1) {
        for key in execute::own_enumerable_keys(sandbox) {
            let value = execute::get_property_result(sandbox, &key)?;
            context = Rc::new((*context).clone().with_host_value(key, value));
        }
    }
    let mut registers = Vec::new();
    quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    })
}

pub fn build() -> Value {
    let run = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT);
    crate::host::namespace_object(vec![
        ("runInNewContext", run.clone()),
        ("runInContext", run),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
