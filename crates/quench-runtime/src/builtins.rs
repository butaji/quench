#![allow(unknown_lints, file_length)]
//! Built-in JavaScript objects and functions

use std::rc::Rc;
use std::cell::RefCell;
use serde::ser::{SerializeMap, SerializeSeq};

use crate::value::{Value, JsError, Object, ObjectKind, NativeFunction, to_js_string, to_number, to_bool};
use crate::Context;
use crate::interpreter::{get_native_this, call_value_with_this};

// Thread-local storage for Array.prototype (used by interpreter for array literal creation)
thread_local! {
    static ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = RefCell::new(None);
}

/// Get the Array.prototype object (for use by interpreter)
pub fn get_array_prototype() -> Option<Rc<RefCell<Object>>> {
    ARRAY_PROTOTYPE.with(|ap| ap.borrow().clone())
}

/// Register all built-in globals into the context
pub fn register_builtins(ctx: &mut Context) {
    register_console(ctx);
    register_json(ctx);
    register_math(ctx);
    register_object(ctx);
    register_array(ctx);
    register_map_and_set(ctx);
    register_global_functions(ctx);
    register_function(ctx);
    register_error(ctx);
}

// ============================================================================
// Console
// ============================================================================

fn register_console(ctx: &mut Context) {
    let console = Object::new(ObjectKind::Ordinary);
    let console = Rc::new(RefCell::new(console));
    
    console.borrow_mut().set("log", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let msg = args.iter().map(to_js_string).collect::<Vec<_>>().join(" ");
        println!("{}", msg);
        Ok(Value::Undefined)
    }))));
    
    console.borrow_mut().set("error", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let msg = args.iter().map(to_js_string).collect::<Vec<_>>().join(" ");
        eprintln!("{}", msg);
        Ok(Value::Undefined)
    }))));
    
    console.borrow_mut().set("warn", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let msg = args.iter().map(to_js_string).collect::<Vec<_>>().join(" ");
        println!("{}", msg);
        Ok(Value::Undefined)
    }))));
    
    ctx.set_global("console".to_string(), Value::Object(console));
}

// ============================================================================
// JSON
// ============================================================================

fn register_json(ctx: &mut Context) {
    let json_obj = Object::new(ObjectKind::Ordinary);
    let json = Rc::new(RefCell::new(json_obj));
    
    json.borrow_mut().set("stringify", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        let result = serde_json::to_string(&JsValueProxy(&val)).unwrap_or_else(|_| "null".to_string());
        Ok(Value::String(result))
    }))));
    
    json.borrow_mut().set("parse", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let text = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(text))
    }))));
    
    ctx.set_global("JSON".to_string(), Value::Object(json));
}

// ============================================================================
// Math
// ============================================================================

fn register_math(ctx: &mut Context) {
    let math = Object::new(ObjectKind::Ordinary);
    let math = Rc::new(RefCell::new(math));
    
    macro_rules! math_fn {
        ($name:expr, $fn:expr) => {
            math.borrow_mut().set($name, Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                let x = args.first().map(to_number).unwrap_or(0.0);
                Ok(Value::Number($fn(x)))
            }))));
        };
    }
    
    math_fn!("abs", f64::abs);
    math_fn!("floor", f64::floor);
    math_fn!("ceil", f64::ceil);
    math_fn!("round", f64::round);
    math_fn!("sqrt", f64::sqrt);
    math.borrow_mut().set("pow", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let base = args.first().map(to_number).unwrap_or(0.0);
        let exp = args.get(1).map(to_number).unwrap_or(1.0);
        Ok(Value::Number(base.powf(exp)))
    }))));
    
    math.borrow_mut().set("max", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let max = args.iter().map(to_number).fold(f64::NEG_INFINITY, f64::max);
        Ok(Value::Number(max))
    }))));
    
    math.borrow_mut().set("min", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let min = args.iter().map(to_number).fold(f64::INFINITY, f64::min);
        Ok(Value::Number(min))
    }))));
    
    math.borrow_mut().set("PI", Value::Number(std::f64::consts::PI));
    math.borrow_mut().set("E", Value::Number(std::f64::consts::E));
    math.borrow_mut().set("random", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(rand_simple()))
    }))));
    
    ctx.set_global("Math".to_string(), Value::Object(math));
}

