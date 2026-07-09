//! Promise constructor implementation

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::value::object::PromiseObjectData;
use crate::JsError;

use super::callbacks::{process_callbacks_sync, queue_callback_on_promise};
use super::helpers::{get_promise_proto, set_promise_proto};
use super::static_methods::{
    promise_all_impl, promise_race_impl, promise_reject_impl_static,
    promise_resolve_impl_static,
};

/// Create the Promise constructor
pub fn create_promise_constructor(
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

/// Register Promise constructor and prototype
pub fn register_promise(ctx: &mut crate::Context) {
    let proto = create_promise_proto();
    set_promise_proto(Rc::clone(&proto));

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
    use crate::value::NativeFunction;

    let proto = super::helpers::create_promise_proto();

    proto.borrow_mut().set(
        "then",
        Value::NativeFunction(Rc::new(NativeFunction::new(promise_then_impl))),
    );
    proto.borrow_mut().set(
        "catch",
        Value::NativeFunction(Rc::new(NativeFunction::new(promise_catch_impl))),
    );
    proto.borrow_mut().set(
        "finally",
        Value::NativeFunction(Rc::new(NativeFunction::new(promise_finally_impl))),
    );

    proto
}

pub fn promise_then_impl(args: Vec<Value>) -> Result<Value, JsError> {
    use std::rc::Rc;

    use crate::value::object::PromiseObjectData;

    let on_fulfilled = args.first().cloned().unwrap_or(Value::Undefined);
    let on_rejected = args.get(1).cloned().unwrap_or(Value::Undefined);

    let current_promise_this = crate::interpreter::get_native_this();

    let promise_proto = get_promise_proto();
    let new_promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&promise_proto));
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    {
        let mut obj = new_promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
    }

    if let Some(Value::Object(ref obj_rc)) = current_promise_this {
        let state = {
            let obj = obj_rc.borrow();
            obj.promise_data.as_ref().map(|d| d.state.clone())
        };

        match state {
            Some(crate::value::object::PromiseState::Fulfilled) => {
                let callback = create_callback_promise(
                    on_fulfilled, on_rejected, Rc::clone(&new_promise_rc)
                );
                queue_callback_on_promise(obj_rc, callback);
                process_callbacks_sync(obj_rc);
            }
            Some(crate::value::object::PromiseState::Rejected) => {
                let callback = create_callback_promise(
                    on_fulfilled, on_rejected, Rc::clone(&new_promise_rc)
                );
                queue_callback_on_promise(obj_rc, callback);
                process_callbacks_sync(obj_rc);
            }
            Some(crate::value::object::PromiseState::Pending) => {
                let callback = create_callback_promise(
                    on_fulfilled, on_rejected, Rc::clone(&new_promise_rc)
                );
                queue_callback_on_promise(obj_rc, callback);
            }
            None => {}
        }
    }

    Ok(Value::Object(new_promise_rc))
}

fn create_callback_promise(
    on_fulfilled: Value,
    on_rejected: Value,
    target_promise: Rc<RefCell<Object>>,
) -> Value {
    super::helpers::create_callback_promise(on_fulfilled, on_rejected, target_promise)
}

pub fn promise_catch_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let on_rejected = args.first().cloned().unwrap_or(Value::Undefined);
    promise_then_impl(vec![Value::Undefined, on_rejected])
}

pub fn promise_finally_impl(args: Vec<Value>) -> Result<Value, JsError> {
    use std::rc::Rc;

    use crate::eval::call_value_with_this;
    use crate::value::object::PromiseObjectData;

    let on_finally = args.first().cloned().unwrap_or(Value::Undefined);
    let current_promise_this = crate::interpreter::get_native_this();

    let (current_state, current_result) = if let Some(Value::Object(ref obj_rc)) = current_promise_this {
        let obj = obj_rc.borrow();
        if let Some(ref data) = obj.promise_data {
            (Some(data.state.clone()), Some(data.result.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let promise_proto = get_promise_proto();
    let new_promise = Object::with_prototype(ObjectKind::Promise, Rc::clone(&promise_proto));
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    let new_promise_result = Rc::clone(&new_promise_rc);
    {
        let mut obj = new_promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
    }

    if matches!(on_finally, Value::Function(_) | Value::NativeFunction(_)) {
        let result = call_value_with_this(on_finally.clone(), vec![], Value::Undefined);
        match result {
            Err(e) => {
                let mut new_data = new_promise_rc.borrow_mut();
                if let Some(ref mut nd) = new_data.promise_data {
                    nd.reject(Value::String(e.to_string()));
                }
                return Ok(Value::Object(new_promise_result));
            }
            Ok(_) => {
                if let (Some(state), Some(result)) = (current_state, current_result) {
                    let mut new_data = new_promise_rc.borrow_mut();
                    if let Some(ref mut nd) = new_data.promise_data {
                        nd.state = state;
                        nd.result = result;
                    }
                }
            }
        }
    } else {
        if let (Some(state), Some(result)) = (current_state, current_result) {
            let mut new_data = new_promise_rc.borrow_mut();
            if let Some(ref mut nd) = new_data.promise_data {
                nd.state = state;
                nd.result = result;
            }
        }
    }

    Ok(Value::Object(new_promise_result))
}

fn queue_microtask(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    Ok(super::microtask::queue_microtask_impl(callback))
}
