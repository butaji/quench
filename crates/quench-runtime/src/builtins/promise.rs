//! Promise built-in implementation

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct PromiseData {
    pub state: PromiseState,
    pub result: Value,
}

impl PromiseData {
    fn new() -> Self {
        PromiseData { state: PromiseState::Pending, result: Value::Undefined }
    }
}

pub fn register_promise(ctx: &mut Context) {
    let proto = create_promise_proto();
    let proto_clone = Rc::clone(&proto);
    let constructor = create_promise_constructor(proto_clone);

    if let Some(op) = crate::value::get_object_prototype() {
        proto.borrow_mut().set("__proto__", Value::Object(op));
    }

    proto.borrow_mut().set("constructor", Value::NativeConstructor(Rc::new(constructor.clone())));
    ctx.set_global("Promise".to_string(), Value::NativeConstructor(Rc::new(constructor)));
}

fn create_promise_proto() -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Ordinary);
    let proto_rc = Rc::new(RefCell::new(proto));

    proto_rc.borrow_mut().set("then", Value::NativeFunction(Rc::new(NativeFunction::new(promise_then_impl))));
    proto_rc.borrow_mut().set("catch", Value::NativeFunction(Rc::new(NativeFunction::new(promise_catch_impl))));
    proto_rc.borrow_mut().set("finally", Value::NativeFunction(Rc::new(NativeFunction::new(promise_finally_impl))));

    proto_rc
}

fn create_new_promise_from_this() -> (Rc<RefCell<Object>>, PromiseData) {
    let new_promise = Object::new(ObjectKind::Promise);
    let new_promise_rc = Rc::new(RefCell::new(new_promise));
    let this_val = crate::interpreter::get_native_this().unwrap_or(Value::Undefined);
    let data = if let Value::Object(ref obj) = this_val {
        obj.borrow_mut().promise_data.take().unwrap_or_else(PromiseData::new)
    } else {
        PromiseData::new()
    };
    (new_promise_rc, data)
}

fn promise_then_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    let _on_fulfilled = args.get(0).cloned().unwrap_or(Value::Undefined);
    let _on_rejected = args.get(1).cloned().unwrap_or(Value::Undefined);

    let (new_promise_rc, promise_data) = create_new_promise_from_this();

    let mut new_data = PromiseData::new();
    new_data.state = promise_data.state.clone();
    new_data.result = promise_data.result.clone();
    new_promise_rc.borrow_mut().promise_data = Some(new_data);

    Ok(Value::Object(new_promise_rc))
}

fn promise_catch_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    let _on_rejected = args.first().cloned().unwrap_or(Value::Undefined);
    let (new_promise_rc, promise_data) = create_new_promise_from_this();

    let mut new_data = PromiseData::new();
    new_data.state = promise_data.state.clone();
    new_data.result = promise_data.result.clone();
    new_promise_rc.borrow_mut().promise_data = Some(new_data);

    Ok(Value::Object(new_promise_rc))
}

fn promise_finally_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    let _on_finally = args.first().cloned().unwrap_or(Value::Undefined);
    let (new_promise_rc, promise_data) = create_new_promise_from_this();

    let mut new_data = PromiseData::new();
    new_data.state = promise_data.state.clone();
    new_data.result = promise_data.result.clone();
    new_promise_rc.borrow_mut().promise_data = Some(new_data);

    Ok(Value::Object(new_promise_rc))
}

fn create_promise_constructor(proto: Rc<RefCell<Object>>) -> NativeConstructor {
    let resolve_proto = Rc::clone(&proto);
    let reject_proto = Rc::clone(&proto);
    let proto_clone = Rc::clone(&proto);

    let mut static_methods = std::collections::HashMap::new();

    static_methods.insert("resolve".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(promise_resolve_impl))));
    static_methods.insert("reject".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(promise_reject_impl))));

    let mut constructor = NativeConstructor::new(
        move |_args| {
            let promise_obj = Object::new(ObjectKind::Promise);
            let promise_rc = Rc::new(RefCell::new(promise_obj));
            {
                let mut obj = promise_rc.borrow_mut();
                obj.prototype = Some(Rc::clone(&proto_clone));
                obj.promise_data = Some(PromiseData::new());
            }
            Ok(Value::Object(promise_rc))
        },
        proto,
    );

    for (name, value) in static_methods {
        constructor.set_static_method(&name, value);
    }

    constructor
}

fn promise_resolve_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
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
        obj.prototype = Some(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
        let mut data = PromiseData::new();
        data.state = PromiseState::Fulfilled;
        data.result = value;
        obj.promise_data = Some(data);
    }
    Ok(Value::Object(promise_rc))
}

fn promise_reject_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
        let mut data = PromiseData::new();
        data.state = PromiseState::Rejected;
        data.result = reason;
        obj.promise_data = Some(data);
    }
    Ok(Value::Object(promise_rc))
}
