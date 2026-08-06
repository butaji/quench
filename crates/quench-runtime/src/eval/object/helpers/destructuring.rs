//! Destructuring assignment helpers.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::value::{JsError, Object, ObjectKind, PropertyFlags, Value};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static LAST_DONE_PRESENT: Cell<bool> = const { Cell::new(true) };
}

/// Box a primitive value for property assignment (ES §10.2.9 [[Set]]).
pub fn box_primitive_for_set(
    obj_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let ctor_name = match obj_val {
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::String(_) => "String",
        _ => {
            return Err(JsError(
                "box_primitive_for_set: not a primitive".to_string(),
            ))
        }
    };
    let ctor_val = env
        .borrow()
        .get(ctor_name)
        .ok_or_else(|| JsError(format!("{} not found", ctor_name)))?;
    let proto = match &ctor_val {
        Value::Object(o) => o.borrow().get("prototype"),
        Value::NativeFunction(nf) => nf
            .prototype
            .borrow()
            .as_ref()
            .map(|p| Value::Object(Rc::clone(p))),
        Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
        _ => None,
    };
    let proto_rc = match proto {
        Some(Value::Object(o)) => o,
        _ => return Err(JsError(format!("{} prototype not found", ctor_name))),
    };
    let mut boxed = Object::new(ObjectKind::Ordinary);
    boxed.prototype = Some(Rc::clone(&proto_rc));
    match obj_val {
        Value::Number(n) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            boxed.set("_value", Value::Number(*n));
        }
        Value::Boolean(b) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            boxed.set("_value", Value::Boolean(*b));
        }
        Value::Symbol(_) => {}
        _ => {}
    }
    Ok(Rc::new(RefCell::new(boxed)))
}

/// Assign to an array destructuring pattern.
pub fn assign_array_destructuring(
    bindings: &[BindingElement],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    array_destructuring_impl(bindings, value, env, false)
}

/// Initialize for-of/for-in lexical array destructuring bindings.
pub fn init_array_destructuring(
    bindings: &[BindingElement],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    array_destructuring_impl(bindings, value, env, true)
}

/// Obtain the iterator for a destructuring source value (for IteratorClose).
fn iterator_from_value(value: &Value) -> Result<Rc<RefCell<Object>>, JsError> {
    match value {
        Value::String(s) => {
            let mut arr = Object::new(ObjectKind::Array);
            arr.elements = s.chars().map(|c| Value::String(c.to_string())).collect();
            Ok(Rc::new(RefCell::new(arr)))
        }
        Value::Generator(gen) => Ok(crate::value::generator::generator_as_iterator_object(
            Rc::clone(gen),
        )),
        Value::Object(o) => obtain_iterator(o),
        _ => Err(JsError("Cannot destructure non-iterable value".to_string())),
    }
}

fn array_destructuring_impl(
    bindings: &[BindingElement],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
    init: bool,
) -> Result<(), JsError> {
    // If GENERATOR_RESUME_VALUE is defined, we're resuming after a yield in a
    // computed property name (e.g. `x[yield]`). Save it as the property key so
    // that object_destructuring_impl uses the correct key instead of re-yielding.
    // Use peek (not take) to leave the value for try_replay_yield to consume.
    let resume_val = crate::interpreter::peek_generator_resume_value();
    if !matches!(resume_val, Value::Undefined) {
        crate::interpreter::set_destructuring_yield_key(resume_val.clone());
    }

    // When a generator is resumed with an abrupt completion (generator.return()
    // / generator.throw()), the destructuring restarts from the top with the
    // Return/Throw control flow pending. Close the iterator and propagate the
    // completion BEFORE the destructuring runs — otherwise the destructuring
    // may throw on the return value (e.g. `{} = 777`) before we even get to
    // the Return check.
    match crate::interpreter::take_control_flow() {
        Some(crate::interpreter::ControlFlow::Return(val)) => {
            if let Ok(iter) = iterator_from_value(value) {
                if let Some(err) = call_iterator_return(&iter) {
                    return Err(err);
                }
            }
            crate::value::set_thrown_value(val);
            crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Return(
                crate::value::take_thrown_value().unwrap_or(Value::Undefined),
            ));
            return Ok(());
        }
        Some(crate::interpreter::ControlFlow::Throw(val)) => {
            if let Ok(iter) = iterator_from_value(value) {
                // IteratorClose on throw: original throw takes precedence.
                let _close_err = call_iterator_return(&iter);
            }
            crate::value::set_thrown_value(val);
            return Err(JsError("Generator threw".to_string()));
        }
        Some(cf) => {
            // Yield/YieldDelegate: consume the ControlFlow and GENERATOR_YIELD_VALUE so
            // that check_generator_flow (called inside array_with_iterator_impl) cannot
            // re-trigger on the same yield. The DESTRUCTURING_YIELD_KEY was already
            // saved at the top of this function.
            if matches!(
                cf,
                crate::interpreter::ControlFlow::Yield(_)
                    | crate::interpreter::ControlFlow::YieldDelegate(_)
            ) {
                let _ = crate::interpreter::take_generator_yield();
            } else {
                crate::interpreter::set_control_flow(cf);
            }
        }
        None => {}
    }

    let iter: Rc<RefCell<Object>> = if let Value::String(s) = value {
        let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
        let len = chars.len();
        let mut arr = Object::new(ObjectKind::Array);
        arr.elements = chars;
        arr.properties
            .insert("length".to_string(), Value::Number(len as f64));
        Rc::new(RefCell::new(arr))
    } else if let Value::Object(arr_rc) = value {
        if arr_rc.borrow().kind == ObjectKind::Array {
            obtain_iterator(arr_rc)?
        } else {
            obtain_iterator(arr_rc)?
        }
    } else if let Value::Generator(gen) = value {
        crate::value::generator::generator_as_iterator_object(Rc::clone(gen))
    } else {
        // Per ES §13.3.3.5 / §13.3.3.6 BindingInitialization / IteratorBindingInitialization:
        // attempting to iterate a non-iterable throws a TypeError. Set the thrown value
        // so the surrounding catch block sees a real error object (not `undefined`)
        // which is required by test262's assert.throws and the harness `assert`.
        let msg = "TypeError: value is not iterable";
        let (err, js_err) = crate::value::error::create_js_error_with_type(&msg, "TypeError");
        crate::value::set_thrown_value(err);
        return Err(js_err);
    };

    // After array_with_iterator_impl completes, check if a generator yield was
    // triggered during destructuring (e.g. `x = yield` as a default value).
    // The outer for-of handler uses this flag to suspend the generator correctly.
    // On generator resume, init_to/assign_to is called again, re-entering this
    // function — the loop is not needed as the replay mechanism in eval_yield
    // handles re-evaluation of the yield expression on the second entry.
    array_with_iterator_impl(bindings, &iter, env, init)?;
    Ok(())
}

/// Obtain an iterator object from an iterable per ES GetIterator.
pub fn obtain_iterator(o: &Rc<RefCell<Object>>) -> Result<Rc<RefCell<Object>>, JsError> {
    if o.borrow().get("next").is_some() {
        return Ok(Rc::clone(o));
    }
    let env = Rc::new(RefCell::new(Environment::new()));
    let iter_method = resolve_iterator_method(o, &env)?;
    let result = crate::eval::function::call_value_with_this(
        iter_method,
        vec![],
        Value::Object(Rc::clone(o)),
    )?;
    match result {
        Value::Object(obj) => Ok(obj),
        Value::Generator(gen) => Ok(crate::value::generator::generator_as_iterator_object(gen)),
        _ => Err(non_iterable_type_error()),
    }
}

/// Get @@iterator method from own or inherited properties (both storage key forms).
fn resolve_iterator_method(
    o: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut keys = Vec::new();
    if let Some(key) = crate::builtins::map::helpers::iterator_prop_key() {
        keys.push(key);
    }
    if let Some(Value::Symbol(sym)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    {
        let sym_key = sym.property_key();
        if !keys.iter().any(|k| k == &sym_key) {
            keys.push(sym_key);
        }
    }
    for key in keys {
        let method = crate::eval::member::eval_object_member(o, &key, Some(env))?;
        if matches!(method, Value::Function(_) | Value::NativeFunction(_)) {
            return Ok(method);
        }
    }
    Err(non_iterable_type_error())
}

fn non_iterable_type_error() -> JsError {
    let (_, js_err) =
        crate::value::error::create_js_error_with_type("undefined is not iterable", "TypeError");
    js_err
}

/// Assign destructuring bindings using an iterator.
pub fn assign_array_with_iterator(
    bindings: &[BindingElement],
    iterator: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    array_with_iterator_impl(bindings, iterator, env, false)
}

/// Check for generator yield or pending control flow after an expression evaluation
/// step. If the generator yielded, re-set GENERATOR_YIELD_VALUE so the outer
/// caller (for-of init/assign) can detect it and suspend correctly.
/// If there's a Return or Throw control flow, close the iterator and propagate.
#[must_use]
fn check_generator_flow(
    iterator: &Rc<RefCell<Object>>,
    iterator_done: &mut bool,
) -> Option<Result<(), JsError>> {
    // Check for generator yield flag first (set by yield inside computed property
    // key evaluation in touch_assignment_target). This is NOT the same as
    // ControlFlow::Yield, which is set by the outer eval loop.
    if crate::interpreter::peek_generator_yield() {
        let yielded = crate::interpreter::take_generator_yield().unwrap_or(Value::Undefined);
        crate::interpreter::set_generator_yield(yielded);
        return Some(Ok(()));
    }
    let cf = crate::interpreter::take_control_flow()?;
    match cf {
        crate::interpreter::ControlFlow::Return(val) => {
            if !*iterator_done {
                let close_err = call_iterator_return(iterator);
                if let Some(err) = close_err {
                    return Some(Err(err));
                }
            }
            crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Return(val));
            Some(Ok(()))
        }
        crate::interpreter::ControlFlow::Throw(val) => {
            if !*iterator_done {
                let _close_err = call_iterator_return(iterator);
            }
            crate::value::set_thrown_value(val);
            Some(Err(JsError("Generator threw".to_string())))
        }
        crate::interpreter::ControlFlow::Yield(yielded_val) => {
            // Consume GENERATOR_YIELD_VALUE so subsequent check_generator_flow calls
            // (for later bindings in the same destructuring) don't re-trigger. The
            // first call handles the yield; later calls should return None.
            let _ = crate::interpreter::take_generator_yield();
            crate::interpreter::set_generator_yield(yielded_val);
            Some(Ok(()))
        }
        crate::interpreter::ControlFlow::YieldDelegate(yielded_val) => {
            let _ = crate::interpreter::take_generator_yield();
            crate::interpreter::set_generator_yield(yielded_val);
            Some(Ok(()))
        }
        other => {
            crate::interpreter::set_control_flow(other);
            None
        }
    }
}

