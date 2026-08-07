//! NewPromiseCapability (25.4.1.5) — creates a {promise, resolve, reject} tuple.

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::error::create_js_error_with_type;
use crate::value::object::PromiseObjectData;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};

/// Validate that `constructor` works as a Promise constructor by calling it
/// with resolve/reject callbacks. Returns the created promise or an error.
/// Validate that `constructor` works as a Promise constructor by calling it
/// with resolve/reject callbacks. Returns the created promise or an error.
/// Implements NewPromiseCapability per spec.
#[allow(dead_code)] // False positive: #[path] bypasses cross-module tracking
pub fn invoke_promise_constructor(
    constructor: &Value,
    proto: &Rc<RefCell<Object>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let resolve_slot: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let reject_slot: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));

    let resolve_already_set: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let reject_already_set: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let resolve_already_set_f = Rc::clone(&resolve_already_set);
    let reject_already_set_f = Rc::clone(&reject_already_set);
    let resolve_slot_f = Rc::clone(&resolve_slot);
    let reject_slot_f = Rc::clone(&reject_slot);

    let resolve_fn =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
            let resolve_arg = args.first().cloned().unwrap_or(Value::Undefined);
            let reject_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            if !matches!(&resolve_arg, Value::Undefined) {
                if *resolve_already_set_f.borrow() {
                    return Err(JsError(
                        "TypeError: GetCapabilitiesExecutor resolve already called".to_string(),
                    ));
                }
                *resolve_already_set_f.borrow_mut() = true;
            }
            *resolve_slot_f.borrow_mut() = Some(resolve_arg);

            if !matches!(&reject_arg, Value::Undefined) {
                if *reject_already_set_f.borrow() {
                    return Err(JsError(
                        "TypeError: GetCapabilitiesExecutor reject already called".to_string(),
                    ));
                }
                *reject_already_set_f.borrow_mut() = true;
            }
            *reject_slot_f.borrow_mut() = Some(reject_arg);

            Ok(Value::Undefined)
        })));

    let reject_fn =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_args: Vec<Value>| {
            Ok(Value::Undefined)
        })));

    let result = call_value_with_this(
        constructor.clone(),
        vec![resolve_fn, reject_fn],
        Value::Undefined,
    );

    if let Err(e) = &result {
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

    let resolve_val = resolve_slot.borrow();
    let reject_val = reject_slot.borrow();

    let resolve_callable = matches!(
        &*resolve_val,
        Some(v) if matches!(v, Value::Object(_) | Value::Function(_) | Value::NativeFunction(_))
    );
    let reject_callable = matches!(
        &*reject_val,
        Some(v) if matches!(v, Value::Object(_) | Value::Function(_) | Value::NativeFunction(_))
    );

    if !resolve_callable {
        let (err_val, _) = create_js_error_with_type(
            "GetCapabilitiesExecutor resolve is not callable",
            "TypeError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(JsError(
            "TypeError: GetCapabilitiesExecutor resolve is not callable".to_string(),
        ));
    }

    if !reject_callable {
        let (err_val, _) = create_js_error_with_type(
            "GetCapabilitiesExecutor reject is not callable",
            "TypeError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(JsError(
            "TypeError: GetCapabilitiesExecutor reject is not callable".to_string(),
        ));
    }

    match result {
        Value::Object(o) => Ok(o),
        _ => {
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
