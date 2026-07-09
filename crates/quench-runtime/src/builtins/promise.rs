//! Promise built-in implementation
//!
//! Implements Promise/A+ specification for asynchronous operations.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::value::object::PromiseObjectData;
use crate::Context;
use crate::JsError;

// Thread-local storage for Promise prototype
thread_local! {
    static PROMISE_PROTO: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

pub fn register_promise(ctx: &mut Context) {
    let proto = create_promise_proto();

    // Store the prototype in thread-local
    PROMISE_PROTO.with(|p| *p.borrow_mut() = Some(Rc::clone(&proto)));

    let proto_clone = Rc::clone(&proto);
    let constructor = create_promise_constructor(proto_clone, Rc::clone(&proto));

    proto.borrow_mut().set(
        "constructor",
        Value::NativeConstructor(Rc::new(constructor.clone())),
    );
    ctx.set_global("Promise".to_string(), Value::NativeConstructor(Rc::new(constructor)));
    ctx.set_global(
        "queueMicrotask".to_string(),
        Value::NativeFunction(Rc::new(NativeFunction::new(queue_microtask))),
    );
}

fn get_promise_proto() -> Rc<RefCell<Object>> {
    PROMISE_PROTO.with(|p| {
        p.borrow().clone().expect("Promise prototype not initialized")
    })
}

fn create_promise_proto() -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Promise);
    let proto_rc = Rc::new(RefCell::new(proto));

    proto_rc.borrow_mut().set(
        "then",
        Value::NativeFunction(Rc::new(NativeFunction::new(promise_then_impl))),
    );
    proto_rc.borrow_mut().set(
        "catch",
        Value::NativeFunction(Rc::new(NativeFunction::new(promise_catch_impl))),
    );
    proto_rc.borrow_mut().set(
        "finally",
        Value::NativeFunction(Rc::new(NativeFunction::new(promise_finally_impl))),
    );

    proto_rc
}

fn create_promise_constructor(
    proto: Rc<RefCell<Object>>,
    proto_for_static: Rc<RefCell<Object>>,
) -> NativeConstructor {
    let proto_clone = Rc::clone(&proto);

    let mut constructor = NativeConstructor::new(
        move |args| {
            let executor = args.first().cloned().unwrap_or(Value::Undefined);

            let promise_obj = Object::new(ObjectKind::Promise);
            let promise_rc = Rc::new(RefCell::new(promise_obj));
            {
                let mut obj = promise_rc.borrow_mut();
                obj.prototype = Some(Rc::clone(&proto_clone));
                obj.promise_data = Some(PromiseObjectData::new());
            }

            if matches!(executor, Value::Function(_) | Value::NativeFunction(_)) {
                let promise_rc_clone = Rc::clone(&promise_rc);
                let promise_rc_clone2 = Rc::clone(&promise_rc);

                let resolve_fn: Box<dyn Fn(Value)> = Box::new(move |value: Value| {
                    let mut obj = promise_rc_clone.borrow_mut();
                    if let Some(ref mut data) = obj.promise_data {
                        if data.state == crate::value::object::PromiseState::Pending {
                            data.fulfill(value);
                        }
                    }
                    drop(obj);
                    // Process pending callbacks
                    let promise_clone = Rc::clone(&promise_rc_clone);
                    process_callbacks_sync(&promise_clone);
                });

                let reject_fn: Box<dyn Fn(Value)> = Box::new(move |reason: Value| {
                    let mut obj = promise_rc_clone2.borrow_mut();
                    if let Some(ref mut data) = obj.promise_data {
                        if data.state == crate::value::object::PromiseState::Pending {
                            data.reject(reason);
                        }
                    }
                    drop(obj);
                    let promise_clone = Rc::clone(&promise_rc_clone2);
                    process_callbacks_sync(&promise_clone);
                });

                let resolve_rc = Rc::new(RefCell::new(resolve_fn));
                let reject_rc = Rc::new(RefCell::new(reject_fn));

                let resolve_rc_clone = Rc::clone(&resolve_rc);
                let reject_rc_clone = Rc::clone(&reject_rc);

                let resolve_val = Value::NativeFunction(Rc::new(NativeFunction::new(
                    move |args: Vec<Value>| {
                        let val = args.first().cloned().unwrap_or(Value::Undefined);
                        resolve_rc_clone.borrow()(val);
                        Ok(Value::Undefined)
                    },
                )));

                let reject_val = Value::NativeFunction(Rc::new(NativeFunction::new(
                    move |args: Vec<Value>| {
                        let reason = args.first().cloned().unwrap_or(Value::Undefined);
                        reject_rc_clone.borrow()(reason);
                        Ok(Value::Undefined)
                    },
                )));

                let _ = call_value_with_this(executor, vec![resolve_val, reject_val], Value::Undefined);
            }

            Ok(Value::Object(promise_rc))
        },
        proto,
    );

    // Set static methods
    let proto_for_static_clone = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "resolve",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                promise_resolve_impl_static(args, Rc::clone(&proto_for_static_clone))
            },
        ))),
    );

    let proto_for_static_clone2 = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "reject",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                promise_reject_impl_static(args, Rc::clone(&proto_for_static_clone2))
            },
        ))),
    );

    let proto_for_static_clone3 = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "all",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                promise_all_impl(args, Rc::clone(&proto_for_static_clone3))
            },
        ))),
    );

    let proto_for_static_clone4 = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "race",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                promise_race_impl(args, Rc::clone(&proto_for_static_clone4))
            },
        ))),
    );

    constructor
}