fn array_with_iterator_impl(
    bindings: &[BindingElement],
    iterator: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
    init: bool,
) -> Result<(), JsError> {
    let mut index = 0;
    let mut iterator_done = false;
    let apply = |binding: &BindingElement, val: &Value| -> Result<(), JsError> {
        if init {
            init_binding_elem(binding, val, env)
        } else {
            assign_binding_elem(binding, val, env)
        }
    };
    for binding in bindings {
        // When the generator is resumed with a pending Return/Throw (from
        // generator.return()/throw()), close the iterator and propagate
        // BEFORE re-running iterator steps for this binding.
        if let Some(result) = check_generator_flow(iterator, &mut iterator_done) {
            return result;
        }
        if let BindingElement::Rest(inner) = binding {
            if let BindingElement::AssignmentTarget(target) = inner.as_ref() {
                // Per ES §13.15.5.3 IteratorDestructuringAssignmentEvaluation step 1,
                // the full reference evaluation happens before IteratorStep. If the
                // computed property key throws (e.g. `...{}[thrower()]`) or yields,
                // the iterator must be closed and body must not execute.
                if let Err(error) = crate::eval::object::touch_assignment_target(target, env) {
                    if !iterator_done {
                        let _close_err = call_iterator_return(iterator);
                    }
                    return Err(error);
                }
                if let Some(result) = check_generator_flow(iterator, &mut iterator_done) {
                    return result;
                }
            }
            let rest_array = collect_remaining_array(iterator, &mut index, env)?;
            // collect_remaining_array consumes the iterator to completion
            iterator_done = true;
            if let Err(error) = apply(inner, &rest_array) {
                if !iterator_done {
                    // Per ES §7.4.6 step 5: original error takes precedence
                    let _close_err = call_iterator_return(iterator);
                }
                return Err(error);
            }
            // Yield from apply must propagate to array_destruct_impl.
            return Ok(());
        }
        if let BindingElement::AssignmentTarget(target) = binding {
            if let Err(error) = crate::eval::object::touch_assignment_target(target, env) {
                if !iterator_done {
                    let _close_err = call_iterator_return(iterator);
                }
                return Err(error);
            }
            if let Some(result) = check_generator_flow(iterator, &mut iterator_done) {
                return result;
            }
        }
        let target_reference = crate::eval::object::take_destructuring_member_reference();
        let step_result = take_iterator_step(iterator, &mut index, env);
        crate::eval::object::set_destructuring_member_reference(target_reference);
        let (elem_value, done) = step_result?;
        iterator_done = done;
        if let Err(error) = apply(binding, &elem_value) {
            let original = crate::value::take_thrown_value();
            // Always close the iterator on destructuring error, regardless of done state.
            // Per ES spec, IteratorClose should be called if the iterator wasn't
            // consumed normally. The done state from take_iterator_step reflects
            // whether next() returned { done: true }, but we should still clean up.
            if !crate::interpreter::peek_generator_yield() {
                let _close_err = call_iterator_return(iterator);
            }
            if let Some(thrown) = original {
                crate::value::set_thrown_value(thrown);
            }
            return Err(error);
        }
        // NOTE: No check_generator_flow here. If yield was triggered during
        // apply (e.g. from a nested destructuring computed property key),
        // it must propagate to array_destruct_impl's loop so the outer
        // for-of handler can see it. The check at the top of this function
        // (line ~302) handles Return/Throw from generator.return/throw.
    }
    if !iterator_done {
        // If a generator yield is pending, the destructuring was suspended
        // mid-evaluation by a yield expression (e.g. `[{} = yield]`). Do NOT
        // close the iterator here — it will be closed on generator resume
        // (via the control flow check in array_destructuring_impl for
        // .return()/.throw(), or via this same code in a fresh call for
        // normal .next() resume). Closing it now would double-close on
        // .return() resume, producing two iterator.return() calls.
        if crate::interpreter::peek_generator_yield() {
            crate::eval::iteration::stage_pending_destructuring_iterator(Rc::clone(iterator));
        } else {
            if let Some(err) = call_iterator_return(iterator) {
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Collect all remaining elements from an array or iterator starting at `index`.
fn collect_remaining_array(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if iterator.borrow().kind == ObjectKind::Array {
        let remaining = {
            let borrowed = iterator.borrow();
            if *index < borrowed.elements.len() {
                borrowed.elements[*index..].to_vec()
            } else {
                Vec::new()
            }
        };
        *index = iterator.borrow().elements.len();
        return Ok(Value::Object(Rc::new(RefCell::new(
            Object::new_array_from(remaining),
        ))));
    }
    let mut items = Vec::new();
    loop {
        match take_iterator_step(iterator, index, env) {
            Ok((Value::Undefined, true)) => break,
            Ok((v, _)) => items.push(v),
            Err(error) => return Err(error),
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(
        Object::new_array_from(items),
    ))))
}

/// Take the next value from an iterator (or array-like).
pub fn take_iterator_value(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    take_iterator_step(iterator, index, env).map(|(value, _)| value)
}

/// Cached [[NextMethod]] for an iterator record (non-enumerable internal slot).
const ITERATOR_NEXT_METHOD: &str = "\0iterNextMethod";

fn cached_iterator_next_method(
    iterator: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(cached) = iterator.borrow().get_own(ITERATOR_NEXT_METHOD) {
        return Ok(cached);
    }
    let next_fn = crate::eval::member::eval_object_member(iterator, "next", Some(env))?;
    iterator.borrow_mut().define(
        ITERATOR_NEXT_METHOD,
        next_fn.clone(),
        PropertyFlags {
            value: Some(next_fn.clone()),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    Ok(next_fn)
}

/// Take the next iterator step, returning `(value, done)`.
pub fn take_iterator_step(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
) -> Result<(Value, bool), JsError> {
    take_iterator_step_with_args(iterator, index, env, vec![], false, false, false)
        .and_then(|result| to_iterator_tuple(result, env, false, index))
}

pub fn take_iterator_step_with_mode(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
    async_from_sync: bool,
    await_result: bool,
) -> Result<IteratorStepResult, JsError> {
    take_iterator_step_with_args(
        iterator,
        index,
        env,
        vec![],
        false,
        async_from_sync,
        await_result,
    )
}

#[derive(Debug, Clone)]
pub enum IteratorStepResult {
    Ready((Value, bool)),
    Pending(Rc<RefCell<Object>>),
}

pub fn take_iterator_step_with_value(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
    value: Value,
) -> Result<(Value, bool), JsError> {
    take_iterator_step_with_args(iterator, index, env, vec![value], true, false, true)
        .and_then(|result| to_iterator_tuple(result, env, true, index))
}

pub fn take_iterator_result_with_value(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
    value: Value,
) -> Result<(Rc<RefCell<Object>>, bool), JsError> {
    let result = call_iterator_next(iterator, env, vec![value])?;
    let Value::Object(result) = result else {
        return Err(iterator_result_type_error());
    };
    let done = crate::eval::member::eval_object_member(&result, "done", Some(env))?;
    LAST_DONE_PRESENT.with(|cell| cell.set(has_property(&result, "done")));
    if !crate::value::to_bool(&done) {
        *index += 1;
    }
    Ok((result, crate::value::to_bool(&done)))
}

pub fn take_last_done_present() -> bool {
    LAST_DONE_PRESENT.with(|cell| cell.replace(true))
}

fn take_iterator_step_with_args(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
    args: Vec<Value>,
    read_done_value: bool,
    async_from_sync: bool,
    await_result: bool,
) -> Result<IteratorStepResult, JsError> {
    if iterator.borrow().kind == ObjectKind::Array {
        let value = {
            let borrowed = iterator.borrow();
            if *index < borrowed.elements.len() {
                Some(borrowed.elements[*index].clone())
            } else {
                borrowed.properties.get(&index.to_string()).cloned()
            }
        };
        if value.is_none() && *index >= iterator.borrow().elements.len() {
            return Ok(IteratorStepResult::Ready((Value::Undefined, true)));
        }
        *index += 1;
        return Ok(IteratorStepResult::Ready((
            value.unwrap_or(Value::Undefined),
            false,
        )));
    }
    let result = call_iterator_next(iterator, env, args)?;
    if async_from_sync {
        return async_from_sync_step(result, index, env, read_done_value);
    }
    let result = if await_result {
        await_iterator_step_result(result)?
    } else {
        IteratorStepResult::Ready((result, false))
    };
    match result {
        IteratorStepResult::Pending(promise) => Ok(IteratorStepResult::Pending(promise)),
        IteratorStepResult::Ready((result, _)) => {
            let tuple = iterator_result_to_tuple(result, env, index, read_done_value)?;
            Ok(IteratorStepResult::Ready(tuple))
        }
    }
}

fn call_iterator_next(
    iterator: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    let next_fn = cached_iterator_next_method(iterator, env)?;
    if !next_fn.is_callable() {
        let (value, error) =
            crate::value::create_js_error_with_type("iterator.next is not callable", "TypeError");
        crate::value::set_thrown_value(value);
        return Err(error);
    }
    let iter_this = Value::Object(Rc::clone(iterator));
    let result = match next_fn {
        Value::Object(obj) => crate::eval::function::call_value_with_this(
            Value::Object(Rc::clone(&obj)),
            args.clone(),
            iter_this.clone(),
        )?,
        other => crate::eval::function::call_value_with_this(other, args, iter_this)?,
    };
    if crate::value::take_thrown_value().is_some() {
        return Err(JsError("TypeError: iterator threw".to_string()));
    }
    Ok(result)
}

fn iterator_result_type_error() -> JsError {
    let (value, error) = crate::value::create_js_error_with_type(
        "Iterator result interface is not an object",
        "TypeError",
    );
    crate::value::set_thrown_value(value);
    error
}

fn async_from_sync_step(
    result: Value,
    index: &mut usize,
    env: &Rc<RefCell<Environment>>,
    read_done_value: bool,
) -> Result<IteratorStepResult, JsError> {
    // Async-from-Sync fallback: mirror AsyncFromSyncIteratorContinuation.
    // PromiseResolve the value first (fires the constructor lookup on promise
    // values, inside next() in V8), then await the record wrapped in a promise
    // so the result-await's PromiseResolve also sees a promise.
    let Value::Object(result_obj) = &result else {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Iterator result interface is not an object",
            "TypeError",
        );
        return Err(js_err);
    };
    let done = crate::eval::member::eval_object_member(result_obj, "done", Some(env))?;
    let value = crate::eval::member::eval_object_member(result_obj, "value", Some(env))?;
    // Await the record through a promise so the result-await's
    // PromiseResolve sees a promise (constructor lookup fires). Created
    // pending and settled AFTER the value-wrapper lookup, so an abrupt
    // completion there can reject it (IfAbruptRejectPromise).
    let wrapped = Rc::new(RefCell::new(crate::value::Object::with_prototype(
        crate::value::ObjectKind::Promise,
        crate::builtins::promise::get_promise_proto(),
    )));
    wrapped.borrow_mut().promise_data = Some(crate::value::object::PromiseObjectData::new());
    // The value-wrapper PromiseResolve fires the constructor lookup on
    // promise values (inside next() in V8).
    let value_wrapper = match crate::builtins::promise::promise_resolve_impl_static(
        vec![value.clone()],
        crate::builtins::promise::get_promise_proto(),
    ) {
        Ok(wrapper) => {
            crate::builtins::promise::settle_resolve(&wrapped, result);
            wrapper
        }
        Err(error) => {
            let reason = crate::value::take_thrown_value()
                .unwrap_or_else(|| Value::String(error.to_string()));
            crate::builtins::promise::settle_reject(&wrapped, reason);
            Value::Undefined
        }
    };
    await_async_iterator_result(Value::Object(wrapped))?;
    if crate::value::to_bool(&done) {
        let value = if read_done_value {
            value
        } else {
            Value::Undefined
        };
        *index += 1;
        return Ok(IteratorStepResult::Ready((value, true)));
    }
    // The unwrap hop: wait for the value-wrapper to settle.
    let value = crate::eval::iteration::await_for_await_of_promise(value_wrapper)?;
    *index += 1;
    Ok(IteratorStepResult::Ready((value, false)))
}

fn await_iterator_step_result(result: Value) -> Result<IteratorStepResult, JsError> {
    let next_result = result.clone();
    let promise = crate::builtins::promise::promise_resolve_impl_static(
        vec![next_result.clone()],
        crate::builtins::promise::get_promise_proto(),
    )?;
    let Value::Object(promise) = promise else {
        return Ok(IteratorStepResult::Ready((next_result, false)));
    };
    if promise.borrow().promise_data.is_some() {
        return Ok(IteratorStepResult::Pending(promise));
    }
    Ok(IteratorStepResult::Ready((result, false)))
}

fn to_iterator_tuple(
    result: IteratorStepResult,
    env: &Rc<RefCell<Environment>>,
    read_done_value: bool,
    index: &mut usize,
) -> Result<(Value, bool), JsError> {
    match result {
        IteratorStepResult::Ready(value) => Ok(value),
        IteratorStepResult::Pending(promise) => iterator_result_to_tuple(
            crate::eval::iteration::await_for_await_of_promise(Value::Object(promise))?,
            env,
            index,
            read_done_value,
        ),
    }
}

fn iterator_result_to_tuple(
    result: Value,
    env: &Rc<RefCell<Environment>>,
    index: &mut usize,
    read_done_value: bool,
) -> Result<(Value, bool), JsError> {
    let Value::Object(result_obj) = result else {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Iterator result interface is not an object",
            "TypeError",
        );
        return Err(js_err);
    };
    let done = crate::eval::member::eval_object_member(&result_obj, "done", Some(env))?;
    LAST_DONE_PRESENT.with(|cell| cell.set(has_property(&result_obj, "done")));
    if crate::value::to_bool(&done) {
        let value = if read_done_value {
            crate::eval::member::eval_object_member(&result_obj, "value", Some(env))?
        } else {
            Value::Undefined
        };
        return Ok((value, true));
    }
    let value = crate::eval::member::eval_object_member(&result_obj, "value", Some(env))?;
    *index += 1;
    Ok((value, false))
}

fn has_property(object: &Rc<RefCell<Object>>, key: &str) -> bool {
    let borrowed = object.borrow();
    if borrowed.properties.contains_key(key)
        || borrowed.getters.contains_key(key)
        || borrowed.setters.contains_key(key)
    {
        return true;
    }
    let prototype = borrowed.prototype.clone();
    drop(borrowed);
    prototype.is_some_and(|prototype| has_property(&prototype, key))
}

pub(crate) fn await_async_iterator_result(result: Value) -> Result<Value, JsError> {
    if !crate::interpreter::is_in_async_generator() && !crate::interpreter::is_in_async_function() {
        return Ok(result);
    }
    let promise = crate::builtins::promise::promise_resolve_impl_static(
        vec![result],
        crate::builtins::promise::get_promise_proto(),
    )?;
    let Value::Object(promise) = promise else {
        return Ok(Value::Undefined);
    };
    let mut data = promise.borrow().promise_data.clone();
    if data
        .as_ref()
        .is_some_and(|data| data.state == crate::value::object::PromiseState::Pending)
    {
        crate::builtins::promise::execute_pending_microtasks()?;
        data = promise.borrow().promise_data.clone();
    } else {
        crate::builtins::promise::execute_pending_microtask()?;
    }
    match data.map(|data| (data.state, data.result)) {
        Some((crate::value::object::PromiseState::Fulfilled, value)) => Ok(value),
        Some((crate::value::object::PromiseState::Rejected, reason)) => {
            crate::value::set_thrown_value(reason);
            Err(JsError("Async iterator result rejected".to_string()))
        }
        _ => Ok(Value::Undefined),
    }
}

/// Call iterator.return, returning an error if it throws or returns a non-Object.
#[must_use = "iterator.return() errors must be handled per ES spec §7.4.6"]
pub fn call_iterator_return(iterator: &Rc<RefCell<Object>>) -> Option<JsError> {
    let iter_this = Value::Object(Rc::clone(iterator));
    let return_call = invoke_iterator_return(iterator, iter_this, vec![]);
    match return_call {
        IteratorReturnResult::Skipped => None,
        IteratorReturnResult::Throw(err) => Some(err),
        IteratorReturnResult::Value(val) => iterator_close_type_error(val),
    }
}

pub fn call_iterator_return_done(
    iterator: &Rc<RefCell<Object>>,
    argument: Value,
) -> Result<Option<bool>, JsError> {
    let iter_this = Value::Object(Rc::clone(iterator));
    match invoke_iterator_return(iterator, iter_this, vec![argument]) {
        IteratorReturnResult::Skipped => Ok(None),
        IteratorReturnResult::Throw(error) => Err(error),
        IteratorReturnResult::Value(value) => {
            if let Some(error) = iterator_close_type_error(value.clone()) {
                return Err(error);
            }
            let Value::Object(object) = value else {
                unreachable!()
            };
            let done = crate::eval::member::eval_object_member(&object, "done", None)?;
            let done = crate::value::to_bool(&done);
            if done {
                crate::eval::member::eval_object_member(&object, "value", None)?;
            }
            Ok(Some(done))
        }
    }
}

enum IteratorReturnResult {
    Skipped,
    Throw(JsError),
    Value(Value),
}

fn invoke_iterator_return(
    iterator: &Rc<RefCell<Object>>,
    iter_this: Value,
    args: Vec<Value>,
) -> IteratorReturnResult {
    let saved_throw = crate::value::take_thrown_value();
    let result = invoke_iterator_return_inner(iterator, iter_this, args);
    match result {
        IteratorReturnResult::Throw(_) if saved_throw.is_some() => {
            if let Some(thrown) = saved_throw {
                crate::value::set_thrown_value(thrown);
            }
            IteratorReturnResult::Skipped
        }
        IteratorReturnResult::Throw(err) => IteratorReturnResult::Throw(err),
        other => {
            if let Some(thrown) = saved_throw {
                crate::value::set_thrown_value(thrown);
            }
            other
        }
    }
}

fn invoke_iterator_return_inner(
    iterator: &Rc<RefCell<Object>>,
    iter_this: Value,
    args: Vec<Value>,
) -> IteratorReturnResult {
    let binding = iterator.borrow_mut();
    // Resolve the "return" method per GetMethod (ES §7.3.9):
    // call the accessor if present, then check the value.
    let resolved = if let Some(getter) = binding.get_getter("return") {
        if let Some(func) = getter.func.clone() {
            match crate::eval::function::call_value_with_this(func, args.clone(), iter_this.clone())
            {
                Ok(val) => val,
                Err(err) => return IteratorReturnResult::Throw(err),
            }
        } else {
            let params: Vec<crate::ast::Param> = Vec::new();
            let body: Vec<crate::ast::Statement> = (*getter.body).clone();
            let closure = getter.closure.clone();
            match crate::eval::function::call_value_with_this(
                crate::value::Value::Function(crate::value::ValueFunction::new_arrow(
                    params,
                    Box::new(crate::ast::ArrowBody::Block(std::rc::Rc::new(body))),
                    closure,
                )),
                args.clone(),
                iter_this.clone(),
            ) {
                Ok(val) => val,
                Err(err) => return IteratorReturnResult::Throw(err),
            }
        }
    } else {
        let resolved = binding.get("return").unwrap_or(Value::Undefined);
        drop(binding);
        resolved
    };
    // GetMethod step: if func is undefined or null, return undefined.
    if matches!(resolved, Value::Undefined | Value::Null) {
        return IteratorReturnResult::Skipped;
    }
    // Check callable.
    if !matches!(
        resolved,
        Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
    ) {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "iterator.return is not a function",
            "TypeError",
        );
        return IteratorReturnResult::Throw(js_err);
    }
    finish_iterator_return_call(crate::eval::function::call_value_with_this(
        resolved, args, iter_this,
    ))
}

fn finish_iterator_return_call(result: Result<Value, JsError>) -> IteratorReturnResult {
    match result {
        Ok(val) => IteratorReturnResult::Value(val),
        Err(err) => IteratorReturnResult::Throw(err),
    }
}

fn iterator_close_type_error(val: Value) -> Option<JsError> {
    if matches!(val, Value::Object(_)) {
        return None;
    }
    let saved_throw = crate::value::take_thrown_value();
    let (_, js_err) = crate::value::error::create_js_error_with_type(
        "Iterator result interface is not an object",
        "TypeError",
    );
    if let Some(thrown) = saved_throw {
        crate::value::set_thrown_value(thrown);
    }
    Some(js_err)
}

/// Assign to an object destructuring pattern.
pub fn assign_object_destructuring(
    props: &[(PropertyKey, BindingElement)],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    object_destructuring_impl(props, value, env, false)
}

/// Initialize for-of/for-in lexical object destructuring bindings.
pub fn init_object_destructuring(
    props: &[(PropertyKey, BindingElement)],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    object_destructuring_impl(props, value, env, true)
}

fn object_destructuring_impl(
    props: &[(PropertyKey, BindingElement)],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
    init: bool,
) -> Result<(), JsError> {
    // Handle pending Return/Throw from generator.return()/throw().
    // If the generator was resumed with Return/Throw, re-set the control
    // flow so it propagates correctly.
    if let Some(cf) = crate::interpreter::take_control_flow() {
        match cf {
            crate::interpreter::ControlFlow::Return(val) => {
                crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Return(val));
                return Ok(());
            }
            crate::interpreter::ControlFlow::Throw(val) => {
                crate::value::set_thrown_value(val);
                return Err(JsError("Generator threw".to_string()));
            }
            crate::interpreter::ControlFlow::Yield(_)
            | crate::interpreter::ControlFlow::YieldDelegate(_) => {
                // Consume the stale control flow. DESTRUCTURING_YIELD_KEY
                // was already saved from the resume value above.
                let _ = crate::interpreter::take_generator_yield();
            }
            other => {
                crate::interpreter::set_control_flow(other);
            }
        }
    }

    let obj = match value {
        Value::Null | Value::Undefined => {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot destructure non-object value",
                "TypeError",
            );
            return Err(js_err);
        }
        Value::Object(o) => o.clone(),
        other => {
            let Value::Object(o) = crate::value::to_object(other)? else {
                return Err(JsError("Cannot destructure non-object value".to_string()));
            };
            o
        }
    };
    let mut excluded = std::collections::HashSet::new();
    let mut rest_binding: Option<&BindingElement> = None;
    let apply = |binding: &BindingElement, val: &Value| -> Result<(), JsError> {
        if init {
            init_binding_elem(binding, val, env)
        } else {
            assign_binding_elem(binding, val, env)
        }
    };

    for (key, binding) in props {
        if is_object_rest_key(key) {
            rest_binding = Some(binding);
            continue;
        }
        if let BindingElement::AssignmentTarget(target) = binding {
            let key_str = compute_property_key(key, env)?;
            // If yield was triggered during computed property key evaluation
            // (e.g. `x[yield]`), re-extract the property name with the resumed
            // value — that is the value passed to generator.next(val).  Then retry
            // the assignment with the correct property name.
            if crate::interpreter::peek_destructuring_yield_key() {
                // Yield was triggered during computed property key evaluation.
                // Use the key saved from generator.next(val) as the property name.
                let resumed_key = crate::interpreter::take_destructuring_yield_key().unwrap();
                let key_str = match &resumed_key {
                    Value::Symbol(s) => s.property_key(),
                    _ => crate::value::to_js_string(&resumed_key),
                };
                excluded.insert(key_str.clone());
                let prop_value =
                    crate::eval::member::eval_object_member(&obj, &key_str, Some(env))?;
                if init {
                    crate::eval::object::init_to(target, &prop_value, env)?;
                } else {
                    crate::eval::object::assign_to(target, &prop_value, env)?;
                }
            } else {
                excluded.insert(key_str.clone());
                crate::eval::object::touch_assignment_target(target, env)?;
                let target_reference = crate::eval::object::take_destructuring_member_reference();
                // If the computed key evaluation triggered a generator yield
                // (e.g. `yield` in `x[yield]`), check if this is a real
                // suspension (ControlFlow::Yield is set) or a stale flag from
                // yield resolution on resume (only GENERATOR_YIELD_VALUE).
                if crate::interpreter::peek_generator_yield() {
                    let cf = crate::interpreter::take_control_flow();
                    match cf {
                        Some(crate::interpreter::ControlFlow::Yield(_))
                        | Some(crate::interpreter::ControlFlow::YieldDelegate(_)) => {
                            // Real suspension; restore control flow for
                            // the outer handler to detect.
                            crate::interpreter::set_control_flow(cf.unwrap());
                            return Ok(());
                        }
                        other => {
                            // Stale flag from yield resolution on resume.
                            if let Some(cf_val) = other {
                                crate::interpreter::set_control_flow(cf_val);
                            }
                            let _ = crate::interpreter::take_generator_yield();
                        }
                    }
                }
                let prop_value =
                    crate::eval::member::eval_object_member(&obj, &key_str, Some(env))?;
                crate::eval::object::set_destructuring_member_reference(target_reference);
                if init {
                    crate::eval::object::init_to(target, &prop_value, env)?;
                } else {
                    crate::eval::object::assign_to(target, &prop_value, env)?;
                }
            }
        } else {
            let key_str = extract_destructure_key(key, env)?;
            excluded.insert(key_str.clone());
            let target_reference = if let BindingElement::Default(inner, _) = binding {
                if let BindingElement::AssignmentTarget(target) = inner.as_ref() {
                    crate::eval::object::touch_assignment_target(target, env)?;
                    crate::eval::object::take_destructuring_member_reference()
                } else {
                    let target = crate::eval::object::binding_pattern_expression(binding.clone());
                    crate::eval::object::touch_assignment_target(&target, env)?;
                    crate::eval::object::take_destructuring_member_reference()
                }
            } else {
                None
            };
            let prop_value = crate::eval::member::eval_object_member(&obj, &key_str, Some(env))?;
            crate::eval::object::set_destructuring_member_reference(target_reference);
            apply(binding, &prop_value)?;
        }
    }

    if let Some(binding) = rest_binding {
        let rest_val = copy_enumerable_own_properties(&obj, &excluded, env)?;
        apply(binding, &rest_val)?;
    }
    Ok(())
}

