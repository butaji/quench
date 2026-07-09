//! Member access evaluation (property lookup on objects, strings, functions, etc.)

use crate::ast::Param;
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
        Value::Class(class) => eval_class_member(class, prop_name, env),
        _ => Ok(Value::Undefined),
    }
}

/// Evaluate member access on a class (static methods and prototype)
pub fn eval_class_member(
    class: &crate::value::ClassValue,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match prop_name {
        "prototype" => {
            // Create and cache the prototype for this class
            let proto = get_class_prototype_cached(class, env)?;
            Ok(Value::Object(proto))
        }
        "name" => {
            Ok(Value::String(class.name.clone().unwrap_or_default()))
        }
        _ => {
            // Check static methods
            for (name, params, body) in &class.static_methods {
                if prop_key_matches(name, prop_name) {
                    let params_vec: Vec<Param> = params.iter().map(|p| Param::new(p)).collect();
                    let func = crate::value::ValueFunction::new(
                        Some(prop_name.to_string()),
                        params_vec,
                        body.clone(),
                        Rc::clone(env),
                    );
                    return Ok(Value::Function(func));
                }
            }
            Ok(Value::Undefined)
        }
    }
}

/// Check if a property key matches a name
fn prop_key_matches(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => s == name,
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => n.to_string() == name,
        crate::ast::PropertyKey::Computed(_) => false,
    }
}

/// Get or create the prototype for a class (for member access)
fn get_class_prototype_cached(
    class: &crate::value::ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Use the shared prototype from ClassValue
    // This ensures that instanceof checks work correctly
    crate::eval::class::get_or_create_class_prototype(class, env)
}

/// Helper to convert PropertyKey to string
fn prop_key_to_string(key: &crate::ast::PropertyKey) -> String {
    match key {
        crate::ast::PropertyKey::Ident(s) => s.clone(),
        crate::ast::PropertyKey::String(s) => s.clone(),
        crate::ast::PropertyKey::Number(n) => n.to_string(),
        crate::ast::PropertyKey::Computed(_) => "[computed]".to_string(),
    }
}

/// Get prototype from a class value
fn get_prototype_from_class_val(val: &Value) -> Option<Rc<RefCell<Object>>> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Some(proto_obj.clone())
            } else {
                None
            }
        }
        Value::Class(class) => {
            // Recursively get prototype - this is a simplified version
            None
        }
        _ => None,
    }
}

fn eval_object_member(o: &Rc<RefCell<Object>>, prop_name: &str) -> Result<Value, JsError> {
    // Check getter first (on this object and prototype chain)
    {
        let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
        while let Some(obj_rc) = current {
            {
                let obj = obj_rc.borrow();
                if let Some(getter_storage) = obj.get_getter(prop_name) {
                    let getter_clone = getter_storage.clone();
                    drop(obj);
                    // Use the original object 'o' as 'this' for the getter
                    return call_getter(o, &getter_clone, &Rc::new(RefCell::new(Environment::new())));
                }
                // Check regular property
                if let Some(val) = obj.properties.get(prop_name) {
                    return Ok(val.clone());
                }
                // Check array elements
                if let Ok(idx) = prop_name.parse::<usize>() {
                    if idx < obj.elements.len() {
                        return Ok(obj.elements[idx].clone());
                    }
                }
                // Move to prototype
                current = obj.prototype.as_ref().map(Rc::clone);
            }
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
    // Arrow functions are always strict mode and cannot have 'arguments' or 'caller'
    if f.is_arrow {
        if prop_name == "arguments" || prop_name == "caller" {
            return Err(JsError("TypeError: 'arguments' and 'caller' are restricted properties and cannot be accessed on arrow functions".to_string()));
        }
    }
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
        "bind" => {
            // Create a bound function that remembers 'this' and initial arguments
            let func_clone = f.clone();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |args: Vec<Value>| {
                    let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
                    let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                    // Return a wrapper that combines bound args with call args
                    let target_func = func_clone.clone();
                    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                        move |call_args: Vec<Value>| {
                            let all_args: Vec<Value> = bound_args.iter().cloned().chain(call_args).collect();
                            crate::eval::function::call_js_function_with_this(
                                target_func.clone(),
                                all_args,
                                bound_this.clone(),
                            )
                        }
                    ))))
                }
            ))))
        }
        // For other properties, look up on the prototype chain
        _ => {
            let proto = f.get_prototype();
            let result = proto.borrow().get(prop_name).unwrap_or(Value::Undefined);
            Ok(result)
        }
    }
}

pub fn eval_native_function_member(nf: &Rc<NativeFunction>, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "name" => Ok(Value::String("anonymous".to_string())),
        "prototype" => {
            // If NativeFunction has a prototype, use it; otherwise create a new one
            if let Some(ref proto) = nf.prototype {
                Ok(Value::Object(Rc::clone(proto)))
            } else {
                let mut proto = Object::new(ObjectKind::Ordinary);
                proto.set("constructor", Value::NativeFunction(Rc::clone(nf)));
                Ok(Value::Object(Rc::new(RefCell::new(proto))))
            }
        }
        "length" => Ok(Value::Number(0.0)),
        "call" => {
            // Create a wrapper that sets 'this' to the first argument
            let nf_clone = nf.clone();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |args: Vec<Value>| {
                    let this_val = args.first().cloned().unwrap_or(Value::Undefined);
                    let remaining_args: Vec<Value> = args.into_iter().skip(1).collect();
                    crate::interpreter::set_native_this(this_val);
                    nf_clone.call(remaining_args)
                },
            ))))
        }
        "apply" => {
            // Create a wrapper that handles apply semantics
            let nf_clone = nf.clone();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |args: Vec<Value>| {
                    let this_val = args.first().cloned().unwrap_or(Value::Undefined);
                    let spread_args = if let Some(Value::Object(o)) = args.get(1) {
                        let o = o.borrow();
                        (0..o.elements.len())
                            .filter_map(|i| o.elements.get(i).cloned())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    crate::interpreter::set_native_this(this_val);
                    nf_clone.call(spread_args)
                },
            ))))
        }
        "bind" => {
            // Create a bound function for native functions
            let nf_clone = nf.clone();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                move |args: Vec<Value>| {
                    let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
                    let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                    let target_nf = nf_clone.clone();
                    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(
                        move |call_args: Vec<Value>| {
                            let all_args: Vec<Value> = bound_args.iter().cloned().chain(call_args).collect();
                            crate::interpreter::set_native_this(bound_this.clone());
                            target_nf.call(all_args)
                        }
                    ))))
                }
            ))))
        }
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
    // Try looking up Number.prototype for both Object and NativeFunction
    if let Some(num_val) = env.borrow().get("Number") {
        let proto = match &num_val {
            Value::Object(num_obj) => {
                let num_obj = num_obj.borrow();
                num_obj.get("prototype")
            }
            Value::NativeFunction(nf) => {
                // For NativeFunction with prototype, return it
                nf.prototype.as_ref().map(|p| Value::Object(Rc::clone(p)))
            }
            _ => None,
        };
        if let Some(Value::Object(proto_obj)) = proto {
            let proto_obj = proto_obj.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}
