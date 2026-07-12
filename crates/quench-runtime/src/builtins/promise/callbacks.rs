//! Promise callback processing

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::object::PromiseState;
use crate::value::{NativeFunction, Object, Value};

use super::helpers::create_callback_promise;

/// Settle a promise as fulfilled, unwrapping thenable values (promises),
/// and enqueue its reactions as microtasks.
pub fn settle_resolve(promise_rc: &Rc<RefCell<Object>>, value: Value) {
    // Thenable unwrapping: if the value is itself a promise, adopt its state
    // instead of fulfilling with the promise object.
    if let Value::Object(ref thenable) = value {
        let is_promise = thenable.borrow().promise_data.is_some();
        if is_promise {
            if Rc::ptr_eq(thenable, promise_rc) {
                settle_reject(
                    promise_rc,
                    Value::String("TypeError: Chaining cycle detected for promise".to_string()),
                );
                return;
            }
            let reaction =
                create_callback_promise(Value::Undefined, Value::Undefined, Rc::clone(promise_rc));
            queue_callback_on_promise(thenable, reaction);
            enqueue_promise_reactions(thenable);
            return;
        }
    }

    {
        let mut obj = promise_rc.borrow_mut();
        match obj.promise_data {
            Some(ref mut data) if data.state == PromiseState::Pending => data.fulfill(value),
            _ => return,
        }
    }
    enqueue_promise_reactions(promise_rc);
}

/// Settle a promise as rejected and enqueue its reactions as microtasks.
pub fn settle_reject(promise_rc: &Rc<RefCell<Object>>, reason: Value) {
    {
        let mut obj = promise_rc.borrow_mut();
        match obj.promise_data {
            Some(ref mut data) if data.state == PromiseState::Pending => data.reject(reason),
            _ => return,
        }
    }
    enqueue_promise_reactions(promise_rc);
}

/// Enqueue the reactions of a settled promise onto the microtask queue.
/// Both callback queues are drained, so reactions stored while the promise
/// was pending are never leaked regardless of the final state.
pub fn enqueue_promise_reactions(promise_rc: &Rc<RefCell<Object>>) {
    let (state, result, callbacks) = {
        let mut obj = promise_rc.borrow_mut();
        if let Some(ref mut data) = obj.promise_data {
            if data.state == PromiseState::Pending {
                return;
            }
            let mut callbacks: Vec<Value> = data.on_fulfilled_callbacks.drain(..).collect();
            callbacks.append(&mut data.on_rejected_callbacks);
            (data.state.clone(), data.result.clone(), callbacks)
        } else {
            return;
        }
    };

    for callback_value in callbacks {
        if let Some((on_fulfilled, on_rejected, target_rc)) = extract_callback_info(&callback_value)
        {
            enqueue_reaction_job(&state, on_fulfilled, on_rejected, result.clone(), target_rc);
        }
    }
}

fn extract_callback_info(callback_value: &Value) -> Option<(Value, Value, Rc<RefCell<Object>>)> {
    if let Value::Object(ref cb_obj) = callback_value {
        let on_fulfilled = cb_obj
            .borrow()
            .properties
            .get("_onFulfilled")
            .cloned()
            .unwrap_or(Value::Undefined);
        let on_rejected = cb_obj
            .borrow()
            .properties
            .get("_onRejected")
            .cloned()
            .unwrap_or(Value::Undefined);
        if let Some(Value::Object(ref target_rc)) =
            cb_obj.borrow().properties.get("_targetPromise").cloned()
        {
            return Some((on_fulfilled, on_rejected, Rc::clone(target_rc)));
        }
    }
    None
}

/// Enqueue a single reaction as a microtask job. The job runs at the next
/// microtask checkpoint, per ES job-queue semantics.
fn enqueue_reaction_job(
    state: &PromiseState,
    on_fulfilled: Value,
    on_rejected: Value,
    result: Value,
    target_rc: Rc<RefCell<Object>>,
) {
    let (callback, is_on_rejected) = match state {
        PromiseState::Fulfilled => (on_fulfilled, false),
        PromiseState::Rejected => (on_rejected, true),
        PromiseState::Pending => return,
    };
    let job = Value::NativeFunction(Rc::new(NativeFunction::new(move |_args: Vec<Value>| {
        execute_callback(&callback, result.clone(), &target_rc, is_on_rejected);
        Ok(Value::Undefined)
    })));
    super::microtask::queue_microtask_impl(job);
}

/// Execute a reaction callback and fulfill/reject the target promise.
/// Rejections preserve the thrown value; fulfillments unwrap promises.
pub fn execute_callback(
    callback: &Value,
    arg: Value,
    target_promise: &Rc<RefCell<Object>>,
    is_on_rejected: bool,
) {
    if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        match call_value_with_this(callback.clone(), vec![arg], Value::Undefined) {
            Ok(val) => settle_resolve(target_promise, val),
            Err(e) => {
                let reason = crate::value::get_thrown_value()
                    .unwrap_or_else(|| Value::String(e.to_string()));
                settle_reject(target_promise, reason);
            }
        }
    } else if is_on_rejected {
        settle_reject(target_promise, arg);
    } else {
        settle_resolve(target_promise, arg);
    }
}

/// Queue a reaction on a promise. The reaction object carries both handlers,
/// so it is stored exactly once even while the promise is pending; on settle
/// both queues are drained (see `enqueue_promise_reactions`).
pub fn queue_callback_on_promise(promise_rc: &Rc<RefCell<Object>>, callback: Value) {
    let mut obj = promise_rc.borrow_mut();
    if let Some(ref mut data) = obj.promise_data {
        match data.state {
            PromiseState::Fulfilled | PromiseState::Pending => {
                data.add_fulfilled_callback(callback);
            }
            PromiseState::Rejected => {
                data.add_rejected_callback(callback);
            }
        }
    }
}