fn is_object_rest_key(key: &PropertyKey) -> bool {
    matches!(key, PropertyKey::Ident(s) if s == "...")
}

fn copy_enumerable_value(
    obj: &Rc<RefCell<Object>>,
    key: &str,
    src: &Object,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(getter) = src.get_getter(key) {
        return crate::eval::object::call_getter(obj, getter, env);
    }
    if let Some(idx) = crate::value::object::helpers::as_array_index(key) {
        if idx < src.elements.len() && !src.holes.contains(&idx) {
            return Ok(src.elements[idx].clone());
        }
    }
    Ok(src.properties.get(key).cloned().unwrap_or(Value::Undefined))
}

fn copy_key_to_rest(rest: &mut Object, key: &str, val: Value) {
    if key.contains('\0') {
        rest.set_symbol(key, val);
    } else {
        rest.set(key, val);
    }
}

fn string_exotic_source_string(obj: &Object) -> Option<String> {
    if obj.exotic_kind != Some(crate::value::kind::ExoticKind::String) {
        return None;
    }
    if let Some(Value::String(s)) = obj.get("_value") {
        return Some(s);
    }
    if obj.properties.contains_key("1") {
        return None;
    }
    if let Some(Value::String(s)) = obj.get("0") {
        return Some(s);
    }
    if obj.elements.len() == 1 {
        if let Value::String(s) = &obj.elements[0] {
            return Some(s.clone());
        }
    }
    None
}