/// Process callbacks stored on a promise (synchronous version)
fn process_callbacks_sync(promise_rc: &Rc<RefCell<Object>>) {
    // Get state and callbacks while holding the lock
    let (state, result, callbacks) = {
        let mut obj = promise_rc.borrow_mut();
        if let Some(ref mut data) = obj.promise_data {
            let callbacks: Vec<_> = data.on_fulfilled_callbacks.drain(..).collect();
            (data.state.clone(), data.result.clone(), callbacks)
        } else {
            return;
        }
    };

    // Process each callback outside the lock
    for callback_value in callbacks {
        if let Value::Object(ref cb_obj) = callback_value {
            let on_fulfilled = cb_obj.borrow().properties.get("_onFulfilled")
                .cloned()
                .unwrap_or(Value::Undefined);
            let target_opt = cb_obj.borrow().properties.get("_targetPromise")
                .cloned();

            if let Some(Value::Object(ref target_rc)) = target_opt {
                if state == crate::value::object::PromiseState::Fulfilled {
                    execute_callback(&on_fulfilled, result.clone(), target_rc);
                }
            }
        }
    }
}

/// Execute a callback and fulfill/reject the target promise
fn execute_callback(callback: &Value, arg: Value, target_promise: &Rc<RefCell<Object>>) {
    let result = if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        call_value_with_this(callback.clone(), vec![arg], Value::Undefined)
    } else {
        Ok(arg)
    };

    let mut target = target_promise.borrow_mut();
    if let Some(ref mut data) = target.promise_data {
        match result {
            Ok(val) => data.fulfill(val),
            Err(e) => data.reject(Value::String(e.to_string())),
        }
    }
    drop(target);

    // Process any callbacks on the target promise
    process_callbacks_sync(target_promise);
}

fn promise_then_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let on_fulfilled = args.first().cloned().unwrap_or(Value::Undefined);
    let on_rejected = args.get(1).cloned().unwrap_or(Value::Undefined);

    let current_promise_this = crate::interpreter::get_native_this();

    // Create new promise with Promise prototype
    let promise_proto = get_promise_proto();
    let new_promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&promise_proto));
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    {
        let mut obj = new_promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
    }

    if let Some(Value::Object(ref obj_rc)) = current_promise_this {
        let (state, _result) = {
            let obj = obj_rc.borrow();
            (
                obj.promise_data.as_ref().map(|d| d.state.clone()),
                obj.promise_data.as_ref().map(|d| d.result.clone()),
            )
        };

        match state {
            Some(crate::value::object::PromiseState::Fulfilled) => {
                // Already fulfilled - execute callback immediately
                let callback = create_callback_promise(on_fulfilled, on_rejected, Rc::clone(&new_promise_rc));
                queue_callback_on_promise(obj_rc, callback);
                // Process the callback now
                process_callbacks_sync(obj_rc);
            }
            Some(crate::value::object::PromiseState::Pending) => {
                // Still pending - store callback
                let callback = create_callback_promise(on_fulfilled, on_rejected, Rc::clone(&new_promise_rc));
                queue_callback_on_promise(obj_rc, callback);
            }
            _ => {}
        }
    }

    Ok(Value::Object(new_promise_rc))
}

fn create_callback_promise(on_fulfilled: Value, on_rejected: Value, target_promise: Rc<RefCell<Object>>) -> Value {
    let obj = Object::new(ObjectKind::Promise);
    let obj_rc = Rc::new(RefCell::new(obj));
    obj_rc.borrow_mut().set("_onFulfilled", on_fulfilled);
    obj_rc.borrow_mut().set("_onRejected", on_rejected);
    obj_rc.borrow_mut().set("_targetPromise", Value::Object(target_promise));
    Value::Object(obj_rc)
}

