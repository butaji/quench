//! Promise.race implementation.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::get_current_env;
use crate::eval::call_value_with_this;
use crate::eval::iteration::get_iterator;
use crate::eval::member::eval_member_access;
use crate::value::error::create_js_error_with_type;
use crate::value::{JsError, NativeFunction, Object, Value};

use crate::builtins::promise::callbacks::enqueue_promise_reactions;
use crate::builtins::promise::capability::invoke_promise_constructor;

/// Implements Promise.race
pub fn promise_race_impl(
    args: Vec<Value>,
    this_val: Value,
    proto: Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let is_callable = matches!(
        this_val,
        Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_)
    );
    if !is_callable {
        return Err(JsError(
            "TypeError: Promise.race called on non-constructor".to_string(),
        ));
    }

    let env = get_current_env().ok_or_else(|| JsError::from("No context for Get"))?;
    let promise_resolve = eval_member_access(&this_val, "resolve", &env)?;

    let is_callable = matches!(
        promise_resolve,
        Value::Function(_) | Value::NativeFunction(_) | Value::Class(_)
    );
    if !is_callable {
        let (err_val, _) =
            create_js_error_with_type("Promise.resolve is not a function", "TypeError");
        crate::value::set_thrown_value(err_val);
        return Err(JsError(
            "TypeError: Promise.resolve is not a function".to_string(),
        ));
    }

    let promise_rc = invoke_promise_constructor(&this_val, &proto)?;

    let settled = Rc::new(RefCell::new(false));
    let settled_f = Rc::clone(&settled);
    let promise_rc_f = Rc::clone(&promise_rc);
    let race_resolve =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
            let result = args.first().cloned().unwrap_or(Value::Undefined);
            let mut s = settled_f.borrow_mut();
            if !*s {
                *s = true;
                {
                    let mut pr = promise_rc_f.borrow_mut();
                    if let Some(ref mut d) = pr.promise_data {
                        d.fulfill(result);
                    }
                }
                enqueue_promise_reactions(&promise_rc_f);
            }
            Ok(Value::Undefined)
        })));
    let settled_r = Rc::clone(&settled);
    let promise_rc_r = Rc::clone(&promise_rc);
    let race_reject =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
            let reason = args.first().cloned().unwrap_or(Value::Undefined);
            let mut s = settled_r.borrow_mut();
            if !*s {
                *s = true;
                {
                    let mut pr = promise_rc_r.borrow_mut();
                    if let Some(ref mut d) = pr.promise_data {
                        d.reject(reason);
                    }
                }
                enqueue_promise_reactions(&promise_rc_r);
            }
            Ok(Value::Undefined)
        })));

    let input = args.first().cloned().unwrap_or(Value::Undefined);
    let values: Vec<Value> = get_iterator(&input)?;

    for value in values {
        let next_promise =
            call_value_with_this(promise_resolve.clone(), vec![value], this_val.clone())?;

        let then_info: Option<(Rc<RefCell<Object>>, Value)> =
            if let Value::Object(ref p) = next_promise {
                p.borrow()
                    .get("then")
                    .map(|then_method| (Rc::clone(p), then_method))
            } else {
                None
            };
        if let Some((p_rc, then_method)) = then_info {
            let _ = call_value_with_this(
                then_method,
                vec![race_resolve.clone(), race_reject.clone()],
                Value::Object(p_rc),
            );
        }
    }

    Ok(Value::Object(promise_rc))
}