fn copy_enumerable_own_properties(
    obj: &Rc<RefCell<Object>>,
    excluded: &std::collections::HashSet<String>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut rest = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = crate::builtins::get_object_prototype() {
        rest.prototype = Some(prototype);
    }
    if let Some(keys) = crate::eval::object::proxy_own_keys(obj)? {
        for key in keys {
            let property_key = match &key {
                Value::String(key) => key.clone(),
                Value::Symbol(symbol) => symbol.property_key(),
                _ => continue,
            };
            if excluded.contains(&property_key)
                || crate::eval::object::proxy_property_is_enumerable(obj, &key)? == Some(false)
            {
                continue;
            }
            let value = crate::eval::member::eval_object_member_value(obj, &key, None)?;
            copy_key_to_rest(&mut rest, &property_key, value);
        }
        return Ok(Value::Object(Rc::new(RefCell::new(rest))));
    }
    let src = obj.borrow();
    if let Some(s) = string_exotic_source_string(&src) {
        for (i, ch) in s.chars().enumerate() {
            let key = i.to_string();
            if excluded.contains(&key) {
                continue;
            }
            copy_key_to_rest(&mut rest, &key, Value::String(ch.to_string()));
        }
    } else {
        // Copy enumerable string-keyed own properties (per OrdinaryOwnPropertyKeys order)
        for key in crate::value::object::enumerable_own_keys(&src) {
            if excluded.contains(&key) {
                continue;
            }
            let val = copy_enumerable_value(obj, &key, &src, env)?;
            copy_key_to_rest(&mut rest, &key, val);
        }
        // Copy enumerable symbol-keyed own properties (per ES2025 RestDestructuringAssignmentEvaluation)
        for key in src.symbol_properties.keys() {
            if excluded.contains(key) {
                continue;
            }
            let val = copy_enumerable_value(obj, key, &src, env)?;
            copy_key_to_rest(&mut rest, key, val);
        }
        // Symbol-keyed getters
        for (key, _getter) in &src.getters {
            if !key.contains('\0') {
                continue; // not a symbol key
            }
            if excluded.contains(key) {
                continue;
            }
            if !src.is_enumerable(key) {
                continue;
            }
            let val = copy_enumerable_value(obj, key, &src, env)?;
            copy_key_to_rest(&mut rest, key, val);
        }
        // Symbol-keyed setters (if no getter)
        for (key, _) in &src.setters {
            if !key.contains('\0') {
                continue;
            }
            if excluded.contains(key) || src.getters.contains_key(key) {
                continue;
            }
            if !src.is_enumerable(key) {
                continue;
            }
            let val = copy_enumerable_value(obj, key, &src, env)?;
            copy_key_to_rest(&mut rest, key, val);
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(rest))))
}