fn rand_simple() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    (nanos as f64) / (u32::MAX as f64)
}

// ============================================================================
// Object
// ============================================================================

fn register_object(ctx: &mut Context) {
    let object = Object::new(ObjectKind::Ordinary);
    let object = Rc::new(RefCell::new(object));
    
    object.borrow_mut().set("keys", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().ok_or_else(|| JsError::from("Object.keys requires argument"))?;
        if let Value::Object(o) = obj {
            let keys: Vec<Value> = o.borrow().properties.keys()
                .map(|k| Value::String(k.clone()))
                .collect();
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array_from(keys)))))
        } else {
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
        }
    }))));
    
    object.borrow_mut().set("values", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().ok_or_else(|| JsError::from("Object.values requires argument"))?;
        if let Value::Object(o) = obj {
            let values: Vec<Value> = o.borrow().properties.values().cloned().collect();
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array_from(values)))))
        } else {
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
        }
    }))));
    
    object.borrow_mut().set("entries", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().ok_or_else(|| JsError::from("Object.entries requires argument"))?;
        if let Value::Object(o) = obj {
            let entries: Vec<Value> = o.borrow().properties.iter()
                .map(|(k, v)| Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
                    Value::String(k.clone()),
                    v.clone()
                ])))))
                .collect();
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array_from(entries)))))
        } else {
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
        }
    }))));
    
    object.borrow_mut().set("assign", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        for arg in args.iter().skip(1) {
            if let Value::Object(src) = arg {
                for (k, v) in src.borrow().properties.iter() {
                    if let Value::Object(to) = &target {
                        to.borrow_mut().set(k, v.clone());
                    }
                }
            }
        }
        Ok(target)
    }))));
    
    object.borrow_mut().set("create", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let proto = args.first().and_then(|v| {
            if let Value::Object(o) = v {
                Some(Rc::clone(o))
            } else {
                None
            }
        });
        let mut obj = if let Some(p) = proto {
            Object::with_prototype(ObjectKind::Ordinary, p)
        } else {
            Object::new(ObjectKind::Ordinary)
        };
        
        if let Some(Value::Object(props_obj)) = args.get(1) {
            for (k, v) in props_obj.borrow().properties.iter() {
                obj.set(k, v.clone());
            }
        }
        
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    }))));
    
    object.borrow_mut().set("defineProperty", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        let prop = args.get(1).map(to_js_string).unwrap_or_default();
        let value = args.get(2).and_then(|v| {
            if let Value::Object(o) = v {
                o.borrow().properties.get("value").cloned()
            } else {
                None
            }
        }).unwrap_or(Value::Undefined);
        
        if let Value::Object(o) = &obj {
            o.borrow_mut().set(&prop, value);
        }
        Ok(obj)
    }))));
    
    object.borrow_mut().set("freeze", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        Ok(args.first().cloned().unwrap_or(Value::Undefined))
    }))));
    
    object.borrow_mut().set("isFrozen", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Boolean(false))
    }))));
    
    // Create Object.prototype and attach to Object
    let object_proto = Object::new(ObjectKind::Ordinary);
    let object_proto_rc = Rc::new(RefCell::new(object_proto));
    object.borrow_mut().set("prototype", Value::Object(Rc::clone(&object_proto_rc)));
    
    ctx.set_global("Object".to_string(), Value::Object(object));
}

// ============================================================================
// Array
// ============================================================================

