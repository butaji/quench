//! Promise static methods implementation

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::eval::iteration::get_iterator;
use crate::value::object::PromiseObjectData;
use crate::value::error::create_js_error_with_type;
use crate::value::{NativeFunction, Object, ObjectKind, Value};
use crate::JsError;

// Re-export enqueue_promise_reactions for use in static methods
pub(crate) use super::enqueue_promise_reactions;

/// Validate that `constructor` works as a Promise constructor by calling it
/// with resolve/reject callbacks. Returns the created promise or an error.
/// Implements NewPromiseCapability per spec.
fn invoke_promise_constructor(
    constructor: &Value,
    proto: &Rc<RefCell<Object>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Mutable slots for resolve/reject functions (None = not yet called)
    let resolve_slot: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let reject_slot: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));

    let resolve_slot_f = Rc::clone(&resolve_slot);
    let reject_slot_f = Rc::clone(&reject_slot);

    // Per spec: GetCapabilitiesExecutor is passed to the constructor, which then
    // calls it. The executor will call these with the resolve/reject values.
    // Per 25.4.1.5.1: throw TypeError if [[Resolve]]/[[Reject]] is not undefined.
    let resolve_slot_f = Rc::clone(&resolve_slot);
    let resolve_already_set: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let reject_slot_f = Rc::clone(&reject_slot);
    let reject_already_set: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // Create executor function that receives (resolve, reject) callbacks from the constructor
    // and forwards calls to our resolve/reject slots
    let resolve_already_set_f = Rc::clone(&resolve_already_set);
    let reject_already_set_f = Rc::clone(&reject_already_set);

    let resolve_fn = Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
        // This is called by the executor: executor(resolve_value, reject_value)
        // where resolve_value is what to pass to our resolve, and reject_value is what to pass to our reject
        let resolve_arg = args.first().cloned().unwrap_or(Value::Undefined);
        let reject_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

        // Call our resolve slot with resolve_arg
        if !matches!(&resolve_arg, Value::Undefined) {
            if *resolve_already_set_f.borrow() {
                return Err(JsError("TypeError: GetCapabilitiesExecutor resolve already called".to_string()));
            }
            *resolve_already_set_f.borrow_mut() = true;
        }
        *resolve_slot_f.borrow_mut() = Some(resolve_arg);

        // Call our reject slot with reject_arg
        if !matches!(&reject_arg, Value::Undefined) {
            if *reject_already_set_f.borrow() {
                return Err(JsError("TypeError: GetCapabilitiesExecutor reject already called".to_string()));
            }
            *reject_already_set_f.borrow_mut() = true;
        }
        *reject_slot_f.borrow_mut() = Some(reject_arg);

        Ok(Value::Undefined)
    })));

    let reject_fn = Value::NativeFunction(Rc::new(NativeFunction::new(move |_args: Vec<Value>| {
        // This function is never actually called - the executor above handles both resolve and reject
        Ok(Value::Undefined)
    })));

    // Call constructor as a new instance
    let result = call_value_with_this(
        constructor.clone(),
        vec![resolve_fn, reject_fn],
        Value::Undefined,
    );

    // If the constructor threw (e.g., because resolve was called twice with non-undefined),
    // propagate that error and ensure thrown value is set.
    if let Err(e) = &result {
        // Extract error type from message (e.g., "TypeError: ..." -> "TypeError")
        let err_msg = e.0.as_str();
        let err_type = if err_msg.starts_with("TypeError:") {
            "TypeError"
        } else {
            "Error"
        };
        let msg = err_msg
            .strip_prefix("TypeError: ")
            .or_else(|| err_msg.strip_prefix("Error: "))
            .unwrap_or(err_msg);
        let (err_val, _) = create_js_error_with_type(msg, err_type);
        crate::value::set_thrown_value(err_val);
        return Err(e.clone());
    }

    let result = result.unwrap();

    // Per spec steps 8-9: Check if resolve/reject are callable
    // If executor never called them, they are undefined (not callable)
    let resolve_val = resolve_slot.borrow();
    let reject_val = reject_slot.borrow();
    
    let resolve_callable = matches!(&*resolve_val, Some(v) 
        if matches!(v, Value::Object(_) | Value::Function(_) | Value::NativeFunction(_)));
    let reject_callable = matches!(&*reject_val, Some(v) 
        if matches!(v, Value::Object(_) | Value::Function(_) | Value::NativeFunction(_)));
    
    if !resolve_callable {
        let (err_val, _) = create_js_error_with_type("GetCapabilitiesExecutor resolve is not callable", "TypeError");
        crate::value::set_thrown_value(err_val);
        return Err(JsError("TypeError: GetCapabilitiesExecutor resolve is not callable".to_string()));
    }
    
    if !reject_callable {
        let (err_val, _) = create_js_error_with_type("GetCapabilitiesExecutor reject is not callable", "TypeError");
        crate::value::set_thrown_value(err_val);
        return Err(JsError("TypeError: GetCapabilitiesExecutor reject is not callable".to_string()));
    }

    // Result must be an object (Promise). If it's not (e.g., implicit undefined return),
    // create a new promise object.
    match result {
        Value::Object(o) => Ok(o),
        _ => {
            // Create a new promise object
            let promise_obj = Object::new(ObjectKind::Promise);
            let promise_rc = Rc::new(RefCell::new(promise_obj));
            {
                let mut obj = promise_rc.borrow_mut();
                obj.prototype = Some(Rc::clone(proto));
                obj.promise_data = Some(PromiseObjectData::new());
            }
            Ok(promise_rc)
        }
    }
}

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

    {
        let mut obj = promise_rc.borrow_mut();
        obj.prototype = Some(proto);
        obj.promise_data = Some(promise_data);
    }

    if total == 0 {
        let mut obj = promise_rc.borrow_mut();
        if let Some(ref mut data) = obj.promise_data {
            data.fulfill(Value::Object(Rc::new(RefCell::new(
                Object::new_array_from(vec![]),
            ))));
        }
        return Ok(Value::Object(Rc::clone(&promise_rc)));
    }

    let results = Rc::new(RefCell::new(vec![Value::Undefined; total]));
    let fulfilled_count = Rc::new(RefCell::new(0usize));
    let rejected_flag = Rc::new(RefCell::new(false));

    for (i, value) in values.into_iter().enumerate() {
        process_promise_all_value(
            i,
            value,
            total,
            &promise_rc,
            &results,
            &fulfilled_count,
            &rejected_flag,
        );
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

    let resolve_fn =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
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
                            Object::new_array_from(results_f.borrow().clone()),
                        ))));
                    }
                }
            }
            Ok(Value::Undefined)
        })));

    let reject_fn = Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
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
    })));

    let ctx = PromiseAllContext {
        promise_rc,
        results,
        fulfilled_count,
        rejected_flag,
    };
    attach_callbacks_to_value(value, idx, total, ctx, resolve_fn, reject_fn);
}

