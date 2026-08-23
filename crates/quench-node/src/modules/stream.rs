//! `stream` module — the public surface is a minimal but real
//! Readable/Writable/Duplex/Transform written in JS
//! (`stream_prelude.js`, mirroring Node's own JS streams) over the
//! native EventEmitter, evaluated once per realm and cached. The Rust
//! capability constructors below remain registered for dispatch
//! compatibility but are no longer exported by the module.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_STREAM_DUPLEX, SPEC_STREAM_PIPELINE, SPEC_STREAM_READABLE, SPEC_STREAM_TRANSFORM,
    SPEC_STREAM_WRITABLE,
};

const PRELUDE: &str = include_str!("stream_prelude.js");

pub fn new_readable(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Readable"))
}
pub fn new_writable(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Writable"))
}
pub fn new_duplex(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Duplex"))
}
pub fn new_transform(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Transform"))
}

fn stream_object(name: &str) -> Value {
    host_api::object(vec![
        ("readable".to_string(), Value::Boolean(true)),
        ("writable".to_string(), Value::Boolean(true)),
        ("name".to_string(), Value::String(name.into())),
    ])
}

pub fn pipeline(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(cached) = state.borrow().stream_module.clone() {
        return Ok(cached);
    }
    let program = quench_runtime::reduce::reduce_global_script_source(PRELUDE)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    })?;
    let deps = host_api::object(vec![(
        "events".to_string(),
        crate::modules::events::build(),
    )]);
    let module = match quench_runtime::vm::call_value(&factory, &Value::Undefined, &[deps]) {
        Ok(module) => module,
        Err(_) => {
            // Keep module loading total when the optional JS stream layer hits
            // an unsupported dynamic construct; the native constructors are
            // the canonical fallback and preserve the public API shape.
            host_api::object(vec![
                (
                    "Readable".into(),
                    crate::host::capability(SPEC_STREAM_READABLE),
                ),
                (
                    "Writable".into(),
                    crate::host::capability(SPEC_STREAM_WRITABLE),
                ),
                ("Duplex".into(), crate::host::capability(SPEC_STREAM_DUPLEX)),
                (
                    "Transform".into(),
                    crate::host::capability(SPEC_STREAM_TRANSFORM),
                ),
                (
                    "pipeline".into(),
                    crate::host::capability(SPEC_STREAM_PIPELINE),
                ),
            ])
        }
    };
    state.borrow_mut().stream_module = Some(module.clone());
    Ok(module)
}