fn register_array(ctx: &mut Context) {
    let array = Object::new(ObjectKind::Ordinary);
    let array = Rc::new(RefCell::new(array));
    
    array.borrow_mut().set("isArray", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(matches!(arg, Value::Object(ref o) if o.borrow().kind == ObjectKind::Array)))
    }))));
    
    array.borrow_mut().set("from", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let items = args.first().cloned().unwrap_or(Value::Undefined);
        let arr = match items {
            Value::Object(o) => {
                let elements: Vec<Value> = o.borrow().elements.clone();
                Object::new_array_from(elements)
            }
            _ => Object::new_array(0),
        };
        Ok(Value::Object(Rc::new(RefCell::new(arr))))
    }))));
    
    array.borrow_mut().set("of", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arr = Object::new_array_from(args.to_vec());
        Ok(Value::Object(Rc::new(RefCell::new(arr))))
    }))));
    
    // Create Array.prototype and attach to Array
    let array_proto = Object::new(ObjectKind::Array);
    let array_proto_rc = Rc::new(RefCell::new(array_proto));
    let get_this_array = || -> Result<Vec<Value>, JsError> {
        match get_native_this() {
            Some(Value::Object(o)) => {
                let arr = o.borrow();
                if arr.kind == ObjectKind::Array {
                    Ok(arr.elements.clone())
                } else {
                    Err(JsError("Array.prototype method called on non-array".to_string()))
                }
            }
            _ => Err(JsError("Array.prototype method called on non-object".to_string())),
        }
    };
    
    // Helper to set the array's elements
    let set_this_elements = |new_elements: Vec<Value>| -> Result<Value, JsError> {
        match get_native_this() {
            Some(Value::Object(o)) => {
                o.borrow_mut().elements = new_elements.clone();
                Ok(Value::Number(new_elements.len() as f64))
            }
            _ => Err(JsError("Array.prototype method called on non-object".to_string())),
        }
    };
    
    // Helper to create result array object
    let make_array = |elements: Vec<Value>| -> Value {
        let arr = Object::new_array_from(elements);
        Value::Object(Rc::new(RefCell::new(arr)))
    };
    
    // length property getter
    array_proto_rc.borrow_mut().set("length", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match get_native_this() {
            Some(Value::Object(o)) => Ok(Value::Number(o.borrow().elements.len() as f64)),
            _ => Ok(Value::Undefined),
        }
    }))));
    
    // Array.prototype.map(callback, thisArg?)
    array_proto_rc.borrow_mut().set("map", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let mut result = Vec::new();
        for (i, elem) in elements.iter().enumerate() {
            let mapped = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    // Call the callback with (element, index, array)
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)
                }
                _ => Err(JsError("Callback is not a function".to_string())),
            }?;
            result.push(mapped);
        }
        Ok(make_array(result))
    }))));
    
    // Array.prototype.filter(callback, thisArg?)
    array_proto_rc.borrow_mut().set("filter", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let mut result = Vec::new();
        for (i, elem) in elements.iter().enumerate() {
            let keep = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let res = call_value_with_this(callback.clone(), callback_args, Value::Undefined)?;
                    to_bool(&res)
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let res = nf.call(callback_args)?;
                    to_bool(&res)
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if keep {
                result.push(elem.clone());
            }
        }
        Ok(make_array(result))
    }))));
    
    // Array.prototype.reduce(callback, initialValue?)
    array_proto_rc.borrow_mut().set("reduce", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let initial = args.get(1).cloned();
        
        let mut accumulator: Value;
        let start_idx: usize;
        
        if let Some(init) = initial {
            accumulator = init;
            start_idx = 0;
        } else if elements.is_empty() {
            return Err(JsError("Reduce of empty array with no initial value".to_string()));
        } else {
            accumulator = elements[0].clone();
            start_idx = 1;
        }
        
        for i in start_idx..elements.len() {
            let elem = &elements[i];
            accumulator = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        accumulator.clone(),
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        accumulator.clone(),
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
        }
        Ok(accumulator)
    }))));
    
    // Array.prototype.forEach(callback, thisArg?)
    array_proto_rc.borrow_mut().set("forEach", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        for (i, elem) in elements.iter().enumerate() {
            match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let _ = call_value_with_this(callback.clone(), callback_args, Value::Undefined);
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let _ = nf.call(callback_args);
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
        }
        Ok(Value::Undefined)
    }))));
    
    // Array.prototype.push(...items)
    array_proto_rc.borrow_mut().set("push", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        elements.extend(args);
        set_this_elements(elements)
    }))));
    
    // Array.prototype.pop()
    array_proto_rc.borrow_mut().set("pop", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let mut elements = get_this_array()?;
        let popped = elements.pop();
        set_this_elements(elements)?;
        Ok(popped.unwrap_or(Value::Undefined))
    }))));
    
    // Array.prototype.shift()
    array_proto_rc.borrow_mut().set("shift", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let mut elements = get_this_array()?;
        let shifted = elements.remove(0);
        set_this_elements(elements)?;
        Ok(shifted)
    }))));
    
    // Array.prototype.unshift(...items)
    array_proto_rc.borrow_mut().set("unshift", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let mut new_items: Vec<Value> = args.to_vec();
        new_items.extend(elements);
        set_this_elements(new_items)
    }))));
    
    // Array.prototype.slice(start?, end?)
    array_proto_rc.borrow_mut().set("slice", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let len = elements.len() as f64;
        let start = args.first().map(to_number).unwrap_or(0.0);
        let end = args.get(1).map(to_number).unwrap_or(len);
        
        let start_idx = if start < 0.0 {
            ((len + start) as isize).max(0).min(len as isize) as usize
        } else {
            (start as usize).min(len as usize)
        };
        let end_idx = if end < 0.0 {
            ((len + end) as isize).max(0).min(len as isize) as usize
        } else {
            (end as usize).min(len as usize)
        };
        
        let result: Vec<Value> = elements[start_idx..end_idx].to_vec();
        Ok(make_array(result))
    }))));
    
    // Array.prototype.concat(...arrays)
    array_proto_rc.borrow_mut().set("concat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        for arg in args {
            match arg {
                Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                    elements.extend(o.borrow().elements.clone());
                }
                _ => elements.push(arg),
            }
        }
        Ok(make_array(elements))
    }))));
    
    // Array.prototype.join(separator?)
    array_proto_rc.borrow_mut().set("join", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let sep = args.first().map(to_js_string).unwrap_or_else(|| ",".to_string());
        let parts: Vec<String> = elements.iter().map(|v| to_js_string(v)).collect();
        Ok(Value::String(parts.join(&sep)))
    }))));
    
    // Array.prototype.indexOf(searchElement, fromIndex?)
    array_proto_rc.borrow_mut().set("indexOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let search = args.first().cloned().unwrap_or(Value::Undefined);
        let from_idx = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
        
        for i in from_idx..elements.len() {
            if crate::value::strict_eq(&elements[i], &search) {
                return Ok(Value::Number(i as f64));
            }
        }
        Ok(Value::Number(-1.0))
    }))));
    
    // Array.prototype.includes(searchElement, fromIndex?)
    array_proto_rc.borrow_mut().set("includes", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let search = args.first().cloned().unwrap_or(Value::Undefined);
        let from_idx = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
        
        for i in from_idx..elements.len() {
            if crate::value::strict_eq(&elements[i], &search) {
                return Ok(Value::Boolean(true));
            }
        }
        Ok(Value::Boolean(false))
    }))));
    
    // Array.prototype.find(predicate, thisArg?)
    array_proto_rc.borrow_mut().set("find", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        
        for (i, elem) in elements.iter().enumerate() {
            let result = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if to_bool(&result) {
                return Ok(elem.clone());
            }
        }
        Ok(Value::Undefined)
    }))));
    
    // Array.prototype.flat(depth?)
    array_proto_rc.borrow_mut().set("flat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let depth = args.first().map(|v| to_number(v) as i32).unwrap_or(1) as i32;
        
        fn flatten(arr: Vec<Value>, depth: i32) -> Vec<Value> {
            if depth <= 0 {
                return arr;
            }
            let mut result = Vec::new();
            for elem in arr {
                match elem {
                    Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                        let inner = o.borrow().elements.clone();
                        result.extend(flatten(inner, depth - 1));
                    }
                    _ => result.push(elem),
                }
            }
            result
        }
        
        Ok(make_array(flatten(elements, depth)))
    }))));
    
    // Array.prototype.some(callback, thisArg?)
    array_proto_rc.borrow_mut().set("some", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        
        for (i, elem) in elements.iter().enumerate() {
            let result = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if to_bool(&result) {
                return Ok(Value::Boolean(true));
            }
        }
        Ok(Value::Boolean(false))
    }))));
    
    // Array.prototype.every(callback, thisArg?)
    array_proto_rc.borrow_mut().set("every", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        
        for (i, elem) in elements.iter().enumerate() {
            let result = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if !to_bool(&result) {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }))));
    
    // Array.prototype.reverse()
    array_proto_rc.borrow_mut().set("reverse", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let mut elements = get_this_array()?;
        elements.reverse();
        set_this_elements(elements.clone())?;
        Ok(make_array(elements))
    }))));
    
    // Array.prototype.sort(compareFn?)
    array_proto_rc.borrow_mut().set("sort", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let _compare_fn = args.first().cloned();
        
        // Simple string comparison sort
        elements.sort_by(|a, b| {
            let a_str = to_js_string(a);
            let b_str = to_js_string(b);
            a_str.cmp(&b_str)
        });
        
        set_this_elements(elements.clone())?;
        Ok(make_array(elements))
    }))));
    
    // Array.prototype.splice(start, deleteCount?, ...items)
    array_proto_rc.borrow_mut().set("splice", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        let start = args.first().map(|v| to_number(v) as isize).unwrap_or(0) as isize;
        let delete_count = args.get(1).map(|v| to_number(v) as usize).unwrap_or(elements.len());
        let items: Vec<Value> = args[2..].to_vec();
        
        let len = elements.len() as isize;
        let mut start_idx = if start < 0 { (len + start).max(0).min(len) as usize } else { (start as usize).min(len as usize) };
        let delete_count = delete_count.min(len as usize - start_idx);
        
        let removed: Vec<Value> = elements.drain(start_idx..start_idx + delete_count).collect();
        
        // Insert new items
        for item in items {
            elements.insert(start_idx, item);
            start_idx += 1;
        }
        
        set_this_elements(elements)?;
        Ok(make_array(removed))
    }))));
    
    // Array.prototype.toString()
    array_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let elements = get_this_array()?;
        let parts: Vec<String> = elements.iter().map(|v| to_js_string(v)).collect();
        Ok(Value::String(parts.join(",")))
    }))));
    
    array.borrow_mut().set("prototype", Value::Object(Rc::clone(&array_proto_rc)));
    
    // Store Array.prototype globally for interpreter to use when creating array literals
    let global_proto = Rc::new(RefCell::new(Object::new(ObjectKind::Array)));
    // Copy all properties from array_proto_rc to global_proto
    let proto_props = array_proto_rc.borrow().properties.clone();
    for (k, v) in proto_props {
        global_proto.borrow_mut().set(&k, v);
    }
    ARRAY_PROTOTYPE.with(|ap| {
        *ap.borrow_mut() = Some(global_proto);
    });
    
    ctx.set_global("Array".to_string(), Value::Object(array));
}