/// Compute the string key for a property key.
pub fn compute_property_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => {
            let value = eval_expression(expr, env, false)?;
            Ok(match value {
                crate::Value::Symbol(symbol) => symbol.property_key(),
                value => crate::value::to_js_string(&value),
            })
        }
    }
}

/// Extract string key from a destructure property key.
pub fn extract_destructure_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => {
            let value = eval_expression(expr, env, false)?;
            Ok(match value {
                crate::Value::Symbol(symbol) => symbol.property_key(),
                value => crate::value::to_js_string(&value),
            })
        }
    }
}

/// Assign to a single binding element (identifier, pattern, or default).
pub fn assign_binding_elem(
    binding: &BindingElement,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    assign_binding_elem_with_default(binding, value, env, None, false)
}

/// Initialize a declared binding element (for-of/for-in lexical head).
pub fn init_binding_elem(
    binding: &BindingElement,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    assign_binding_elem_with_default(binding, value, env, None, true)
}

fn assign_binding_elem_with_default(
    binding: &BindingElement,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
    default_expr: Option<&Expression>,
    init: bool,
) -> Result<(), JsError> {
    match binding {
        BindingElement::Identifier(name) if name == "__hole" => Ok(()),
        BindingElement::Identifier(name) => {
            if init {
                init_to_identifier(name, value, env, default_expr)
            } else {
                assign_to_identifier(name, value, env, default_expr)
            }
        }
        BindingElement::ArrayPattern(bindings) => {
            if init {
                init_array_destructuring(bindings, value, env)
            } else {
                assign_array_destructuring(bindings, value, env)
            }
        }
        BindingElement::ObjectPattern(props) => {
            if init {
                init_object_destructuring(props, value, env)
            } else {
                assign_object_destructuring(props, value, env)
            }
        }
        BindingElement::Default(binding, default) => {
            let (value, name_default) = if matches!(value, Value::Undefined) {
                let target_reference = crate::eval::object::take_destructuring_member_reference();
                let default_val = eval_expression(default, env, false)?;
                crate::eval::object::set_destructuring_member_reference(target_reference);
                // If the default evaluation triggered a generator yield or
                // pending control flow, propagate without proceeding.
                if crate::interpreter::peek_generator_yield() {
                    return Ok(());
                }
                if let Some(cf) = crate::interpreter::take_control_flow() {
                    match cf {
                        crate::interpreter::ControlFlow::Return(val) => {
                            crate::interpreter::set_control_flow(
                                crate::interpreter::ControlFlow::Return(val),
                            );
                            return Ok(());
                        }
                        crate::interpreter::ControlFlow::Throw(val) => {
                            crate::value::set_thrown_value(val);
                            return Err(JsError("Generator threw".to_string()));
                        }
                        other => {
                            crate::interpreter::set_control_flow(other);
                        }
                    }
                }
                (default_val, Some(default.as_ref()))
            } else {
                (value.clone(), None)
            };
            assign_binding_elem_with_default(binding, &value, env, name_default, init)
        }
        BindingElement::Rest(_) => Ok(()),
        BindingElement::AssignmentTarget(target) => {
            if init {
                crate::eval::object::init_to(target, value, env)
            } else {
                crate::eval::object::assign_to(target, value, env)
            }
        }
    }
}

fn prepare_identifier_binding_value(
    name: &str,
    value: &Value,
    default_expr: Option<&Expression>,
) -> Value {
    match value {
        Value::Function(f)
            if f.name.is_none() && default_expr.is_some_and(is_anonymous_function_definition) =>
        {
            let mut cloned = f.clone();
            cloned.name = Some(name.to_string());
            let _ = cloned.set_property("name", Value::String(name.to_string()));
            Value::Function(cloned)
        }
        Value::Class(c) => {
            let has_name = c.name.is_some()
                || c.static_methods.iter().any(|(k, _, _, _, _)| match k {
                    crate::ast::PropertyKey::Ident(s) | crate::ast::PropertyKey::String(s) => {
                        s == "name"
                    }
                    _ => false,
                });
            if !has_name {
                let mut cloned = c.as_ref().clone();
                cloned.name = Some(name.to_string());
                Value::Class(Box::new(cloned))
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

/// Assign a value to an identifier (variable reference).
pub fn assign_to_identifier(
    name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
    default_expr: Option<&Expression>,
) -> Result<(), JsError> {
    let value = prepare_identifier_binding_value(name, value, default_expr);

    if let Some(scope) = crate::eval::object::take_destructuring_identifier_reference(name) {
        if scope.is_none() && !crate::interpreter::is_strict_mode() {
            if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
                global_obj.borrow_mut().set(name, value.clone());
                return Ok(());
            }
        }
        let Some(scope) = scope else { return Ok(()) };
        if scope.borrow().is_tdz(name) {
            let msg = format!(
                "ReferenceError: Cannot access '{}' before initialization",
                name
            );
            let (err, js_err) =
                crate::value::error::create_js_error_with_type(&msg, "ReferenceError");
            crate::value::set_thrown_value(err);
            return Err(js_err);
        }
        if scope.borrow().get_kind(name) == Some(VarKind::Const) {
            if scope.borrow().is_function_name(name)
                && (default_expr.is_some() || !crate::interpreter::is_strict_mode())
            {
                return Ok(());
            }
            return Err(JsError(format!(
                "TypeError: Assignment to constant variable '{}'",
                name
            )));
        }
        if scope.borrow().is_declared_only(name) {
            scope.borrow_mut().initialize_declared(name, value);
        } else {
            let set_result = scope.borrow_mut().set(
                name.to_string(),
                value,
                crate::interpreter::is_strict_mode(),
            );
            if !set_result && crate::interpreter::is_strict_mode() {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    &format!("{} is not defined", name),
                    "ReferenceError",
                );
                return Err(js_err);
            }
        }
        return Ok(());
    }

    if env.borrow().is_tdz(name) {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            &format!(
                "ReferenceError: Cannot access '{}' before initialization",
                name
            ),
            "ReferenceError",
        );
        return Err(js_err);
    }

    if !crate::interpreter::is_strict_mode() && !env.borrow().has(name) {
        if let Some(scope) = env.borrow().var_binding_scope(name) {
            if scope.borrow_mut().set(name.to_string(), value.clone(), false) {
                return Ok(());
            }
        }
    }

    if !env.borrow().has(name) {
        if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
            if global_obj.borrow().has_own(name) {
                global_obj.borrow_mut().set(name, value.clone());
                return Ok(());
            }
        }
        if let Some(result) = env.borrow().set_in_object_env(
            name,
            value.clone(),
            crate::interpreter::is_strict_mode(),
        ) {
            if !result && crate::interpreter::is_strict_mode() {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    &format!("{} is not defined", name),
                    "ReferenceError",
                );
                return Err(js_err);
            }
            return Ok(());
        }
    }

    if env.borrow().has(name) {
        if let Some(kind) = env.borrow().get_kind(name) {
            if kind == VarKind::Const {
                if env.borrow().binding_scope(name).is_some_and(|scope| {
                    scope.borrow().is_function_name(name)
                        && (default_expr.is_some() || !crate::interpreter::is_strict_mode())
                }) {
                    return Ok(());
                }
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    &format!("Assignment to constant variable '{}'", name),
                    "TypeError",
                );
                return Err(js_err);
            }
        }
        if crate::interpreter::is_strict_mode() {
            if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
                if let Some(flags) = global_obj.borrow().get_descriptor(name) {
                    if !flags.writable {
                        let (_, js_err) = crate::value::error::create_js_error_with_type(
                            "Cannot assign to read only property",
                            "TypeError",
                        );
                        return Err(js_err);
                    }
                }
            }
        }
        let undef_fix_val = value.clone();
        let set_result = env.borrow_mut().set(name, value);
        if !set_result && crate::interpreter::is_strict_mode() {
            // Check if this is a TDZ violation.
            if env.borrow().is_tdz(name) {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    &format!(
                        "ReferenceError: Cannot access '{}' before initialization",
                        name
                    ),
                    "ReferenceError",
                );
                return Err(js_err);
            }
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                &format!("{} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        // Also check TDZ when set succeeded (set now returns false for TDZ).
        if env.borrow().is_tdz(name) {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                &format!(
                    "ReferenceError: Cannot access '{}' before initialization",
                    name
                ),
                "ReferenceError",
            );
            return Err(js_err);
        }
        // Workaround: if set returned true but get still returns undefined,
        // the global object likely has a non-writable property (e.g. `eval`).
        let fix_name = name.to_string();
        if !crate::interpreter::is_strict_mode()
            && matches!(env.borrow().get(&fix_name), Some(Value::Undefined))
        {
            for scope_rc in env.borrow().scopes.iter().rev() {
                let mut scope = scope_rc.borrow_mut();
                if scope.is_declared_only(&fix_name) {
                    scope.initialize_declared(&fix_name, undef_fix_val.clone());
                    break;
                }
                if scope.has(&fix_name) {
                    scope.set(fix_name.clone(), undef_fix_val.clone(), false);
                    break;
                }
            }
        }
    } else {
        if crate::interpreter::is_strict_mode() {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                &format!("{} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        let use_global_this = matches!(env.borrow().get("globalThis"), Some(Value::Object(_)));
        if use_global_this {
            if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
                global_obj.borrow_mut().set(name, value);
            }
        } else {
            env.borrow_mut().define(name.to_string(), value);
        }
    }
    Ok(())
}