fn queue_callback_on_promise(promise_rc: &Rc<RefCell<Object>>, callback: Value) {
    let mut obj = promise_rc.borrow_mut();
    if let Some(ref mut data) = obj.promise_data {
        data.add_fulfilled_callback(callback);
    }
}

fn promise_catch_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let on_rejected = args.first().cloned().unwrap_or(Value::Undefined);
    promise_then_impl(vec![Value::Undefined, on_rejected])
}

fn promise_finally_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let _on_finally = args.first().cloned().unwrap_or(Value::Undefined);

    // Create new promise
    let promise_proto = get_promise_proto();
    let new_promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&promise_proto));
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    {
        let mut obj = new_promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
    }

    // For now, just pass through
    let current_promise_this = crate::interpreter::get_native_this();
    if let Some(Value::Object(ref obj_rc)) = current_promise_this {
        let obj = obj_rc.borrow();
        if let Some(ref data) = obj.promise_data {
            let mut new_data = new_promise_rc.borrow_mut();
            if let Some(ref mut nd) = new_data.promise_data {
                nd.state = data.state.clone();
                nd.result = data.result.clone();
            }
        }
    }

    Ok(Value::Object(new_promise_rc))
}

fn promise_resolve_impl_static(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);

    // If already a promise, return it
    if let Value::Object(ref obj) = value {
        let obj_ref = obj.borrow();
        if obj_ref.kind == ObjectKind::Promise {
            return Ok(value.clone());
        }
    }

    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(PromiseObjectData::new());
        if let Some(ref mut data) = obj.promise_data {
            data.fulfill(value);
        }
    }
    Ok(Value::Object(promise_rc))
}

fn promise_reject_impl_static(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let reason = args.first().cloned().unwrap_or(Value::Undefined);

    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(PromiseObjectData::new());
        if let Some(ref mut data) = obj.promise_data {
            data.reject(reason);
        }
    }
    Ok(Value::Object(promise_rc))
}