// ============================================================================
// Map and Set
// ============================================================================

fn register_map_and_set(ctx: &mut Context) {
    let map_constructor = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let map_obj = Object::new(ObjectKind::Map);
        let map = Rc::new(RefCell::new(map_obj));
        let entries = Object::new_array(0);
        map.borrow_mut().set("_entries", Value::Object(Rc::new(RefCell::new(entries))));
        Ok(Value::Object(map))
    })));
    ctx.set_global("Map".to_string(), map_constructor);
    
    let set_constructor = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let set_obj = Object::new(ObjectKind::Set);
        Ok(Value::Object(Rc::new(RefCell::new(set_obj))))
    })));
    ctx.set_global("Set".to_string(), set_constructor);
}

// ============================================================================
// Global functions
// ============================================================================

fn register_global_functions(ctx: &mut Context) {
    ctx.register_native("setTimeout", |args| {
        let _callback = args.first().map(to_js_string).unwrap_or_default();
        let _delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    
    ctx.register_native("setInterval", |args| {
        let _callback = args.first().map(to_js_string).unwrap_or_default();
        let _interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    
    ctx.register_native("clearTimeout", |_args| Ok(Value::Undefined));
    ctx.register_native("clearInterval", |_args| Ok(Value::Undefined));
    
    ctx.register_native("parseInt", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let n = s.trim().parse::<i64>().ok().map(|n| n as f64).unwrap_or(f64::NAN);
        Ok(Value::Number(n))
    });
    
    ctx.register_native("parseFloat", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let n = s.trim().parse::<f64>().unwrap_or(f64::NAN);
        Ok(Value::Number(n))
    });
    
    ctx.register_native("isNaN", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    });
    
    ctx.register_native("isFinite", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_finite()))
    });
    
    ctx.register_native("encodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(urlencoding::encode(&s).to_string()))
    });
    
    ctx.register_native("decodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let decoded = urlencoding::decode(&s).map(|d| d.to_string()).unwrap_or(s);
        Ok(Value::String(decoded))
    });
    
    let number_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(to_number).unwrap_or(0.0);
        Ok(Value::Number(n))
    })));
    ctx.set_global("Number".to_string(), number_fn);
    
    let string_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(s))
    })));
    ctx.set_global("String".to_string(), string_fn);
    
    let boolean_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let b = args.first().map(to_bool).unwrap_or(false);
        Ok(Value::Boolean(b))
    })));
    ctx.set_global("Boolean".to_string(), boolean_fn);
    
    // Helper to get current timestamp
    fn chrono_now() -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
    }
    
    // Create Date.prototype first
    let date_proto = Object::new(ObjectKind::Date);
    let date_proto_rc = Rc::new(RefCell::new(date_proto));
    // Add toString to Date.prototype
    date_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let date_str = format!("Date @ {}", chrono_now());
        Ok(Value::String(date_str))
    }))));
    // Add valueOf to Date.prototype
    date_proto_rc.borrow_mut().set("valueOf", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(chrono_now() as f64))
    }))));
    
    // Date constructor - returns a Date object
    let date_proto_clone = Rc::clone(&date_proto_rc);
    let date_fn = NativeFunction::with_prototype(
        move |_args| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64;
            let date_obj = Object::with_prototype(ObjectKind::Date, Rc::clone(&date_proto_clone));
            let date = Rc::new(RefCell::new(date_obj));
            date.borrow_mut().set("_timestamp", Value::Number(now));
            Ok(Value::Object(date))
        },
        date_proto_rc,
    );
    
    ctx.set_global("Date".to_string(), Value::NativeFunction(Rc::new(date_fn)));
}