struct PromiseAllContext<'a> {
    promise_rc: &'a Rc<RefCell<Object>>,
    results: &'a Rc<RefCell<Vec<Value>>>,
    fulfilled_count: &'a Rc<RefCell<usize>>,
    rejected_flag: &'a Rc<RefCell<bool>>,
}

fn attach_callbacks_to_value(
    value: Value,
    idx: usize,
    total: usize,
    ctx: PromiseAllContext,
    resolve_fn: Value,
    reject_fn: Value,
) {
    let is_promise = matches!(&value, Value::Object(p) if p.borrow().promise_data.is_some());
    if is_promise {
        if let Value::Object(ref p) = value {
            let obj = p.borrow();
            if let Some(ref data) = obj.promise_data {
                match data.state {
                    crate::value::object::PromiseState::Fulfilled => {
                        let result = data.result.clone();
                        let already_rejected = *ctx.rejected_flag.borrow();
                        {
                            let mut r = ctx.results.borrow_mut();
                            r[idx] = result;
                        }
                        {
                            let mut c = ctx.fulfilled_count.borrow_mut();
                            *c += 1;
                            if *c == total && !already_rejected {
                                // Clone results before borrowing promise_rc
                                let results_clone = ctx.results.borrow().clone();
                                drop(c);
                                let mut p = ctx.promise_rc.borrow_mut();
                                if let Some(ref mut d) = p.promise_data {
                                    d.fulfill(Value::Object(Rc::new(RefCell::new(
                                        Object::new_array_from(results_clone),
                                    ))));
                                }
                            }
                        }
                    }
                    crate::value::object::PromiseState::Rejected => {
                        let mut r = ctx.rejected_flag.borrow_mut();
                        *r = true;
                        {
                            let mut p = ctx.promise_rc.borrow_mut();
                            if let Some(ref mut d) = p.promise_data {
                                d.reject(data.result.clone());
                            }
                        }
                        // Process callbacks on the outer promise
                        let promise_rc_clone = Rc::clone(ctx.promise_rc);
                        drop(r);
                        enqueue_promise_reactions(&promise_rc_clone);
                    }
                    _ => attach_then_handlers(p, resolve_fn, reject_fn),
                }
            }
        }
    } else {
        let already_rejected = *ctx.rejected_flag.borrow();
        {
            let mut r = ctx.results.borrow_mut();
            r[idx] = value;
        }
        {
            let mut c = ctx.fulfilled_count.borrow_mut();
            *c += 1;
            if *c == total && !already_rejected {
                // Clone results before borrowing promise_rc
                let results_clone = ctx.results.borrow().clone();
                drop(c);
                let mut p = ctx.promise_rc.borrow_mut();
                if let Some(ref mut d) = p.promise_data {
                    d.fulfill(Value::Object(Rc::new(RefCell::new(
                        Object::new_array_from(results_clone),
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

        let on_fulfilled =
            Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let cb = pf.borrow().clone();
                if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                    let _ = call_value_with_this(cb, vec![val], Value::Undefined);
                }
                Ok(Value::Undefined)
            })));
        let on_rejected =
            Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
                let reason = args.first().cloned().unwrap_or(Value::Undefined);
                let cb = pr.borrow().clone();
                if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                    let _ = call_value_with_this(cb, vec![reason], Value::Undefined);
                }
                Ok(Value::Undefined)
            })));
        let _ = call_value_with_this(
            then_method.clone(),
            vec![on_fulfilled, on_rejected],
            Value::Undefined,
        );
    }
}

