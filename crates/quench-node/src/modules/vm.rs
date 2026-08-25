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
    let global = quench_runtime::vm::current_global_object();
    let filename = args.get(2).and_then(|options| {
        execute::get_property_result(options, "filename")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    });
    let has_filename = filename.is_some();
    if let Some(filename) = filename {
        let updated = execute::set_property(
            global.clone(),
            "\0quench_vm_filename",
            Value::String(filename),
        );
        execute::replace_value(&global, &updated);
    }
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let result = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    });
    if has_filename {
        let updated = execute::delete_property(global.clone(), "\0quench_vm_filename").0;
        execute::replace_value(&global, &updated);
    }
    result
}

pub fn construct_run_in_new_context(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    run_in_new_context(state, None, args)
}

pub fn create_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let context = args
        .first()
        .cloned()
        .unwrap_or_else(|| Value::object(vec![]));
    if !matches!(context, Value::Object(_) | Value::Array(_)) {
        return Err(execute::type_error("context must be an object"));
    }
    let updated = execute::set_property(context.clone(), "\0vmContext", Value::Boolean(true));
    execute::replace_value(&context, &updated);
    Ok(context)
}

pub fn is_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    Ok(Value::Boolean(matches!(
        execute::get_property(value, "\0vmContext"),
        Value::Boolean(true)
    )))
}

pub fn run_in_context(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    run_in_new_context(state, receiver, args)
}

pub fn build() -> Value {
    let run = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT);
    let create_context = crate::host::capability(crate::registry::SPEC_VM_CREATE_CONTEXT);
    let is_context = crate::host::capability(crate::registry::SPEC_VM_IS_CONTEXT);
    let run_in_context = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_CONTEXT);
    let source_text_module = crate::host::capability(crate::registry::SPEC_VM_SOURCE_TEXT_MODULE);
    crate::host::namespace_object(vec![
        ("runInNewContext", run.clone()),
        ("runInContext", run_in_context),
        ("createContext", create_context),
        ("isContext", is_context),
        ("SourceTextModule", source_text_module),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
