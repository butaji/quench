//! Member access evaluation (property lookup on objects, strings, functions, etc.)

use crate::env::Environment;
use crate::eval::object::call_getter;
use crate::value::{to_js_string, to_number, JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on a value
pub fn eval_member_access(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => eval_object_member(o, prop_name),
        Value::String(s) => eval_string_member(s, prop_name),
        Value::Function(f) => eval_function_member(f, prop_name),
        Value::NativeFunction(nf) => eval_native_function_member(nf, prop_name),
        Value::NativeConstructor(nc) => eval_native_constructor_member(nc, prop_name),
        Value::Number(_) => eval_number_member(obj_val, prop_name, env),
        _ => Ok(Value::Undefined),
    }
}

fn eval_object_member(o: &Rc<RefCell<Object>>, prop_name: &str) -> Result<Value, JsError> {
    // Check getter first
    {
        let obj = o.borrow();
        if let Some(getter_storage) = obj.get_getter(prop_name) {
            let getter_clone = getter_storage.clone();
            drop(obj);
            return call_getter(o, &getter_clone, &Rc::new(RefCell::new(Environment::new())));
        }
    }
    // Check regular properties
    {
        let obj = o.borrow();
        if let Some(val) = obj.get(prop_name) {
            return Ok(val);
        }
    }
    // Handle Date.prototype specially
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Date && prop_name == "prototype" {
            let mut proto = Object::new(ObjectKind::Ordinary);
            proto.set("constructor", Value::Object(Rc::clone(o)));
            return Ok(Value::Object(Rc::new(RefCell::new(proto))));
        }
    }
    Ok(Value::Undefined)
}

fn eval_string_member(s: &str, prop_name: &str) -> Result<Value, JsError> {
    let s = s.to_string();
    let prop_name = prop_name.to_string();
    match prop_name.as_str() {
        "length" => Ok(Value::Number(s.len() as f64)),
        "charAt" | "charCodeAt" | "indexOf" | "substring" | "slice"
        | "toUpperCase" | "toLowerCase" | "trim" | "split"
        | "includes" | "startsWith" | "endsWith" | "replace" | "match"
        | "search" | "concat" => string_method(&s, &prop_name),
        _ => Ok(Value::Undefined),
    }
}

fn string_method(s: &str, method: &str) -> Result<Value, JsError> {
    let s = s.to_string();
    let method = method.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let s = s.clone();
        match method.as_str() {
            "length" => Ok(Value::Number(s.len() as f64)),
            "charAt" => {
                let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
            }
            "indexOf" => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
            }
            "toUpperCase" => Ok(Value::String(s.to_uppercase())),
            "toLowerCase" => Ok(Value::String(s.to_lowercase())),
            "trim" => Ok(Value::String(s.trim().to_string())),
            "includes" => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Boolean(s.contains(&needle)))
            }
            "startsWith" => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Boolean(s.starts_with(&needle)))
            }
            "endsWith" => {
                let needle = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Boolean(s.ends_with(&needle)))
            }
            "concat" => {
                let sep = args.iter().map(to_js_string).collect::<Vec<_>>().join("");
                Ok(Value::String(format!("{}{}", s, sep)))
            }
            "split" => {
                let sep = args.first().map(to_js_string).unwrap_or_default();
                let parts: Vec<Value> = if sep.is_empty() {
                    s.chars().map(|c| Value::String(c.to_string())).collect()
                } else {
                    s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                };
                Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(parts.len())))))
            }
            "substring" => {
                let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
                let start = start.min(s.len());
                let end = end.min(s.len());
                let start = start.min(end);
                Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
            }
            "slice" => {
                let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0) as isize;
                let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
                let len = s.len() as isize;
                let start = if start < 0 { (len + start).max(0) as usize } else { start as usize }.min(len as usize);
                let end = if end < 0 { (len + end).max(0) as usize } else { end as usize }.min(len as usize);
                let end = end.max(start);
                Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
            }
            "match" => {
                let pattern = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Boolean(s.contains(&pattern)))
            }
            "search" => {
                let pattern = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
            }
            _ => Ok(Value::Undefined),
        }
    }))))
}

pub fn eval_function_member(f: &crate::value::ValueFunction, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "name" => Ok(Value::String(f.name.clone().unwrap_or_default())),
        "prototype" => {
            let proto = f.get_prototype();
            Ok(Value::Object(proto))
        }
        "call" => {
            // Create a native function for call that invokes the ValueFunction
            let func_clone = f.clone();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |args: Vec<Value>| {
                    let this_val = args.first().cloned().unwrap_or(Value::Undefined);
                    let remaining_args: Vec<Value> = args.into_iter().skip(1).collect();
                    crate::eval::function::call_js_function_with_this(
                        func_clone.clone(),
                        remaining_args,
                        this_val,
                    )
                }
            ))))
        }
        "apply" => {
            // Create a native function for apply that invokes the ValueFunction
            let func_clone = f.clone();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |args: Vec<Value>| {
                    let this_val = args.first().cloned().unwrap_or(Value::Undefined);
                    let args_array = args.get(1);
                    let spread_args = if let Some(Value::Object(o)) = args_array {
                        let o = o.borrow();
                        (0..o.elements.len())
                            .filter_map(|i| o.elements.get(i).cloned())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    crate::eval::function::call_js_function_with_this(
                        func_clone.clone(),
                        spread_args,
                        this_val,
                    )
                }
            ))))
        }
        _ => Ok(Value::Undefined),
    }
}

pub fn eval_native_function_member(nf: &Rc<NativeFunction>, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "name" => Ok(Value::String("anonymous".to_string())),
        "prototype" => {
            let mut proto = Object::new(ObjectKind::Ordinary);
            proto.set("constructor", Value::NativeFunction(Rc::clone(nf)));
            Ok(Value::Object(Rc::new(RefCell::new(proto))))
        }
        "length" => Ok(Value::Number(0.0)),
        "call" => Ok(Value::NativeFunction(Rc::clone(nf))),
        "apply" => Ok(Value::NativeFunction(Rc::clone(nf))),
        _ => Ok(Value::Undefined),
    }
}

pub(crate) fn eval_native_constructor_member(nc: &Rc<NativeConstructor>, prop_name: &str) -> Result<Value, JsError> {
    // Check static methods first
    if let Some(val) = nc.get_static_method(prop_name) {
        return Ok(val);
    }

    match prop_name {
        "prototype" => Ok(Value::Object(Rc::clone(&nc.prototype))),
        "length" => Ok(Value::Number(0.0)),
        "name" => Ok(Value::String("anonymous".to_string())),
        _ => Ok(Value::Undefined),
    }
}

fn eval_number_member(
    _obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
        let num_obj = num_obj.borrow();
        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
            let proto_obj = proto.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}
