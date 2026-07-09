//! Promise callback processing

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{Object, Value};
use crate::JsError;

/// Process callbacks stored on a promise (synchronous version)
pub fn process_callbacks_sync(promise_rc: &Rc<RefCell<Object>>) {
    loop {
        let (state, result, callbacks) = {
            let mut obj = promise_rc.borrow_mut();
            if let Some(ref mut data) = obj.promise_data {
                let fulfilled_callbacks: Vec<_> = data.on_fulfilled_callbacks.drain(..).collect();
                let rejected_callbacks: Vec<_> = data.on_rejected_callbacks.drain(..).collect();
                let all_callbacks: Vec<_> = fulfilled_callbacks
                    .into_iter()
                    .chain(rejected_callbacks.into_iter())
                    .collect();
                (data.state.clone(), data.result.clone(), all_callbacks)
            } else {
                return;
            }
        };

        let mut has_callbacks = false;
        for callback_value in callbacks {
            has_callbacks = true;
            if let Value::Object(ref cb_obj) = callback_value {
                let on_fulfilled = cb_obj.borrow().properties.get("_onFulfilled")
                    .cloned()
                    .unwrap_or(Value::Undefined);
                let on_rejected = cb_obj.borrow().properties.get("_onRejected")
                    .cloned()
                    .unwrap_or(Value::Undefined);
                let target_opt = cb_obj.borrow().properties.get("_targetPromise")
                    .cloned();

                if let Some(Value::Object(ref target_rc)) = target_opt {
                    match state {
                        crate::value::object::PromiseState::Fulfilled => {
                            execute_callback(&on_fulfilled, result.clone(), target_rc, false);
                        }
                        crate::value::object::PromiseState::Rejected => {
                            execute_callback(&on_rejected, result.clone(), target_rc, true);
                        }
                        crate::value::object::PromiseState::Pending => {}
                    }
                }
            }
        }

        if !has_callbacks {
            break;
        }

        let has_new = {
            let obj = promise_rc.borrow();
            if let Some(ref data) = obj.promise_data {
                !data.on_fulfilled_callbacks.is_empty() || !data.on_rejected_callbacks.is_empty()
            } else {
                false
            }
        };

        if !has_new {
            break;
        }
    }
}

/// Execute a callback and fulfill/reject the target promise
pub fn execute_callback(
    callback: &Value,
    arg: Value,
    target_promise: &Rc<RefCell<Object>>,
    is_on_rejected: bool,
) {
    let result = if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        call_value_with_this(callback.clone(), vec![arg], Value::Undefined)
    } else if is_on_rejected {
        Err(JsError(format!("{}", arg)))
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

    process_callbacks_sync(target_promise);
}

/// Queue a callback on a promise
pub fn queue_callback_on_promise(promise_rc: &Rc<RefCell<Object>>, callback: Value) {
    let mut obj = promise_rc.borrow_mut();
    if let Some(ref mut data) = obj.promise_data {
        data.add_fulfilled_callback(callback);
    }
}
