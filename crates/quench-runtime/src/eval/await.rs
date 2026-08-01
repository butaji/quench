//! Await expression runtime support
//!
//! Implements `await` for async functions by scheduling continuations
//! as microtasks using Promise.resolve() semantics.

use crate::value::{Object, Value};
use crate::JsError;
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static LAST_PENDING_AWAIT: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
}

/// Evaluator for await expressions within an async function context.
/// Takes the already-evaluated argument value and returns it wrapped in
/// Promise.resolve() semantics for chaining.
pub fn eval_await_value(arg_value: Value) -> Result<Value, JsError> {
    let promise = if is_promise(&arg_value) {
        arg_value
    } else {
        crate::builtins::promise::promise_resolve_impl_static(
            vec![arg_value],
            crate::builtins::promise::get_promise_proto(),
        )?
    };
    if let Value::Object(object) = &promise {
        if is_promise(&promise) {
            let state = object
                .borrow()
                .promise_data
                .as_ref()
                .map(|data| (data.state.clone(), data.result.clone()));
            let pending = state
                .as_ref()
                .is_some_and(|(state, _)| *state == crate::value::object::PromiseState::Pending);
            if pending {
                LAST_PENDING_AWAIT.with(|cell| *cell.borrow_mut() = Some(Rc::clone(object)));
                let _ = crate::builtins::promise::execute_pending_microtasks();
                let settled = object
                    .borrow()
                    .promise_data
                    .as_ref()
                    .map(|data| (data.state.clone(), data.result.clone()));
                if let Some((crate::value::object::PromiseState::Fulfilled, result)) = settled {
                    return Ok(result);
                }
                if let Some((crate::value::object::PromiseState::Rejected, reason)) = settled {
                    crate::value::error::set_thrown_value(reason);
                    return Err(JsError("Promise rejected".to_string()));
                }
            } else {
                let _ = crate::builtins::promise::execute_pending_microtasks();
            }
            if let Some((crate::value::object::PromiseState::Fulfilled, result)) = state {
                return Ok(result);
            }
            if let Some((crate::value::object::PromiseState::Rejected, reason)) = state {
                crate::value::error::set_thrown_value(reason);
                return Err(JsError("Promise rejected".to_string()));
            }
        }
    }
    Ok(promise)
}

pub(crate) fn take_last_pending_await() -> Option<Rc<RefCell<Object>>> {
    LAST_PENDING_AWAIT.with(|cell| cell.borrow_mut().take())
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
