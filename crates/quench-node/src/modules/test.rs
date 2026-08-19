//! `node:test` — minimal synchronous test runner.
//!
//! `test(name, fn)` executes the callback immediately. A throwing
//! callback propagates the error, failing the whole fixture; that is
//! the observable contract the conformance runner classifies on.

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub fn run(args: &[Value]) -> Result<Value, VmError> {
    let callback = args.iter().find(|arg| {
        matches!(
            arg,
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        )
    });
    let Some(callback) = callback else {
        return Ok(Value::Undefined);
    };
    quench_runtime::vm::call_value(callback, &Value::Undefined, &[])?;
    Ok(Value::Undefined)
}