/// Implements Promise.race
pub fn promise_race_impl(args: Vec<Value>, this_val: Value, proto: Rc<RefCell<Object>>) -> Result<Value, JsError> {
    // Validate this is callable (constructor) per spec
    let is_callable = matches!(
        this_val,
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) | Value::Class(_)
    );
    if !is_callable {
        return Err(JsError("TypeError: Promise.race called on non-constructor".to_string()));
    }

    // Per spec: invoke NewPromiseCapability(this) which validates the constructor.
    // This will throw if this is not a valid Promise constructor.
    let promise_rc = invoke_promise_constructor(&this_val, &proto)?;

    let input = args.first().cloned().unwrap_or(Value::Undefined);

    // Use iterator protocol - throws TypeError for non-iterables
    let values: Vec<Value> = get_iterator(&input)?;

    let settled = Rc::new(RefCell::new(false));

    for value in values {
        process_promise_race_value(value, &promise_rc, &settled);

        if *settled.borrow() {
            break;
        }
    }

    Ok(Value::Object(promise_rc))
}

#[allow(clippy::complexity)]
fn process_promise_race_value(
    value: Value,
    promise_rc: &Rc<RefCell<Object>>,
    settled: &Rc<RefCell<bool>>,
) {
    let (resolve_fn, reject_fn) = make_race_callbacks(promise_rc, settled);

    if let Value::Object(ref p) = value {
        let obj = p.borrow();
        if let Some(ref data) = obj.promise_data {
            match data.state {
                crate::value::object::PromiseState::Fulfilled => {
                    settle_promise(settled, promise_rc, |d| d.fulfill(data.result.clone()));
                }
                crate::value::object::PromiseState::Rejected => {
                    settle_promise(settled, promise_rc, |d| d.reject(data.result.clone()));
                }
                _ => {
                    if let Some(ref then_method) = obj.get("then") {
                        if attach_race_handlers(then_method, resolve_fn, reject_fn).is_err() {
                            // If then is not callable, reject with TypeError
                            let mut s = settled.borrow_mut();
                            if !*s {
                                *s = true;
                                drop(s);
                                let mut pr = promise_rc.borrow_mut();
                                if let Some(ref mut d) = pr.promise_data {
                                    d.reject(Value::String("TypeError: value.then is not a function".to_string()));
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        settle_promise(settled, promise_rc, |d| d.fulfill(value));
    }
}

fn make_race_callbacks(
    promise_rc: &Rc<RefCell<Object>>,
    settled: &Rc<RefCell<bool>>,
) -> (Value, Value) {
    let promise_rc_f = Rc::clone(promise_rc);
    let settled_f = Rc::clone(settled);
    let promise_rc_r = Rc::clone(promise_rc);
    let settled_r = Rc::clone(settled);

    let resolve_fn =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
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
        })));

    let reject_fn = Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
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
    })));

    (resolve_fn, reject_fn)
}

fn settle_promise<F>(settled: &Rc<RefCell<bool>>, promise_rc: &Rc<RefCell<Object>>, fulfill: F)
where
    F: FnOnce(&mut PromiseObjectData),
{
    let mut s = settled.borrow_mut();
    if !*s {
        *s = true;
        let mut pr = promise_rc.borrow_mut();
        if let Some(ref mut d) = pr.promise_data {
            fulfill(d);
        }
    }
}

fn attach_race_handlers(then_method: &Value, resolve_fn: Value, reject_fn: Value) -> Result<Value, JsError> {
    let pf = Rc::new(RefCell::new(resolve_fn.clone()));
    let pr = Rc::new(RefCell::new(reject_fn.clone()));

    let on_fulfilled =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
            let val = args.first().cloned().unwrap_or(Value::Undefined);
            let cb = pf.borrow().clone();
            if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                let _ = call_value_with_this(cb, vec![val], Value::Undefined);
            }
            Ok(Value::Undefined)
        })));
    let on_rejected =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
            let reason = args.first().cloned().unwrap_or(Value::Undefined);
            let cb = pr.borrow().clone();
            if matches!(cb, Value::Function(_) | Value::NativeFunction(_)) {
                let _ = call_value_with_this(cb, vec![reason], Value::Undefined);
            }
            Ok(Value::Undefined)
        })));
    call_value_with_this(
        then_method.clone(),
        vec![on_fulfilled, on_rejected],
        Value::Undefined,
    )
}
