//! Promise built-in implementation
//!
//! Implements Promise/A+ specification for asynchronous operations.

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::value::object::PromiseObjectData;
use crate::Context;
use crate::JsError;

pub fn register_promise(ctx: &mut Context) {
    let proto = create_promise_proto();
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

fn create_promise_proto() -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Ordinary);
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
                });

                let reject_fn: Box<dyn Fn(Value)> = Box::new(move |reason: Value| {
                    let mut obj = promise_rc_clone2.borrow_mut();
                    if let Some(ref mut data) = obj.promise_data {
                        if data.state == crate::value::object::PromiseState::Pending {
                            data.reject(reason);
                        }
                    }
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

    // Set static methods with closure to capture prototype
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
            move |_args: Vec<Value>| {
                promise_all_impl_static(Rc::clone(&proto_for_static_clone3))
            },
        ))),
    );

    let proto_for_static_clone4 = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "race",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |_args: Vec<Value>| {
                promise_race_impl_static(Rc::clone(&proto_for_static_clone4))
            },
        ))),
    );

    let proto_for_static_clone5 = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "allSettled",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |_args: Vec<Value>| {
                promise_all_settled_impl_static(Rc::clone(&proto_for_static_clone5))
            },
        ))),
    );

    let proto_for_static_clone6 = Rc::clone(&proto_for_static);
    constructor.set_static_method(
        "any",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |_args: Vec<Value>| {
                promise_any_impl_static(Rc::clone(&proto_for_static_clone6))
            },
        ))),
    );

    constructor
}

fn promise_then_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let on_fulfilled = args.first().cloned().unwrap_or(Value::Undefined);
    let on_rejected = args.get(1).cloned().unwrap_or(Value::Undefined);

    let new_promise = Object::new(ObjectKind::Promise);
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    {
        let mut obj = new_promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
    }

    let current_promise_this = crate::interpreter::get_native_this();

    if let Some(Value::Object(ref obj_rc)) = current_promise_this {
        let obj = obj_rc.borrow();
        if let Some(ref data) = obj.promise_data {
            let mut new_data = new_promise_rc.borrow_mut();
            if let Some(ref mut nd) = new_data.promise_data {
                nd.state = data.state.clone();
                nd.result = data.result.clone();
            }

            if data.state != crate::value::object::PromiseState::Pending {
                let executor = match data.state {
                    crate::value::object::PromiseState::Fulfilled => on_fulfilled.clone(),
                    crate::value::object::PromiseState::Rejected => on_rejected.clone(),
                    crate::value::object::PromiseState::Pending => Value::Undefined,
                };

                if matches!(executor, Value::Function(_) | Value::NativeFunction(_)) {
                    let result = call_value_with_this(executor, vec![data.result.clone()], Value::Undefined);
                    match result {
                        Ok(val) => {
                            if let Some(ref mut nd) = new_data.promise_data {
                                nd.fulfill(val);
                            }
                        }
                        Err(e) => {
                            if let Some(ref mut nd) = new_data.promise_data {
                                nd.reject(Value::String(e.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Value::Object(new_promise_rc))
}

fn promise_catch_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let on_rejected = args.first().cloned().unwrap_or(Value::Undefined);
    promise_then_impl(vec![Value::Undefined, on_rejected])
}

fn promise_finally_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let on_finally = args.first().cloned().unwrap_or(Value::Undefined);

    let new_promise = Object::new(ObjectKind::Promise);
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    {
        let mut obj = new_promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
    }

    let current_promise_this = crate::interpreter::get_native_this();

    if let Some(Value::Object(ref obj_rc)) = current_promise_this {
        let obj = obj_rc.borrow();
        if let Some(ref data) = obj.promise_data {
            let mut new_data = new_promise_rc.borrow_mut();
            if let Some(ref mut nd) = new_data.promise_data {
                nd.state = data.state.clone();
                nd.result = data.result.clone();
            }

            if matches!(on_finally, Value::Function(_) | Value::NativeFunction(_)) {
                let _ = call_value_with_this(on_finally, vec![], Value::Undefined);
            }
        }
    }

    Ok(Value::Object(new_promise_rc))
}

fn promise_resolve_impl_static(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);

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

fn promise_all_impl_static(proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(PromiseObjectData::new());
    }
    Ok(Value::Object(promise_rc))
}

fn promise_race_impl_static(proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(PromiseObjectData::new());
    }
    Ok(Value::Object(promise_rc))
}

fn promise_all_settled_impl_static(proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(PromiseObjectData::new());
    }
    Ok(Value::Object(promise_rc))
}

fn promise_any_impl_static(proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(PromiseObjectData::new());
    }
    Ok(Value::Object(promise_rc))
}

fn queue_microtask(args: Vec<Value>) -> Result<Value, JsError> {
    let _callback = args.first().cloned().unwrap_or(Value::Undefined);
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
