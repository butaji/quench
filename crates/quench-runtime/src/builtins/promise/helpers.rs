//! Promise helper functions

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::object::PromiseObjectData;
use crate::value::{Object, ObjectKind, Value};
use crate::JsError;

// Thread-local storage for Promise prototype
thread_local! {
    static PROMISE_PROTO: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the stored Promise prototype
pub fn get_promise_proto() -> Rc<RefCell<Object>> {
    PROMISE_PROTO.with(|p| {
        p.borrow()
            .clone()
            .expect("Promise prototype not initialized")
    })
}

/// Store the Promise prototype in thread-local storage
pub fn set_promise_proto(proto: Rc<RefCell<Object>>) {
    PROMISE_PROTO.with(|p| *p.borrow_mut() = Some(Rc::clone(&proto)));
}

/// Clear the stored Promise prototype (called on context reset, so a reset
/// context never hands out promises with a previous context's prototype)
pub fn clear_promise_proto() {
    PROMISE_PROTO.with(|p| *p.borrow_mut() = None);
}

/// Create a new Promise prototype object
pub fn create_promise_proto() -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Promise);
    Rc::new(RefCell::new(proto))
}

/// Create resolved promise with given value
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

/// Create rejected promise with given reason
pub fn create_rejected_promise(reason: Value) -> Result<Rc<RefCell<Object>>, JsError> {
    let promise_obj = Object::new(ObjectKind::Promise);
    let promise_rc = Rc::new(RefCell::new(promise_obj));
    {
        let mut obj = promise_rc.borrow_mut();
        obj.promise_data = Some(PromiseObjectData::new());
        if let Some(ref mut data) = obj.promise_data {
            data.reject(reason);
        }
    }
    Ok(promise_rc)
}

/// Create callback promise object for linking promises
pub fn create_callback_promise(
    on_fulfilled: Value,
    on_rejected: Value,
    target_promise: Rc<RefCell<Object>>,
) -> Value {
    let obj = Object::new(ObjectKind::Promise);
    let obj_rc = Rc::new(RefCell::new(obj));
    obj_rc.borrow_mut().set("_onFulfilled", on_fulfilled);
    obj_rc.borrow_mut().set("_onRejected", on_rejected);
    obj_rc
        .borrow_mut()
        .set("_targetPromise", Value::Object(target_promise));
    Value::Object(obj_rc)
}