/// Initialize a declared binding (for-of/for-in lexical head), including TDZ slots.
pub fn init_to_identifier(
    name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
    default_expr: Option<&Expression>,
) -> Result<(), JsError> {
    let value = prepare_identifier_binding_value(name, value, default_expr);
    if env.borrow().is_tdz(name) {
        env.borrow_mut().initialize_declared(name, value);
        return Ok(());
    }
    assign_to_identifier(name, &value, env, default_expr)
}

/// Declare destructuring pattern bindings with the given declaration kind.
pub fn declare_pattern_bindings_with_kind(
    pattern: &BindingElement,
    kind: VarKind,
    env: &Rc<RefCell<Environment>>,
) {
    match pattern {
        BindingElement::Identifier(name) => {
            if name != "__hole" && !env.borrow().current_scope().borrow().has(name) {
                env.borrow_mut().declare_var(name.clone(), kind);
            }
        }
        BindingElement::ArrayPattern(elements) => {
            for element in elements {
                declare_pattern_bindings_with_kind(element, kind, env);
            }
        }
        BindingElement::ObjectPattern(properties) => {
            for (_, binding) in properties {
                declare_pattern_bindings_with_kind(binding, kind, env);
            }
        }
        BindingElement::Default(binding, _) => {
            declare_pattern_bindings_with_kind(binding, kind, env);
        }
        BindingElement::Rest(binding) => {
            declare_pattern_bindings_with_kind(binding, kind, env);
        }
        BindingElement::AssignmentTarget(_) => {}
    }
}

/// Convert a binding pattern to an assignment target expression.
pub fn binding_pattern_expression(pattern: BindingElement) -> Expression {
    match pattern {
        BindingElement::Identifier(name) => Expression::Identifier(name),
        BindingElement::ArrayPattern(elements) => Expression::ArrayPattern(elements),
        BindingElement::ObjectPattern(properties) => Expression::ObjectPattern(properties),
        BindingElement::Default(binding, _) => binding_pattern_expression(*binding),
        BindingElement::Rest(binding) => binding_pattern_expression(*binding),
        BindingElement::AssignmentTarget(expr) => expr,
    }
}