// ============================================================================
// Error
// ============================================================================

fn register_error(ctx: &mut Context) {
    // Create Error prototype
    let error_proto = Object::new(ObjectKind::Ordinary);
    let error_proto_rc = Rc::new(RefCell::new(error_proto));
    // Add toString to Error.prototype
    error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("Error".to_string()))
    }))));
    
    let error_proto_clone = Rc::clone(&error_proto_rc);
    // Error constructor function with prototype
    let error_constructor = NativeFunction::with_prototype(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&error_proto_rc),
    );
    ctx.set_global("Error".to_string(), Value::NativeFunction(Rc::new(error_constructor)));
    
    // TypeError
    let type_error_proto = Object::new(ObjectKind::Ordinary);
    let type_error_proto_rc = Rc::new(RefCell::new(type_error_proto));
    // Add toString to TypeError.prototype
    type_error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("TypeError".to_string()))
    }))));
    
    let type_error_proto_clone = Rc::clone(&type_error_proto_rc);
    let type_error_constructor = NativeFunction::with_prototype(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&type_error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&type_error_proto_rc),
    );
    ctx.set_global("TypeError".to_string(), Value::NativeFunction(Rc::new(type_error_constructor)));
    
    // ReferenceError
    let ref_error_proto = Object::new(ObjectKind::Ordinary);
    let ref_error_proto_rc = Rc::new(RefCell::new(ref_error_proto));
    // Add toString to ReferenceError.prototype
    ref_error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("ReferenceError".to_string()))
    }))));
    
    let ref_error_proto_clone = Rc::clone(&ref_error_proto_rc);
    let ref_error_constructor = NativeFunction::with_prototype(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&ref_error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&ref_error_proto_rc),
    );
    ctx.set_global("ReferenceError".to_string(), Value::NativeFunction(Rc::new(ref_error_constructor)));
    
    // SyntaxError
    let syntax_error_proto = Object::new(ObjectKind::Ordinary);
    let syntax_error_proto_rc = Rc::new(RefCell::new(syntax_error_proto));
    // Add toString to SyntaxError.prototype
    syntax_error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("SyntaxError".to_string()))
    }))));
    
    let syntax_error_proto_clone = Rc::clone(&syntax_error_proto_rc);
    let syntax_error_constructor = NativeFunction::with_prototype(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&syntax_error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&syntax_error_proto_rc),
    );
    ctx.set_global("SyntaxError".to_string(), Value::NativeFunction(Rc::new(syntax_error_constructor)));
    
    // Link prototype chains: TypeError.prototype -> Error.prototype
    type_error_proto_rc.borrow_mut().set("__proto__", Value::Object(Rc::clone(&error_proto_rc)));
    ref_error_proto_rc.borrow_mut().set("__proto__", Value::Object(Rc::clone(&error_proto_rc)));
    syntax_error_proto_rc.borrow_mut().set("__proto__", Value::Object(Rc::clone(&error_proto_rc)));
}

