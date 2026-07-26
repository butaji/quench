//! Await expression runtime support
//!
//! Implements `await` for async functions by scheduling continuations
//! as microtasks using Promise.resolve() semantics.

use crate::builtins::promise::create_resolved_promise;
use crate::value::Value;

/// Evaluator for await expressions within an async function context.
/// Takes the already-evaluated argument value and returns it wrapped in
/// Promise.resolve() semantics for chaining.
pub fn eval_await_value(arg_value: Value) -> Value {
    // Convert to Promise using Promise.resolve() semantics:
    // - If value is already a Promise, use it
    // - Otherwise, wrap in Promise.resolve(value)
    if is_promise(&arg_value) {
        arg_value
    } else {
        Value::Object(create_resolved_promise(arg_value))
    }
}

/// Check if a value is a Promise (has Promise [[Prototype]] chain)
pub fn is_promise(value: &Value) -> bool {
    match value {
        Value::Object(obj_rc) => {
            let obj = obj_rc.borrow();
            // Check if it's a Promise object (has promise_data)
            if obj.promise_data.is_some() {
                return true;
            }
            // Check prototype chain for Promise prototype marker
            let mut current = obj.prototype.clone();
            while let Some(proto_rc) = current {
                let proto = proto_rc.borrow();
                if proto.promise_data.is_some() {
                    return true;
                }
                current = proto.prototype.clone();
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