fn promise_all_impl(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let input = args.first().cloned().unwrap_or(Value::Undefined);

    let values: Vec<Value> = if let Value::Object(ref obj) = input {
        obj.borrow().elements.clone()
    } else {
        vec![]
    };

    let total = values.len();
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    let mut promise_data = PromiseObjectData::new();
    promise_data.state = crate::value::object::PromiseState::Pending;

    if total == 0 {
        promise_data.fulfill(Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![])))));
        {
            let mut obj = promise_rc.borrow_mut();
            obj.prototype = Some(proto);
            obj.promise_data = Some(promise_data);
        }
        return Ok(Value::Object(promise_rc));
    }

    let results = Rc::new(RefCell::new(vec![Value::Undefined; total]));
    let fulfilled_count = Rc::new(RefCell::new(0usize));
    let rejected_flag = Rc::new(RefCell::new(false));

    for (i, value) in values.into_iter().enumerate() {
        // Create separate clones for resolve and reject closures
        let promise_rc_f = Rc::clone(&promise_rc);
        let results_f = Rc::clone(&results);
        let count_f = Rc::clone(&fulfilled_count);
        let rejected_f = Rc::clone(&rejected_flag);
        let total_f = total;
        let idx_f = i;

        let promise_rc_r = Rc::clone(&promise_rc);
        let rejected_r = Rc::clone(&rejected_flag);

        let resolve_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                {
                    let mut r = results_f.borrow_mut();
                    r[idx_f] = val;
                }
                {
                    let mut c = count_f.borrow_mut();
                    *c += 1;
                    if *c == total_f && !*rejected_f.borrow() {
                        let mut p = promise_rc_f.borrow_mut();
                        if let Some(ref mut d) = p.promise_data {
                            d.fulfill(Value::Object(Rc::new(RefCell::new(Object::new_array_from(results_f.borrow().clone())))));
                        }
                    }
                }
                Ok(Value::Undefined)
            },
        )));

        let reject_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                let reason = args.first().cloned().unwrap_or(Value::Undefined);
                {
                    let mut r = rejected_r.borrow_mut();
                    *r = true;
                }
                let mut p = promise_rc_r.borrow_mut();
                if let Some(ref mut d) = p.promise_data {
                    d.reject(reason);
                }
                Ok(Value::Undefined)
            },
        )));

        // Chain to the value
        if let Value::Object(ref p) = value {
            let obj = p.borrow();
            if let Some(ref data) = obj.promise_data {
                match data.state {
                    crate::value::object::PromiseState::Fulfilled => {
                        let result = data.result.clone();
                        {
                            let mut r = results.borrow_mut();
                            r[i] = result;
                        }
                        {
                            let mut c = fulfilled_count.borrow_mut();
                            *c += 1;
                            if *c == total && !*rejected_flag.borrow() {
                                let mut p = promise_rc.borrow_mut();
                                if let Some(ref mut d) = p.promise_data {
                                    d.fulfill(Value::Object(Rc::new(RefCell::new(Object::new_array_from(results.borrow().clone())))));
                                }
                            }
                        }
                    }
                    crate::value::object::PromiseState::Rejected => {
                        let mut r = rejected_flag.borrow_mut();
                        *r = true;
                        let mut p = promise_rc.borrow_mut();
                        if let Some(ref mut d) = p.promise_data {
                            d.reject(data.result.clone());
                        }
                    }
                    _ => {
                        // Attach then handlers
                        if let Some(ref then_method) = obj.get("then") {
                            let pf = Rc::new(RefCell::new(resolve_fn.clone()));
                            let pr = Rc::new(RefCell::new(reject_fn.clone()));

                            let on_fulfilled = Value::NativeFunction(Rc::new(NativeFunction::new(
                                move |args: Vec<Value>| {
                                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                                    let cb = pf.borrow().clone();
                                    if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                                        let _ = call_value_with_this(cb, vec![val], Value::Undefined);
                                    }
                                    Ok(Value::Undefined)
                                },
                            )));
                            let on_rejected = Value::NativeFunction(Rc::new(NativeFunction::new(
                                move |args: Vec<Value>| {
                                    let reason = args.first().cloned().unwrap_or(Value::Undefined);
                                    let cb = pr.borrow().clone();
                                    if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                                        let _ = call_value_with_this(cb, vec![reason], Value::Undefined);
                                    }
                                    Ok(Value::Undefined)
                                },
                            )));
                            let _ = call_value_with_this(then_method.clone(), vec![on_fulfilled, on_rejected], Value::Undefined);
                        }
                    }
                }
            }
        } else {
            // Non-object values resolve immediately
            {
                let mut r = results.borrow_mut();
                r[i] = value;
            }
            {
                let mut c = fulfilled_count.borrow_mut();
                *c += 1;
                if *c == total && !*rejected_flag.borrow() {
                    let mut p = promise_rc.borrow_mut();
                    if let Some(ref mut d) = p.promise_data {
                        d.fulfill(Value::Object(Rc::new(RefCell::new(Object::new_array_from(results.borrow().clone())))));
                    }
                }
            }
        }
    }

    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(promise_data);
    }

    Ok(Value::Object(promise_rc))
}

fn promise_race_impl(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let input = args.first().cloned().unwrap_or(Value::Undefined);

    let values: Vec<Value> = if let Value::Object(ref obj) = input {
        obj.borrow().elements.clone()
    } else {
        vec![]
    };

    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    let promise_data = PromiseObjectData::new();
    let settled = Rc::new(RefCell::new(false));

    for value in values {
        // Create separate clones for resolve and reject closures
        let promise_rc_f = Rc::clone(&promise_rc);
        let settled_f = Rc::clone(&settled);
        let promise_rc_r = Rc::clone(&promise_rc);
        let settled_r = Rc::clone(&settled);

        let resolve_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                let mut s = settled_f.borrow_mut();
                if !*s {
                    *s = true;
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut p = promise_rc_f.borrow_mut();
                    if let Some(ref mut d) = p.promise_data {
                        d.fulfill(val);
                    }
                }
                Ok(Value::Undefined)
            },
        )));

        let reject_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                let mut s = settled_r.borrow_mut();
                if !*s {
                    *s = true;
                    let reason = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut p = promise_rc_r.borrow_mut();
                    if let Some(ref mut d) = p.promise_data {
                        d.reject(reason);
                    }
                }
                Ok(Value::Undefined)
            },
        )));

        if let Value::Object(ref p) = value {
            let obj = p.borrow();
            if let Some(ref data) = obj.promise_data {
                match data.state {
                    crate::value::object::PromiseState::Fulfilled => {
                        let mut s = settled.borrow_mut();
                        if !*s {
                            *s = true;
                            let mut pr = promise_rc.borrow_mut();
                            if let Some(ref mut d) = pr.promise_data {
                                d.fulfill(data.result.clone());
                            }
                        }
                    }
                    crate::value::object::PromiseState::Rejected => {
                        let mut s = settled.borrow_mut();
                        if !*s {
                            *s = true;
                            let mut pr = promise_rc.borrow_mut();
                            if let Some(ref mut d) = pr.promise_data {
                                d.reject(data.result.clone());
                            }
                        }
                    }
                    _ => {
                        if let Some(ref then_method) = obj.get("then") {
                            let pf = Rc::new(RefCell::new(resolve_fn.clone()));
                            let pr = Rc::new(RefCell::new(reject_fn.clone()));

                            let on_fulfilled = Value::NativeFunction(Rc::new(NativeFunction::new(
                                move |args: Vec<Value>| {
                                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                                    let cb = pf.borrow().clone();
                                    if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                                        let _ = call_value_with_this(cb, vec![val], Value::Undefined);
                                    }
                                    Ok(Value::Undefined)
                                },
                            )));
                            let on_rejected = Value::NativeFunction(Rc::new(NativeFunction::new(
                                move |args: Vec<Value>| {
                                    let reason = args.first().cloned().unwrap_or(Value::Undefined);
                                    let cb = pr.borrow().clone();
                                    if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                                        let _ = call_value_with_this(cb, vec![reason], Value::Undefined);
                                    }
                                    Ok(Value::Undefined)
                                },
                            )));
                            let _ = call_value_with_this(then_method.clone(), vec![on_fulfilled, on_rejected], Value::Undefined);
                        }
                    }
                }
            }
        } else {
            let mut s = settled.borrow_mut();
            if !*s {
                *s = true;
                let mut pr = promise_rc.borrow_mut();
                if let Some(ref mut d) = pr.promise_data {
                    d.fulfill(value);
                }
            }
        }

        // Check if already settled
        if *settled.borrow() {
            break;
        }
    }

    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(promise_data);
    }

    Ok(Value::Object(promise_rc))
}

