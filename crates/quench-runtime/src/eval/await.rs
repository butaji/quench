//! Await expression runtime support
//!
//! Implements `await` for async functions by scheduling continuations
//! as microtasks using Promise.resolve() semantics.

use crate::builtins::promise::create_resolved_promise;
use crate::env::Environment;
use crate::eval::call_value_with_this;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::value::{JsError, NativeFunction, Value};
use std::cell::RefCell;
use std::rc::Rc;

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

/// Create an async function body executor as a native function.
/// This runs the async function body and resolves/rejects the Promise.
pub fn create_async_function_executor(
    body_stmts: Vec<crate::ast::Statement>,
    closure: Rc<RefCell<Environment>>,
    resolve_fn: Value,
    reject_fn: Value,
) -> Value {
    let body_fn = NativeFunction::new(move |_args: Vec<Value>| {
        // Evaluate the async function body
        let result = eval_function_body(&body_stmts, &closure, false);

        match result {
            Ok(val) => {
                // Call resolve with the result
                call_value_with_this(resolve_fn, vec![val], Value::Undefined)
            }
            Err(e) => {
                // Call reject with the error
                call_value_with_this(reject_fn, vec![Value::String(e.message)], Value::Undefined)
            }
        }
    });

    Value::NativeFunction(Rc::new(body_fn))
}
