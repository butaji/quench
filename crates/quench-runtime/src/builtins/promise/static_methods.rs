//! Promise static methods implementation

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{NativeFunction, Object, ObjectKind, Value};
use crate::value::object::PromiseObjectData;
use crate::JsError;

/// Implements Promise.resolve
pub fn promise_resolve_impl_static(
    args: Vec<Value>,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
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

/// Implements Promise.reject
pub fn promise_reject_impl_static(
    args: Vec<Value>,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
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

/// Implements Promise.all
pub fn promise_all_impl(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
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
        process_promise_all_value(
            i, value, total, &promise_rc, &results, &fulfilled_count, &rejected_flag
        );
    }

    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(promise_data);
    }

    Ok(Value::Object(promise_rc))
}

fn process_promise_all_value(
    idx: usize,
    value: Value,
    total: usize,
    promise_rc: &Rc<RefCell<Object>>,
    results: &Rc<RefCell<Vec<Value>>>,
    fulfilled_count: &Rc<RefCell<usize>>,
    rejected_flag: &Rc<RefCell<bool>>,
) {
    let promise_rc_f = Rc::clone(promise_rc);
    let results_f = Rc::clone(results);
    let count_f = Rc::clone(fulfilled_count);
    let rejected_f = Rc::clone(rejected_flag);
    let total_f = total;
    let idx_f = idx;

    let promise_rc_r = Rc::clone(promise_rc);
    let rejected_r = Rc::clone(rejected_flag);

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
                        d.fulfill(Value::Object(Rc::new(RefCell::new(
                            Object::new_array_from(results_f.borrow().clone())
                        ))));
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

    attach_callbacks_to_value(value, idx, total, promise_rc, results, fulfilled_count, rejected_flag, resolve_fn, reject_fn);
}

fn attach_callbacks_to_value(
    value: Value,
    idx: usize,
    total: usize,
    promise_rc: &Rc<RefCell<Object>>,
    results: &Rc<RefCell<Vec<Value>>>,
    fulfilled_count: &Rc<RefCell<usize>>,
    rejected_flag: &Rc<RefCell<bool>>,
    resolve_fn: Value,
    reject_fn: Value,
) {
    if let Value::Object(ref p) = value {
        let obj = p.borrow();
        if let Some(ref data) = obj.promise_data {
            match data.state {
                crate::value::object::PromiseState::Fulfilled => {
                    let result = data.result.clone();
                    {
                        let mut r = results.borrow_mut();
                        r[idx] = result;
                    }
                    {
                        let mut c = fulfilled_count.borrow_mut();
                        *c += 1;
                        if *c == total && !*rejected_flag.borrow() {
                            let mut p = promise_rc.borrow_mut();
                            if let Some(ref mut d) = p.promise_data {
                                d.fulfill(Value::Object(Rc::new(RefCell::new(
                                    Object::new_array_from(results.borrow().clone())
                                ))));
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
                _ => attach_then_handlers(p, resolve_fn, reject_fn),
            }
        }
    } else {
        let mut r = results.borrow_mut();
        r[idx] = value;
        {
            let mut c = fulfilled_count.borrow_mut();
            *c += 1;
            if *c == total && !*rejected_flag.borrow() {
                let mut p = promise_rc.borrow_mut();
                if let Some(ref mut d) = p.promise_data {
                    d.fulfill(Value::Object(Rc::new(RefCell::new(
                        Object::new_array_from(results.borrow().clone())
                    ))));
                }
            }
        }
    }
}

fn attach_then_handlers(p: &Rc<RefCell<Object>>, resolve_fn: Value, reject_fn: Value) {
    if let Some(ref then_method) = p.borrow().get("then") {
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

/// Implements Promise.race
pub fn promise_race_impl(args: Vec<Value>, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
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
        process_promise_race_value(value, &promise_rc, &settled);

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

fn process_promise_race_value(value: Value, promise_rc: &Rc<RefCell<Object>>, settled: &Rc<RefCell<bool>>) {
    let promise_rc_f = Rc::clone(promise_rc);
    let settled_f = Rc::clone(settled);
    let promise_rc_r = Rc::clone(promise_rc);
    let settled_r = Rc::clone(settled);

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
                        attach_race_handlers(then_method, resolve_fn, reject_fn);
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
}

fn attach_race_handlers(then_method: &Value, resolve_fn: Value, reject_fn: Value) {
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