// Thread-local microtask queue
thread_local! {
    static MICROTASK_QUEUE: RefCell<VecDeque<Value>> = const { RefCell::new(VecDeque::new()) };
}

pub fn get_pending_microtasks() -> Vec<Value> {
    MICROTASK_QUEUE.with(|queue| {
        queue.borrow_mut().drain(..).collect()
    })
}

pub fn execute_pending_microtasks() -> Result<(), JsError> {
    loop {
        let tasks = get_pending_microtasks();
        if tasks.is_empty() {
            break;
        }
        for task in tasks {
            if matches!(task, Value::Function(_) | Value::NativeFunction(_)) {
                call_value_with_this(task, vec![], Value::Undefined)?;
            }
        }
    }
    Ok(())
}

fn queue_microtask(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        MICROTASK_QUEUE.with(|q| q.borrow_mut().push_back(callback));
    }

    Ok(Value::Undefined)
}

pub fn create_resolved_promise(value: Value) -> Rc<RefCell<Object>> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
        if let Some(ref mut data) = obj.promise_data {
            data.fulfill(value);
        }
    }
    promise_rc
}

pub fn create_rejected_promise(reason: Value) -> Rc<RefCell<Object>> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
        if let Some(ref mut data) = obj.promise_data {
            data.reject(reason);
        }
    }
    promise_rc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::object::PromiseState;

    #[test]
    fn test_promise_state_pending() {
        let data = PromiseObjectData::new();
        assert_eq!(data.state, PromiseState::Pending);
    }

    #[test]
    fn test_promise_fulfill() {
        let mut data = PromiseObjectData::new();
        data.fulfill(Value::Number(42.0));
        assert_eq!(data.state, PromiseState::Fulfilled);
        assert_eq!(data.result, Value::Number(42.0));
    }

    #[test]
    fn test_promise_reject() {
        let mut data = PromiseObjectData::new();
        data.reject(Value::String("error".to_string()));
        assert_eq!(data.state, PromiseState::Rejected);
        assert_eq!(data.result, Value::String("error".to_string()));
    }

    #[test]
    fn test_create_resolved_promise() {
        let promise = create_resolved_promise(Value::Number(42.0));
        let obj = promise.borrow();
        if let Some(ref data) = obj.promise_data {
            assert_eq!(data.state, PromiseState::Fulfilled);
            assert_eq!(data.result, Value::Number(42.0));
        }
    }

    #[test]
    fn test_create_rejected_promise() {
        let promise = create_rejected_promise(Value::String("error".to_string()));
        let obj = promise.borrow();
        if let Some(ref data) = obj.promise_data {
            assert_eq!(data.state, PromiseState::Rejected);
            assert_eq!(data.result, Value::String("error".to_string()));
        }
    }
}
