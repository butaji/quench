//! Rust-owned `node:wasi` surface. Native WASI syscalls remain an explicit
//! capability boundary; this module only owns the Node object contract.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_WASI_IMPORT_OBJECT, SPEC_WASI_INITIALIZE, SPEC_WASI_START,
    SPEC_WASI_CONSTRUCTOR,
};

const IMPORTS: &str = "\0quench:wasi:imports";

pub fn build() -> Value {
    crate::host::namespace_object(vec![(
        "WASI",
        crate::host::capability(SPEC_WASI_CONSTRUCTOR),
    )])
    .unwrap_or_else(|_| Value::Undefined)
}

pub fn new_wasi(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let args_value = option_or(&options, "args", host_api::array(Vec::new()))?;
    let env = option_or(&options, "env", host_api::object(Vec::new()))?;
    let preopens = option_or(&options, "preopens", host_api::object(Vec::new()))?;
    let return_on_exit = match execute::get_property_result(&options, "returnOnExit") {
        Ok(Value::Undefined) | Err(_) => Value::Boolean(true),
        Ok(value) => value,
    };
    let imports = host_api::object(Vec::new());
    Ok(host_api::object(vec![
        ("options".into(), options),
        ("args".into(), args_value),
        ("env".into(), env),
        ("preopens".into(), preopens),
        ("returnOnExit".into(), return_on_exit),
        (IMPORTS.into(), imports.clone()),
        ("wasiImport".into(), imports),
        ("start".into(), crate::host::capability(SPEC_WASI_START)),
        (
            "initialize".into(),
            crate::host::capability(SPEC_WASI_INITIALIZE),
        ),
        (
            "getImportObject".into(),
            crate::host::capability(SPEC_WASI_IMPORT_OBJECT),
        ),
    ]))
}

pub fn start(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    invoke_export(args.first(), "_start")
}

pub fn initialize(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    invoke_export(args.first(), "_initialize")
}

pub fn import_object(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let imports = receiver
        .and_then(|value| execute::get_property_result(value, IMPORTS).ok())
        .unwrap_or_else(|| host_api::object(Vec::new()));
    Ok(host_api::object(vec![(
        "wasi_snapshot_preview1".into(),
        imports,
    )]))
}

fn option_or(options: &Value, key: &str, fallback: Value) -> Result<Value, VmError> {
    match execute::get_property_result(options, key) {
        Ok(Value::Undefined) | Err(_) => Ok(fallback),
        Ok(value) => Ok(value),
    }
}

fn invoke_export(instance: Option<&Value>, name: &str) -> Result<Value, VmError> {
    let Some(instance) = instance else {
        return Err(invalid_export(name));
    };
    let exports = execute::get_property_result(instance, "exports")
        .map_err(|_| invalid_export(name))?;
    let function = execute::get_property_result(&exports, name)
        .map_err(|_| invalid_export(name))?;
    if !quench_runtime::is_callable(&function) {
        return Err(invalid_export(name));
    }
    execute::call(&function, &exports, &[])
}

fn invalid_export(name: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "message".into(),
            Value::String(format!("instance must export {name}")),
        ),
    ]))
}
