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
/// microtask checkpoint.
///
/// The closure captures `target_rc` and `result`. When `execute_callback` later settles
/// the target promise, it must do so without re-borrowing `target_rc`
/// (which would cause a "RefCell already borrowed" panic since the closure
/// already holds a borrow through the captured Rc). Therefore,
/// `execute_callback` borrows `target_rc` ONCE, sets the state, drains the
/// callbacks, drops the borrow, then enqueues the callbacks directly as
/// microtasks — bypassing `enqueue_promise_reactions` which would re-borrow.
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
    // Capture the result value directly so execute_callback can pass it to the callback
    let captured_result = result.clone();
    let job = Value::NativeFunction(Rc::new(NativeFunction::new(move |_args: Vec<Value>| {
        execute_callback(
            &callback,
            captured_result.clone(),
            &target_rc,
            is_on_rejected,
        );
        Ok(Value::Undefined)
    })));
    super::microtask::queue_microtask_impl(job);
}

/// Execute a reaction callback and settle the target promise.
/// Settles the promise in ONE borrow, then directly enqueues the drained
/// callbacks as microtasks — this avoids the double-borrow panic that would
/// occur if we called `enqueue_promise_reactions` (which re-borrows target_rc).
pub fn execute_callback(
    callback: &Value,
    arg: Value,
    target_promise: &Rc<RefCell<Object>>,
    is_on_rejected: bool,
) {
    // Evaluate the callback and get the settlement value+state
    let (settled_value, is_rejected) =
        if matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
            match call_value_with_this(callback.clone(), vec![arg.clone()], Value::Undefined) {
                Ok(val) => (val, false),
                Err(e) => {
                    let reason = crate::value::get_thrown_value()
                        .unwrap_or_else(|| Value::String(e.to_string()));
                    (reason, true)
                }
            }
        } else if is_on_rejected {
            (arg.clone(), true)
        } else {
            (arg.clone(), false)
        };

    let settled_state = if is_rejected {
        PromiseState::Rejected
    } else {
        PromiseState::Fulfilled
    };

    // Check if the callback returned a thenable (promise) that needs unwrapping.
    // Per Promise.then spec: "If result is a Promise, adopt its state"
    let is_thenable = if let Value::Object(ref obj) = settled_value {
        obj.borrow().promise_data.is_some()
    } else {
        false
    };

    // Borrow target_promise ONCE: set state, drain callbacks.
    // The borrow is dropped at the end of this block.
    let drained_callbacks = {
        let mut obj = target_promise.borrow_mut();
        match obj.promise_data {
            Some(ref mut data) if data.state == PromiseState::Pending => {
                if is_thenable {
                    // Callback returned a thenable - chain to it instead of settling directly.
                    // We need to attach callbacks that will settle target_promise when
                    // the thenable resolves/rejects.
                    if let Value::Object(ref thenable) = settled_value {
                        // Clone the target_promise Rc for each callback to avoid borrow conflicts
                        let target_for_fulfill = Rc::clone(target_promise);
                        let target_for_reject = Rc::clone(target_promise);
                        let target_for_chain = Rc::clone(target_promise);

                        // Callback for when the thenable fulfills
                        let fulfill_cb = Value::NativeFunction(Rc::new(NativeFunction::new(
                            move |args: Vec<Value>| {
                                let val = args.first().cloned().unwrap_or(Value::Undefined);
                                settle_resolve(&target_for_fulfill, val);
                                Ok(Value::Undefined)
                            },
                        )));

                        // Callback for when the thenable rejects
                        let reject_cb = Value::NativeFunction(Rc::new(NativeFunction::new(
                            move |args: Vec<Value>| {
                                let reason = args.first().cloned().unwrap_or(Value::Undefined);
                                settle_reject(&target_for_reject, reason);
                                Ok(Value::Undefined)
                            },
                        )));

                        // Chain to the thenable
                        let cb_promise =
                            create_callback_promise(fulfill_cb, reject_cb, target_for_chain);
                        queue_callback_on_promise(thenable, cb_promise);
                        enqueue_promise_reactions(thenable);
                    }
                    // Return empty callbacks since we're chaining, not settling directly
                    Vec::new()
                } else if is_rejected {
                    data.reject(settled_value.clone());
                    // Drain callbacks while we still have the borrow
                    let mut cb: Vec<Value> = data.on_fulfilled_callbacks.drain(..).collect();
                    cb.append(&mut data.on_rejected_callbacks);
                    cb
                } else {
                    data.fulfill(settled_value.clone());
                    // Drain callbacks while we still have the borrow
                    let mut cb: Vec<Value> = data.on_fulfilled_callbacks.drain(..).collect();
                    cb.append(&mut data.on_rejected_callbacks);
                    cb
                }
            }
            _ => return,
        }
    };
    // borrow dropped here — target_promise is no longer borrowed
    for callback_value in drained_callbacks {
        if let Some((on_fulfilled, on_rejected, chained_rc)) =
            extract_callback_info(&callback_value)
        {
            enqueue_reaction_job(
                &settled_state,
                on_fulfilled,
                on_rejected,
                settled_value.clone(),
                chained_rc,
            );
        }
    }
}

/// Queue a reaction on a promise. The reaction object carries both handlers.
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