pub fn is_anonymous_function_definition(expr: &Expression) -> bool {
    match expr {
        Expression::FunctionExpression { name: None, .. } | Expression::ArrowFunction { .. } => {
            true
        }
        Expression::Parenthesized(inner) => is_anonymous_function_definition(inner),
        Expression::Sequence(exprs) if exprs.len() == 1 => {
            is_anonymous_function_definition(&exprs[0])
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::test262::host::Test262Host;
    use crate::Context;
    use crate::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(src)
    }

    // ─── box_primitive_for_set: Number ────────────────────────────────────────

    #[test]
    fn box_primitive_number() {
        let r = eval("var n = Object(5); n.valueOf()").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn box_primitive_boolean() {
        let r = eval("var b = Object(true); b.valueOf()").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_rest_only_destructure() {
        let r = eval("var [...[a,b,c]] = [3,4,5]; a+b+c").unwrap();
        assert_eq!(r, Value::Number(12.0));
    }

    // ─── generator destructuring ─────────────────────────────────────────────

    #[test]
    fn async_gen_default_empty_object_pattern() {
        let r = eval(
            "var access=0, obj=Object.defineProperty({}, 'attr', { get: function() { access++; } }); \
             var n=0; class C { async *method({} = obj) { n=1; } } \
             C.prototype.method.call(new C()).next(); n + access",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn destructure_default_array_literal() {
        let r = eval("function f([v] = [99]) { return v; } f()").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn for_of_const_destructure_default_arrow_gets_binding_name() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let name = ctx
            .eval(
                "var iterCount = 0; var fnName = ''; \
                 for (const [arrow = () => {}] of [[]]) { fnName = arrow.name; iterCount++; } \
                 fnName",
            )
            .unwrap();
        assert_eq!(name, Value::String("arrow".into()));
    }

    #[test]
    fn destructure_default_arrow_function_gets_param_name() {
        let r = eval(
            "var name = ''; \
             function* g([arrow = () => {}]) { name = arrow.name; } \
             g([]).next(); name",
        )
        .unwrap();
        assert_eq!(r, Value::String("arrow".into()));
    }

    #[test]
    fn const_empty_object_destructure_null_throws_type_error() {
        let err = eval("try { const {} = null; 'no throw'; } catch (e) { e.name }").unwrap();
        assert_eq!(err, Value::String("TypeError".into()));
    }

    #[test]
    fn array_destructure_without_symbol_iterator_throws_type_error() {
        let err = eval(
            "try { \
               delete Array.prototype[Symbol.iterator]; \
               (function([a, b]) {})([1, 2]); \
               'no throw'; \
             } catch (e) { e.name }",
        )
        .unwrap();
        assert_eq!(err, Value::String("TypeError".into()));
    }

    #[test]
    fn async_gen_object_destructure_getter_throws() {
        let err = eval(
            "try { \
               var poisonedProperty = Object.defineProperty({}, 'poisoned', { \
                 get: function() { throw new Error('getter'); } \
               }); \
               class C { async *method({ poisoned } = poisonedProperty) {} } \
               C.prototype.method(); \
               'no throw'; \
             } catch (e) { e.message }",
        )
        .unwrap();
        assert_eq!(err, Value::String("getter".into()));
    }

    #[test]
    fn async_gen_default_pattern_iter_step_error() {
        let err = eval(
            "try { \
               (function() { \
                 var g = {}; \
                 g[Symbol.iterator] = function() { \
                   return { next: function() { throw new Error('step'); } }; \
                 }; \
                 class C { async *method([x] = g) {} } \
                 C.prototype.method(); \
               })(); \
               'no throw'; \
             } catch (e) { e.message }",
        )
        .unwrap();
        assert_eq!(err, Value::String("step".into()));
    }

    #[test]
    fn async_gen_default_array_pattern_from_iterator() {
        let r = eval(
            "var iter={}; \
             iter[Symbol.iterator]=function(){ return { \
               next:function(){ return {value:42,done:false}; } \
             }; }; \
             function f([v] = iter) { return v; } f()",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn regular_fn_rest_destructure() {
        let r = eval("function f([...[a,b,c]]) { return a+b+c; } f([3,4,5])").unwrap();
        assert_eq!(r, Value::Number(12.0));
    }

    #[test]
    fn standalone_gen_rest_destructure() {
        let r =
            eval("function* f([...[a,b,c]]) { return a+b+c; } f([3,4,5]).next().value").unwrap();
        assert_eq!(r, Value::Number(12.0));
    }

    #[test]
    fn generator_method_destructure_closes_iterator() {
        let r = eval(
            "var doneCallCount = 0; \
             var iter = {}; \
             iter[Symbol.iterator] = function() { \
               return { \
                 next: function() { return { value: null, done: false }; }, \
                 return: function() { doneCallCount += 1; return {}; } \
               }; \
             }; \
             var callCount = 0; \
             class C { *method([x]) { callCount = 1; } } \
             new C().method(iter).next(); \
             doneCallCount + callCount * 10",
        )
        .unwrap();
        assert_eq!(r, Value::Number(11.0));
    }

    #[test]
    fn nested_yield_operand_yields_inner_value() {
        let r = eval(
            "class A { *g() { yield yield 1; } } \
             A.prototype.g().next().value",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn nested_yield_operand_suspends_outer_on_second_next() {
        let r = eval(
            "class A { *g() { yield yield 1; } } \
             var iter = A.prototype.g(); \
             iter.next(); \
             iter.next().done",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn nested_yield_operand_completes_on_third_next() {
        let r = eval(
            "class A { *g() { yield yield 1; } } \
             var iter = A.prototype.g(); \
             iter.next(); \
             iter.next(); \
             iter.next().done",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn generator_method_destructures_rest_param() {
        let r = eval(
            "var c=0,x=0,y=0,z=0; class C { *method([...[a, b, c]]) { \
             x=a; y=b; z=c; c=1; } } new C().method([3, 4, 5]).next(); x+y+z",
        )
        .unwrap();
        assert_eq!(r, Value::Number(12.0));
    }

    #[test]
    fn assign_array_destructuring_generator_elision() {
        use crate::ast::BindingElement;
        use crate::eval::object::helpers::destructuring::assign_array_destructuring;

        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; }",
        )
        .unwrap();
        let gen = ctx.eval("g()").unwrap();
        let env = Rc::clone(ctx.env());
        let bindings = vec![BindingElement::Identifier("__hole".into())];
        assign_array_destructuring(&bindings, &gen, &env).unwrap();
        assert_eq!(ctx.eval("first").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn bind_params_destructures_generator_elision() {
        use crate::ast::{BindingElement, Param};
        use crate::env::Environment;
        use crate::eval::function::bind_params;
        use crate::value::ValueFunction;

        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; }",
        )
        .unwrap();
        let gen = ctx.eval("g()").unwrap();
        let params = vec![Param {
            name: "arg".to_string(),
            default: None,
            pattern: Some(BindingElement::ArrayPattern(vec![
                BindingElement::Identifier("__hole".into()),
            ])),
            rest: false,
        }];
        let env = Rc::clone(ctx.env());
        let f = ValueFunction::new(None, params.clone(), vec![], Rc::clone(&env), false, false);
        let call_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&env))));
        bind_params(&f, &params, std::slice::from_ref(&gen), &call_env).unwrap();
        assert_eq!(ctx.eval("first").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("second").unwrap(), Value::Number(0.0));
    }

    #[test]
    fn rest_pattern_forwards_iterator_step_error() {
        let err = eval(
            "try { \
               (function([...x]) {})(function*() { throw new Error('step'); }()); \
               'no throw'; \
             } catch (e) { e.message }",
        )
        .unwrap();
        assert_eq!(err, Value::String("step".into()));
    }

    #[test]
    fn async_gen_method_rest_forwards_iterator_step_error() {
        let err = eval(
            "try { \
               (function() { \
                 class C { async *method([...x]) {} } \
                 C.prototype.method(function*() { throw new Error('step'); }()); \
               })(); \
               'no throw'; \
             } catch (e) { e.message }",
        )
        .unwrap();
        assert_eq!(err, Value::String("step".into()));
    }

    #[test]
    fn destructure_generator_elision_advances_iterator() {
        let mut host = crate::test262::QuenchHost::new();
        host.run_script(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; } \
             class C { method([,]) {} } \
             new C().method(g()); \
             if (first !== 1 || second !== 0) throw new Error('got ' + first + ',' + second);",
        )
        .expect("class method generator destructuring");
    }

    #[test]
    fn destructure_generator_elision_iife() {
        let r = eval(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; } \
             (function([,]) {})(g()); \
             first + second * 10",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    // ─── array destructuring ─────────────────────────────────────────────────

    #[test]
    fn array_destructuring_basic() {
        let r = eval("var [a, b] = [1, 2]; a + b").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn array_destructuring_spread() {
        let r = eval("var [first, ...rest] = [1, 2, 3]; rest[0] + rest[1]").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn array_destructuring_skip() {
        let r = eval("var [, second] = [10, 20]; second").unwrap();
        assert_eq!(r, Value::Number(20.0));
    }

    #[test]
    fn array_destructuring_default() {
        let r = eval("var [a = 1] = []; a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn array_destructuring_nested() {
        let r = eval("var [[inner]] = [[42]]; inner").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── object destructuring ────────────────────────────────────────────────

    #[test]
    fn object_destructuring_basic() {
        let r = eval("var {x, y} = {x: 1, y: 2}; x + y").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn object_destructuring_rename() {
        let r = eval("var {x: alias} = {x: 99}; alias").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn object_destructuring_default() {
        let r = eval("var {missing = 5} = {}; missing").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn object_destructuring_nested() {
        let r = eval("var {outer: {inner}} = {outer: {inner: 7}}; inner").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn object_destructuring_rest() {
        let r = eval("var {a, ...rest} = {a: 1, b: 2, c: 3}; rest.b + rest.c").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn object_literal_rest_invokes_getter() {
        let r = eval("var o = { get v() { return 2; } }; var {...rest} = o; rest.v").unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    #[test]
    fn iterator_next_accessor_invoked_per_step() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let loads = ctx
            .eval(
                "var loadNextCount = 0, iterationCount = 0; \
                 var iterable = {}, iterator = {}; \
                 iterable[Symbol.iterator] = function() { return iterator; }; \
                 function next() { \
                   if (iterationCount) return { done: true }; \
                   return { value: 45, done: false }; \
                 } \
                 Object.defineProperty(iterator, 'next', { \
                   get: function() { loadNextCount++; return next; }, \
                   configurable: true \
                 }); \
                 for (var x of iterable) { \
                   Object.defineProperty(iterator, 'next', { \
                     get: function() { throw new Error('too early'); } \
                   }); \
                   iterationCount++; \
                 } \
                 JSON.stringify([iterationCount, loadNextCount])",
            )
            .unwrap();
        assert_eq!(loads, Value::String("[1,1]".to_string()));
    }

    #[test]
    fn array_destructure_ref_eval_before_iterator_next() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let counts = ctx
            .eval(
                "var nextCount = 0, returnCount = 0; \
                 var iterable = {}; \
                 var iterator = { \
                   next: function() { nextCount += 1; return { done: true }; }, \
                   return: function() { returnCount += 1; } \
                 }; \
                 iterable[Symbol.iterator] = function() { return iterator; }; \
                 var thrower = function() { throw new Error('Test262'); }; \
                 try { for ([ {}[thrower()] ] of [iterable]) {} } catch (_) {} \
                 JSON.stringify([nextCount, returnCount])",
            )
            .unwrap();
        assert_eq!(counts, Value::String("[0,1]".to_string()));
    }

    #[test]
    fn object_rest_for_of_number_is_instanceof_object() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let ok = ctx
            .eval(
                "var rest, ok = false; \
                 for ({...rest} of [51]) { ok = rest instanceof Object; } \
                 ok",
            )
            .unwrap();
        assert_eq!(ok, Value::Boolean(true));
    }

    #[test]
    fn object_rest_for_of_string_indexes_per_code_unit() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let chars = ctx
            .eval(
                "var rest; \
                 for ({...rest} of ['foo']) {} \
                 JSON.stringify([rest['0'], rest['1'], rest['2']])",
            )
            .unwrap();
        assert_eq!(chars, Value::String("[\"f\",\"o\",\"o\"]".to_string()));
    }

    #[test]
    fn object_rest_for_of_enumerates_in_own_key_order() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let calls = ctx
            .eval(
                "var calls = []; \
                 var o = { get z() { calls.push('z'); }, get a() { calls.push('a'); } }; \
                 Object.defineProperty(o, 1, { get: function() { calls.push(1); }, enumerable: true }); \
                 Object.defineProperty(o, Symbol('foo'), { \
                   get: function() { calls.push('Symbol(foo)'); }, enumerable: true \
                 }); \
                 for ({...rest} of [o]) {} \
                 JSON.stringify(calls)",
            )
            .unwrap();
        assert_eq!(
            calls,
            Value::String("[1,\"z\",\"a\",\"Symbol(foo)\"]".to_string())
        );
    }

    #[test]
    fn object_rest_param_invokes_getter() {
        let r = eval(
            "class C { method({...rest}) { return rest.v; } } \
             var count = 0; \
             var o = { get v() { count++; return 2; } }; \
             new C().method(o) + count",
        )
        .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    // ─── compute_property_key ────────────────────────────────────────────────

    #[test]
    fn destructuring_string_key() {
        let r = eval("var {'foo': x} = {'foo': 42}; x").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── assign_binding_elem: identifier assignment ───────────────────────────

    #[test]
    fn binding_elem_identifier_const() {
        let r = eval("const x = 5; x").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn binding_elem_identifier_let() {
        let r = eval("let y = 10; y").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    // ─── assign_to_identifier: const assignment throws ─────────────────────

    #[test]
    fn assign_to_const_throws() {
        let r = eval("const x = 1; x = 2");
        assert!(r.is_err());
    }

    #[test]
    fn assign_to_undeclared_strict_throws() {
        let r = eval("'use strict'; z = 1");
        assert!(r.is_err());
    }

    #[test]
    fn stale_throw_does_not_block_iterator_return_invocation() {
        use super::call_iterator_return;

        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let iter_val = ctx
            .eval(
                "globalThis.__rc = 0; ({ \
                   next: function(){ return {done:true}; }, \
                   return: function(){ globalThis.__rc += 1; return {}; } \
                 })",
            )
            .unwrap();
        let Value::Object(iter) = iter_val else {
            panic!("expected object iterator");
        };
        crate::value::set_thrown_value(Value::Number(0.0));
        assert!(call_iterator_return(&iter).is_none());
        let count = ctx.eval("globalThis.__rc").unwrap();
        assert_eq!(count, Value::Number(1.0));
    }

    #[test]
    fn iterator_close_non_object_return_throws_type_error() {
        let err = eval(
            "var iterable = {};
             iterable[Symbol.iterator] = function() {
               return {
                 next: function() { return { done: true }; },
                 return: function() { return null; }
               };
             };
             for ([] of [iterable]) {}",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("TypeError"),
            "expected TypeError, got {err}"
        );
    }

    #[test]
    fn iterator_next_non_object_result_throws_type_error() {
        let err = eval(
            "var iterable = {};
             iterable[Symbol.iterator] = function() {
               return { next: function() { return true; } };
             };
             for (var x of iterable) {}",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("TypeError"),
            "expected TypeError, got {err}"
        );
    }

    // ─── string is iterable for destructuring ────────────────────────────────

    #[test]
    fn string_is_iterable_for_destructuring() {
        let r = eval("var [a, b, c] = 'xyz'; a + b + c").unwrap();
        assert_eq!(r, Value::String("xyz".into()));
    }

    // ─── assign_array_with_iterator: excess bindings ────────────────────────

    #[test]
    fn array_destructuring_fewer_values() {
        let r = eval("var [a, b, c] = [1]; b").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn array_destructuring_more_values() {
        let r = eval("var [a] = [1, 2, 3]; a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn destructure_param_iterator_value_getter_throw() {
        let err = eval(
            "var poisonedValue = Object.defineProperty({}, 'value', { \
               get: function() { throw new Error('ITER_VAL_ERR'); } \
             }); \
             var g = {}; \
             g[Symbol.iterator] = function() { \
               return { next: function() { return poisonedValue; } }; \
             }; \
             function f([x]) {} \
             try { f(g); 'ok'; } catch (e) { e.message; }",
        )
        .unwrap();
        assert_eq!(err, Value::String("ITER_VAL_ERR".into()));
    }

    #[test]
    fn array_prototype_symbol_iterator_generator_is_valid_iterator() {
        let r = eval(
            "Array.prototype[Symbol.iterator] = function* () { yield 1; yield 2; }; \
             var pair = (function(){ var [a, b] = [99]; return [a, b]; })(); \
             pair[0] + ',' + pair[1]",
        )
        .unwrap();
        assert_eq!(r, Value::String("1,2".into()));
    }

    #[test]
    fn sync_generator_destructure_param_binds_at_call() {
        let err = eval(
            "var poisonedValue = Object.defineProperty({}, 'value', { \
               get: function() { throw new Error('GEN_PARAM_ERR'); } \
             }); \
             var g = {}; \
             g[Symbol.iterator] = function() { \
               return { next: function() { return poisonedValue; } }; \
             }; \
             function* f([x]) {} \
             try { f(g); 'ok'; } catch (e) { e.message; }",
        )
        .unwrap();
        assert_eq!(err, Value::String("GEN_PARAM_ERR".into()));
    }

    #[test]
    fn object_destructure_param_string_argument_uses_to_object() {
        let r = eval(
            "var fnParam; (function({test262 = fnParam = arguments}) { \
             fnParam = arguments; })('function'); fnParam[0]",
        )
        .unwrap();
        assert_eq!(r, Value::String("function".into()));
    }

    #[test]
    fn destructuring_this_private_field_before_getter_throws_reference_error() {
        let r = eval(
            "class C extends class {} { #field; constructor() { var init = () => super(); \
             var object = { get a() { init(); } }; ({a: this.#field} = object); } } new C()",
        );
        assert!(r.is_err());
        let msg = format!("{:?}", r.unwrap_err());
        assert!(
            msg.contains("ReferenceError"),
            "expected ReferenceError before getter runs, got {msg}"
        );
    }

    #[test]
    fn destructure_yield_iterator_close_throw_propagates() {
        // Tests IteratorClose error propagation: when destructuring encounters
        // a return completion (from generator.return()) AND the iterator's
        // return() throws, the throw should propagate per IteratorClose step 6.
        let err = eval(
            "var returnCount = 0; \
             var iterable = {}; \
             var iterator = { \
               return: function() { returnCount += 1; throw new Error('CLOSE_ERR'); } \
             }; \
             iterable[Symbol.iterator] = function() { return iterator; }; \
             function* g() { var result; result = [ {}[yield] ] = iterable; } \
             var iter = g(); iter.next(); \
             try { iter.return(); 'no throw'; } catch (e) { e.message; }",
        )
        .unwrap();
        assert_eq!(err, Value::String("CLOSE_ERR".into()));
    }

    #[test]
    fn destructure_return_closes_iterator_before_returning_generator_value() {
        let result = eval(
            "var nextCount = 0; var returnCount = 0; \
             var iterator = { next: function() { nextCount += 1; return {done: false, value: undefined}; }, \
             return: function() { returnCount += 1; return {}; } }; \
             var iterable = {}; iterable[Symbol.iterator] = function() { return iterator; }; \
             function* g() { var result; result = [ {} = yield ] = iterable; } \
             var iter = g(); iter.next(); var result = iter.return(777); \
             [nextCount, returnCount, result.value, result.done]",
        )
        .unwrap();
        let Value::Object(array) = result else {
            panic!("expected array")
        };
        assert_eq!(array.borrow().get("0"), Some(Value::Number(1.0)));
        assert_eq!(array.borrow().get("1"), Some(Value::Number(1.0)));
        assert_eq!(array.borrow().get("2"), Some(Value::Number(777.0)));
        assert_eq!(array.borrow().get("3"), Some(Value::Boolean(true)));
    }

    #[test]
    fn object_destructure_resumes_computed_target_key() {
        let result = eval(
            "var x = {}; var iter = (function*() { var result; \
             result = { x: x[yield] } = { x: 23 }; })(); \
             var first = iter.next(); var second = iter.next('prop'); \
             [first.value, first.done, x.prop, second.value, second.done]",
        )
        .unwrap();
        let Value::Object(array) = result else {
            panic!("expected array")
        };
        assert_eq!(array.borrow().get("2"), Some(Value::Number(23.0)));
    }

    #[test]
    fn destructure_yield_iterator_return_non_object_throws_typeerror() {
        // Tests IteratorClose throws TypeError when `return` returns non-Object.
        let err = eval(
            "var nextCount = 0; \
             var iterable = {}; \
             var x; \
             var iterator = { \
               next: function() { nextCount += 1; return { done: nextCount > 10 }; }, \
               return: function() { return null; } \
             }; \
             iterable[Symbol.iterator] = function() { return iterator; }; \
             function* g() { var result; result = [ x , ...{}[yield] ] = iterable; } \
             var iter = g(); iter.next(); \
             try { iter.return(); 'no throw'; } catch (e) { e.name; }",
        )
        .unwrap();
        assert_eq!(err, Value::String("TypeError".into()));
    }

    #[test]
    fn destructure_yield_default_suspends_generator() {
        // Tests that yield in a default value suspends the generator,
        // and then iter.return() properly closes the iterator.
        let v = eval(
            "var nextCount = 0; var returnCount = 0; \
             var iterator = { \
               next: function() { nextCount += 1; return {done: false, value: undefined}; }, \
               return: function() { returnCount += 1; return {}; } \
             }; \
             var iterable = {}; \
             iterable[Symbol.iterator] = function() { return iterator; }; \
             function* g() { var result; result = [ {} = yield ] = iterable; } \
             var iter = g(); \
             var n = iter.next(); \
             var rc1 = iter.return(777); \
             JSON.stringify([nextCount, returnCount, rc1.value, rc1.done])",
        )
        .unwrap();
        assert_eq!(v, Value::String("[1,1,777,true]".into()));
    }

    // ─── TDZ + default value destructuring ─────────────────────────────────────
    // Reproducer: let [x = 23] = [,]; — the initializer 23 must be evaluated
    // BEFORE initializing the TDZ slot, so that x = 23 (not TDZ error).

    #[test]
    fn object_rest_excludes_symbol_keys_from_object_keys() {
        // Object.keys(rest) must NOT include symbol keys
        let r = eval(
            "var rest; var o = {}; \
             Object.defineProperty(o, 'z', { get: function() { return 1; }, enumerable: true }); \
             Object.defineProperty(o, 'a', { get: function() { return 2; }, enumerable: true }); \
             Object.defineProperty(o, 1, { get: function() { return 3; }, enumerable: true }); \
             Object.defineProperty(o, Symbol('foo'), { get: function() { return 4; }, enumerable: true }); \
             for ({...rest} of [o]) {} \
             Object.keys(rest).length",
        )
        .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn let_destruct_array_default_with_hole() {
        let r = eval("let [x = 23] = [,]; x").unwrap();
        assert_eq!(r, Value::Number(23.0));
    }

    #[test]
    fn let_destruct_array_default_with_undefined() {
        let r = eval("let [x = 99] = [undefined]; x").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn let_destruct_array_default_with_iterator_hole() {
        let r = eval("let [x = 42, y = 7] = [,]; x + ',' + y").unwrap();
        assert_eq!(r, Value::String("42,7".into()));
    }

    #[test]
    fn const_destruct_array_default_with_hole() {
        let r = eval("const [x = 11] = [,]; x").unwrap();
        assert_eq!(r, Value::Number(11.0));
    }

    #[test]
    fn object_destructure_param_computed_key_evaluated_at_call() {
        let err = eval(
            "function thrower() { throw new Error('COMPUTED_KEY_ERR'); } \
             function f({ [thrower()]: x }) {} \
             try { f({}); 'ok'; } catch (e) { e.message; }",
        )
        .unwrap();
        assert_eq!(err, Value::String("COMPUTED_KEY_ERR".into()));
    }

    #[test]
    fn array_destructure_member_target_evaluates_object_before_key_once() {
        let result = eval(
            "var log = []; \
             function source() { log.push('source'); return [1]; } \
             function target() { log.push('target'); return { set q(v) { log.push('set'); } }; } \
             function key() { log.push('key'); return { toString() { log.push('string'); return 'q'; } }; } \
             ([target()[key()]] = source()); log.join(',');",
        )
        .unwrap();
        assert_eq!(result, Value::String("source,target,key,string,set".into()));
    }

    #[test]
    fn destructuring_inferred_class_name_is_configurable() {
        let result = eval(
            "var xCls, cls, xCls2; \
             var vals = []; \
             [xCls = class x {}, cls = class {}, xCls2 = class { static name() {} }] = vals; \
             [cls.name, Object.getOwnPropertyDescriptor(cls, 'name').configurable, delete cls.name]",
        )
        .unwrap();
        let Value::Object(array) = result else {
            panic!("expected array")
        };
        assert_eq!(array.borrow().get("0"), Some(Value::String("cls".into())));
        assert_eq!(array.borrow().get("1"), Some(Value::Boolean(true)));
        assert_eq!(array.borrow().get("2"), Some(Value::Boolean(true)));
    }
}
