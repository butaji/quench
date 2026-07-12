//! Native function and constructor member access evaluation

use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on a native function
pub fn eval_native_function_member(
    nf: &Rc<NativeFunction>,
    prop_name: &str,
) -> Result<Value, JsError> {
    // Check custom properties first
    if let Some(val) = nf.get_property(prop_name) {
        return Ok(val);
    }
    match prop_name {
        "name" => Ok(Value::String("anonymous".to_string())),
        "prototype" => eval_native_prototype(nf),
        "length" => Ok(Value::Number(0.0)),
        "call" => eval_native_call_method(nf),
        "apply" => eval_native_apply_method(nf),
        "bind" => eval_native_bind_method(nf),
        "toPrimitive" | "hasInstance" | "isConcatSpreadable" => eval_well_known_symbol(prop_name),
        _ => Ok(Value::Undefined),
    }
}

fn eval_native_prototype(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    if let Some(ref proto) = nf.prototype {
        Ok(Value::Object(Rc::clone(proto)))
    } else {
        let mut proto = Object::new(ObjectKind::Ordinary);
        proto.set("constructor", Value::NativeFunction(Rc::clone(nf)));
        Ok(Value::Object(Rc::new(RefCell::new(proto))))
    }
}

fn eval_well_known_symbol(prop_name: &str) -> Result<Value, JsError> {
    if let Some(sym) = crate::builtins::symbol::get_well_known_symbol_no_ctx(prop_name) {
        Ok(sym)
    } else {
        Ok(Value::Undefined)
    }
}

/// Handle NativeFunction.prototype.call
fn eval_native_call_method(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    let nf_clone = nf.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::NativeFunction(nf_clone.clone()));
            let this_val = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(this_val.clone());
            let remaining_args: Vec<Value> = args.into_iter().skip(1).collect();
            let result = crate::eval::function::call_native_function(
                nf_clone.clone(),
                remaining_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle NativeFunction.prototype.apply
fn eval_native_apply_method(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    let nf_clone = nf.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::NativeFunction(nf_clone.clone()));
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
            let result = crate::eval::function::call_native_function(
                nf_clone.clone(),
                spread_args,
                this_val,
            );
            crate::interpreter::take_native_this();
            crate::interpreter::take_this_value();
            result
        },
    ))))
}

/// Handle NativeFunction.prototype.bind
fn eval_native_bind_method(nf: &Rc<NativeFunction>) -> Result<Value, JsError> {
    let nf_clone = nf.clone();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
        move |args: Vec<Value>| {
            crate::interpreter::set_native_this(Value::NativeFunction(nf_clone.clone()));
            let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
            crate::interpreter::set_this_value(bound_this.clone());
            let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
            let target_nf = nf_clone.clone();
            let result = Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |call_args: Vec<Value>| {
                    crate::interpreter::set_native_this(Value::NativeFunction(target_nf.clone()));
                    crate::interpreter::set_this_value(bound_this.clone());
                    let all_args: Vec<Value> =
                        bound_args.iter().cloned().chain(call_args).collect();
                    let result = crate::eval::function::call_native_function(
                        target_nf.clone(),
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

/// Evaluate member access on a native constructor
pub fn eval_native_constructor_member(
    nc: &Rc<NativeConstructor>,
    prop_name: &str,
) -> Result<Value, JsError> {
    // Check static methods first
    if let Some(val) = nc.get_static_method(prop_name) {
        return Ok(val);
    }

    // Check if this is the Function constructor
    let is_function_constructor = crate::builtins::function::get_function_prototype()
        .map(|fp| Rc::ptr_eq(&fp, &nc.prototype))
        .unwrap_or(false);

    match prop_name {
        "prototype" => Ok(Value::Object(Rc::clone(&nc.prototype))),
        "length" => {
            if is_function_constructor {
                Ok(Value::Number(1.0))
            } else {
                Ok(Value::Number(0.0))
            }
        }
        "name" => {
            if is_function_constructor {
                Ok(Value::String("Function".to_string()))
            } else {
                Ok(Value::String("anonymous".to_string()))
            }
        }
        _ => Ok(Value::Undefined),
    }
}
