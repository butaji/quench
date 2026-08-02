//! Await expression runtime support
//!
//! Implements `await` for async functions by scheduling continuations as
//! microtasks using Promise semantics.

use crate::builtins::promise::enqueue_promise_reactions;
use crate::env::Environment;
use crate::value::{NativeFunction, Object, Value};
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
                if !crate::interpreter::is_in_async_generator() {
                    let _ = crate::builtins::promise::execute_pending_microtasks();
                }
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
                if !crate::interpreter::is_in_async_generator() {
                    let _ = crate::builtins::promise::execute_pending_microtasks();
                }
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

pub fn await_with_continuation(
    arg_value: Value,
    continuation: Rc<dyn Fn(Value) -> Result<Value, JsError>>,
) -> Result<Value, JsError> {
    let in_async_fn = crate::interpreter::is_in_async_function();
    let in_async_gen = crate::interpreter::is_in_async_generator();

    if !in_async_fn && !in_async_gen {
        let value = eval_await_value(arg_value)?;
        return continuation(value);
    }

    let awaited = if is_promise(&arg_value) {
        arg_value
    } else {
        crate::builtins::promise::promise_resolve_impl_static(
            vec![arg_value],
            crate::builtins::promise::get_promise_proto(),
        )?
    };

    let object = match awaited {
        Value::Object(object) => object,
        _ => return eval_await_value(awaited),
    };
    let promise_state = {
        let data = object.borrow();
        data.promise_data
            .as_ref()
            .map(|state| (state.state.clone(), state.result.clone()))
    };
    let Some((promise_state, _promise_result)) = promise_state else {
        return eval_await_value(Value::Object(object));
    };

    match promise_state.clone() {
        crate::value::object::PromiseState::Pending
        | crate::value::object::PromiseState::Fulfilled
        | crate::value::object::PromiseState::Rejected => {
            LAST_PENDING_AWAIT.with(|cell| *cell.borrow_mut() = Some(Rc::clone(&object)));
            let pending = crate::builtins::promise::create_pending_promise();
            let in_async_fn = in_async_fn;
            let in_async_gen = in_async_gen;
            let awaited_for_fulfillment = Rc::clone(&object);
            let fulfilled = {
                let continuation = Rc::clone(&continuation);
                Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
                    let arg = awaited_for_fulfillment
                        .borrow()
                        .promise_data
                        .as_ref()
                        .map(|data| data.result.clone())
                        .unwrap_or_else(|| args.first().cloned().unwrap_or(Value::Undefined));
                    if in_async_fn {
                        crate::interpreter::enter_async_function();
                    }
                    if in_async_gen {
                        crate::interpreter::enter_async_generator();
                    }
                    crate::interpreter::take_control_flow();
                    let resumed = continuation(arg.clone());
                    if in_async_gen {
                        crate::interpreter::leave_async_generator();
                    }
                    if in_async_fn {
                        crate::interpreter::leave_async_function();
                    }
                    resumed
                })))
            };
            let rejected =
                Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
                    let reason = args.first().cloned().unwrap_or(Value::Undefined);
                    crate::value::error::set_thrown_value(reason.clone());
                    Err(JsError("Promise rejected".to_string()))
                })));
            let reaction = crate::builtins::promise::create_callback_promise(
                fulfilled,
                rejected,
                Rc::clone(&pending),
            );
            crate::builtins::promise::queue_callback_on_promise(&object, reaction);
            if !matches!(promise_state, crate::value::object::PromiseState::Pending) {
                enqueue_promise_reactions(&object);
            }
            Ok(Value::Object(pending))
        }
    }
}

pub fn await_statement(
    arg_value: Value,
    tail: Vec<crate::ast::Statement>,
    env: Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let in_arrow = in_arrow_function;
    let continuation = Rc::new(move |result: Value| {
        if tail.is_empty() {
            Ok(result)
        } else {
            crate::eval::statement::eval_function_body(&tail, &env, in_arrow)
        }
    });
    await_with_continuation(arg_value, continuation)
}

pub(crate) fn take_last_pending_await() -> Option<Rc<RefCell<Object>>> {
    LAST_PENDING_AWAIT.with(|cell| cell.borrow_mut().take())
}

/// Check if a value is a Promise (has Promise [[Prototype]] chain)
pub fn is_promise(value: &Value) -> bool {
    match value {
        Value::Object(obj_rc) => {
            let obj = obj_rc.borrow();
            if obj.promise_data.is_some() {
                return true;
            }
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

#[cfg(test)]
mod for_await_tests;