// ============================================================================
// Function
// ============================================================================

fn register_function(ctx: &mut Context) {
    // Function.prototype - the object that is the prototype of all function objects
    let function_proto = Object::new(ObjectKind::Function);
    let function_proto_rc = Rc::new(RefCell::new(function_proto));
    let function_proto_clone = Rc::clone(&function_proto_rc);
    
    // Function constructor with prototype
    let function_constructor = NativeFunction::with_prototype(
        move |_args| {
            // Function constructor creates a new function from arguments
            // In practice, we just return an empty function
            let func = Object::with_prototype(ObjectKind::Function, Rc::clone(&function_proto_clone));
            let func_rc = Rc::new(RefCell::new(func));
            Ok(Value::Object(func_rc))
        },
        function_proto_rc,
    );
    
    ctx.set_global("Function".to_string(), Value::NativeFunction(Rc::new(function_constructor)));
}

// ============================================================================
// Helpers
// ============================================================================

impl Object {
    fn new_array_from(items: Vec<Value>) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        obj.elements = items.clone();
        obj.properties.insert("length".to_string(), Value::Number(items.len() as f64));
        obj
    }
}

struct JsValueProxy<'a>(&'a Value);

impl serde::Serialize for JsValueProxy<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        match self.0 {
            Value::Undefined => serializer.serialize_unit(),
            Value::Null => serializer.serialize_unit(),
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Number(n) => serializer.serialize_f64(*n),
            Value::String(s) => serializer.serialize_str(s),
            Value::Object(obj_rc) => {
                let obj = obj_rc.borrow();
                
                // Check if it's an array (has numeric indices and length)
                if obj.kind == ObjectKind::Array || !obj.elements.is_empty() {
                    // Serialize as array
                    let mut seq = serializer.serialize_seq(Some(obj.elements.len()))?;
                    for val in &obj.elements {
                        seq.serialize_element(&JsValueProxy(val))?;
                    }
                    seq.end()
                } else {
                    // Serialize as object - collect own properties only
                    let mut map = serializer.serialize_map(Some(obj.properties.len()))?;
                    for (key, val) in &obj.properties {
                        // Skip internal properties
                        if key.starts_with("__") || key == "constructor" || key == "prototype" {
                            continue;
                        }
                        map.serialize_entry(key, &JsValueProxy(val))?;
                    }
                    map.end()
                }
            }
            #[allow(unused_variables)] Value::Function(_) => serializer.serialize_str("[Function]"),
            Value::NativeFunction(_) => serializer.serialize_str("[Function]"),
            Value::Symbol(s) => serializer.serialize_str(&format!("Symbol({})", s)),
        }
    }
}
