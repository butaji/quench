//! Function member access evaluation

use crate::value::{create_js_error_with_type, set_thrown_value, JsError, NativeFunction, Value, ValueFunction};
use std::rc::Rc;

/// Evaluate member access on a JS function
pub fn eval_function_member(f: &ValueFunction, prop_name: &str) -> Result<Value, JsError> {
    // Check custom properties first (e.g., sameValue, notSameValue on assert)
    if let Some(val) = f.get_property(prop_name) {
        return Ok(val);
    }
    if f.is_arrow && (prop_name == "arguments" || prop_name == "caller") {
        let msg = format!(
            "'caller' and 'arguments' are restricted properties and cannot be accessed on arrow functions"
        );
        let (err, js_err) = create_js_error_with_type(&msg, "TypeError");
        set_thrown_value(err);
        return Err(js_err);
    }
    match prop_name {
        "name" => Ok(Value::String(f.name.clone().unwrap_or_default())),
        "length" => eval_function_length(f),
        "prototype" => Ok(Value::Object(f.get_prototype())),
        "call" => eval_function_call_method(f),
        "apply" => eval_function_apply_method(f),
        "bind" => eval_function_bind_method(f),
        _ => Ok(f
            .get_prototype()
            .borrow()
            .get(prop_name)
            .unwrap_or(Value::Undefined)),
    }
}

fn eval_function_length(f: &ValueFunction) -> Result<Value, JsError> {
    let mut count = 0;
    for p in &f.params {
        if p.default.is_some() {
            break;
        }
        count += 1;
    }
    Ok(Value::Number(count as f64))
}

/// Handle Function.prototype.call
fn eval_function_call_method(f: &ValueFunction) -> Result<Value, JsError> {
    let func_clone = f.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::Function(func_clone.clone()));
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let remaining_args: Vec<Value> = args.into_iter().skip(1).collect();
            let result = crate::eval::function::call_js_function_impl(
                func_clone.clone(),
                remaining_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle Function.prototype.apply
fn eval_function_apply_method(f: &ValueFunction) -> Result<Value, JsError> {
    let func_clone = f.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::Function(func_clone.clone()));
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let args_array = args.get(1);
            let spread_args = if let Some(Value::Object(o)) = args_array {
                let o = o.borrow();
                (0..o.elements.len())
                    .filter_map(|i| o.elements.get(i).cloned())
                    .collect()
            } else {
                Vec::new()
            };
            let result = crate::eval::function::call_js_function_impl(
                func_clone.clone(),
                spread_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle Function.prototype.bind
fn eval_function_bind_method(f: &ValueFunction) -> Result<Value, JsError> {
    let func_clone = f.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::Function(func_clone.clone()));
            let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(bound_this.clone());
            let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
            let target_func = func_clone.clone();
            let result = Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |call_args: Vec<Value>| {
                    crate::interpreter::set_native_this(Value::Function(target_func.clone()));
                    crate::interpreter::set_this_value(bound_this.clone());
                    let all_args: Vec<Value> =
                        bound_args.iter().cloned().chain(call_args).collect();
                    let result = crate::eval::function::call_js_function_impl(
                        target_func.clone(),
                        all_args,
                        bound_this.clone(),
                    );
                    crate::interpreter::take_native_this();
                    crate::interpreter::take_this_value();
                    result
                },
            ))));
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}
