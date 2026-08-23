//! `util.inherits(ctor, superCtor)` — Node's constructor-inheritance
//! helper: defines a hidden `super_` on `ctor`, chains
//! `ctor.prototype` to `superCtor.prototype`, and redefines the
//! hidden `constructor` back-reference.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn inherits(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let ctor = args.first().unwrap_or(&Value::Undefined);
    let super_ctor = args.get(1).unwrap_or(&Value::Undefined);
    if matches!(ctor, Value::Undefined | Value::Null) {
        return Err(invalid_arg("ctor", "function", ctor));
    }
    if matches!(super_ctor, Value::Undefined | Value::Null) {
        return Err(invalid_arg("superCtor", "function", super_ctor));
    }
    let super_proto = execute::get_property(super_ctor, "prototype");
    if matches!(super_proto, Value::Undefined) {
        return Err(invalid_prop("superCtor.prototype", &super_proto));
    }
    define_hidden(ctor, "super_", super_ctor.clone())?;
    let proto = execute::get_property(ctor, "prototype");
    // `set_prototype_of` returns a replacement identity (a new value whose
    // `[[Prototype]]` is `super_proto`) rather than mutating `proto` in
    // place. Republish `proto -> chained` so later reads of `ctor.prototype`
    // (including the `new (ctor)` path) see the linked chain, and do all
    // subsequent work on the replacement value.
    let chained = execute::set_prototype_of(&proto, &super_proto)?;
    execute::replace_value(&proto, &chained);
    define_hidden(&chained, "constructor", ctor.clone())?;
    Ok(Value::Undefined)
}

fn define_hidden(target: &Value, key: &str, value: Value) -> Result<(), VmError> {
    let descriptor = host_api::object(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]);
    let updated = execute::define_property(target.clone(), key, descriptor)?;
    execute::replace_value(target, &updated);
    Ok(())
}

fn invalid_arg(name: &str, expected: &str, value: &Value) -> VmError {
    coded_type_error(format!(
        "The \"{name}\" argument must be of type {expected}.{}",
        crate::modules::util::invalid_arg_received(value)
    ))
}

fn invalid_prop(name: &str, value: &Value) -> VmError {
    coded_type_error(format!(
        "The \"{name}\" property must be of type object.{}",
        crate::modules::util::invalid_arg_received(value)
    ))
}

fn coded_type_error(message: String) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        ("message".to_string(), Value::String(message)),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}
