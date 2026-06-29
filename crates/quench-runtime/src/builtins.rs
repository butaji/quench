//! Built-in JavaScript objects and functions

use std::rc::Rc;
use std::cell::RefCell;

use crate::js_runtime::value::{Value, JsError, Object, ObjectKind, NativeFunction, to_js_string, to_number, to_bool};

/// Register built-in globals into the environment
pub fn register_builtins(ctx: &mut crate::js_runtime::Context) {
    // console
    register_console(ctx);
    
    // JSON
    register_json(ctx);
    
    // Math
    register_math(ctx);
    
    // Object
    register_object(ctx);
    
    // Array
    register_array(ctx);
    
    // Map and Set
    register_map_and_set(ctx);
    
    // Date
    register_date(ctx);
    
    // Error
    register_error(ctx);
    
    // Global functions
    register_global_functions(ctx);
}

fn register_console(ctx: &mut crate::js_runtime::Context) {
    let console_obj = Object::new(ObjectKind::Ordinary);
    let console = Rc::new(RefCell::new(console_obj));
    
    // console.log - outputs to stdout
    console.borrow_mut().set("log", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let output = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{}", output);
        Ok(Value::Undefined)
    }))));
    
    // console.error - outputs to stderr
    console.borrow_mut().set("error", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let output = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("{}", output);
        Ok(Value::Undefined)
    }))));
    
    // console.warn - outputs to stderr
    console.borrow_mut().set("warn", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let output = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("{}", output);
        Ok(Value::Undefined)
    }))));
    
    // console.info - outputs to stdout
    console.borrow_mut().set("info", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let output = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{}", output);
        Ok(Value::Undefined)
    }))));
    
    // console.debug - outputs to stderr (debug)
    console.borrow_mut().set("debug", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let output = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[debug] {}", output);
        Ok(Value::Undefined)
    }))));
    
    ctx.set_global("console".to_string(), Value::Object(console));
}

fn register_json(ctx: &mut crate::js_runtime::Context) {
    let json_obj = Object::new(ObjectKind::Ordinary);
    let json = Rc::new(RefCell::new(json_obj));
    
    // JSON.stringify
    let _json_clone = Rc::clone(&json);
    json.borrow_mut().set("stringify", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        let result = serde_json::to_string(&json_value_to_serde(&value)).unwrap_or_default();
        Ok(Value::String(result))
    }))));
    
    // JSON.parse
    json.borrow_mut().set("parse", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) => Ok(serde_to_json_value(v)),
            Err(_) => Err(JsError("JSON.parse error".to_string())),
        }
    }))));
    
    ctx.set_global("JSON".to_string(), Value::Object(json));
}

fn register_math(ctx: &mut crate::js_runtime::Context) {
    let math_obj = Object::new(ObjectKind::Ordinary);
    let math = Rc::new(RefCell::new(math_obj));
    
    // Math constants
    math.borrow_mut().set("PI", Value::Number(std::f64::consts::PI));
    math.borrow_mut().set("E", Value::Number(std::f64::consts::E));
    math.borrow_mut().set("LN2", Value::Number(std::f64::consts::LN_2));
    math.borrow_mut().set("LN10", Value::Number(std::f64::consts::LN_10));
    math.borrow_mut().set("LOG2E", Value::Number(std::f64::consts::LOG2_E));
    math.borrow_mut().set("LOG10E", Value::Number(std::f64::consts::LOG10_E));
    math.borrow_mut().set("SQRT2", Value::Number(std::f64::consts::SQRT_2));
    math.borrow_mut().set("SQRT1_2", Value::Number(std::f64::consts::FRAC_1_SQRT_2));
    
    // Math methods
    math.borrow_mut().set("floor", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.floor()))
    }))));
    
    math.borrow_mut().set("ceil", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.ceil()))
    }))));
    
    math.borrow_mut().set("round", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.round()))
    }))));
    
    math.borrow_mut().set("trunc", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.trunc()))
    }))));
    
    math.borrow_mut().set("abs", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.abs()))
    }))));
    
    math.borrow_mut().set("max", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let max = args.iter()
            .map(|v| to_number(v))
            .fold(f64::NEG_INFINITY, f64::max);
        Ok(Value::Number(max))
    }))));
    
    math.borrow_mut().set("min", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let min = args.iter()
            .map(|v| to_number(v))
            .fold(f64::INFINITY, f64::min);
        Ok(Value::Number(min))
    }))));
    
    math.borrow_mut().set("pow", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let base = args.get(0).map(|v| to_number(v)).unwrap_or(f64::NAN);
        let exp = args.get(1).map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(base.powf(exp)))
    }))));
    
    math.borrow_mut().set("sqrt", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.sqrt()))
    }))));
    
    math.borrow_mut().set("sin", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.sin()))
    }))));
    
    math.borrow_mut().set("cos", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.cos()))
    }))));
    
    math.borrow_mut().set("tan", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.tan()))
    }))));
    
    math.borrow_mut().set("log", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.ln()))
    }))));
    
    math.borrow_mut().set("exp", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(n.exp()))
    }))));
    
    math.borrow_mut().set("random", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
        Ok(Value::Number(rand_simple()))
    }))));
    
    math.borrow_mut().set("sign", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Number(if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 }))
    }))));
    
    ctx.set_global("Math".to_string(), Value::Object(math));
}

fn register_object(ctx: &mut crate::js_runtime::Context) {
    let object_obj = Object::new(ObjectKind::Ordinary);
    let object = Rc::new(RefCell::new(object_obj));
    
    // Object.keys
    object.borrow_mut().set("keys", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().cloned().unwrap_or(Value::Null);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                let keys: Vec<Value> = o.properties.keys()
                    .map(|k| Value::String(k.clone()))
                    .collect();
                Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(keys.len())))))
            }
            Value::String(s) => {
                let keys: Vec<Value> = (0..s.len())
                    .map(|i| Value::String(i.to_string()))
                    .collect();
                Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(keys.len())))))
            }
            _ => Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0))))),
        }
    }))));
    
    // Object.values
    object.borrow_mut().set("values", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().cloned().unwrap_or(Value::Null);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                let values: Vec<Value> = o.properties.values()
                    .cloned()
                    .collect();
                Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(values.len())))))
            }
            _ => Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0))))),
        }
    }))));
    
    // Object.entries
    object.borrow_mut().set("entries", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().cloned().unwrap_or(Value::Null);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                let entries: Vec<Value> = o.properties.iter()
                    .map(|(k, v)| Value::Object(Rc::new(RefCell::new({
                        let mut arr = Object::new_array(2);
                        arr.set("0", Value::String(k.clone()));
                        arr.set("1", v.clone());
                        arr
                    }))))
                    .collect();
                Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(entries.len())))))
            }
            _ => Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0))))),
        }
    }))));
    
    // Object.assign
    object.borrow_mut().set("assign", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = target.clone() {
            for arg in args.iter().skip(1) {
                if let Value::Object(src) = arg {
                    let src = src.borrow();
                    for (k, v) in &src.properties {
                        o.borrow_mut().set(k, v.clone());
                    }
                }
            }
            Ok(target)
        } else {
            Ok(target)
        }
    }))));
    
    // Object.create
    object.borrow_mut().set("create", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))))
    }))));
    
    // Object.hasOwnProperty
    object.borrow_mut().set("hasOwnProperty", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let prop = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            Ok(Value::Boolean(o.properties.contains_key(&prop)))
        } else {
            Ok(Value::Boolean(false))
        }
    }))));
    
    ctx.set_global("Object".to_string(), Value::Object(object));
}

fn register_array(ctx: &mut crate::js_runtime::Context) {
    let array_obj = Object::new(ObjectKind::Ordinary);
    let array = Rc::new(RefCell::new(array_obj));
    
    // Array.isArray
    array.borrow_mut().set("isArray", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(matches!(arg, Value::Object(ref o) if o.borrow().kind == ObjectKind::Array)))
    }))));
    
    // Array.from
    array.borrow_mut().set("from", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arr = args.first().cloned().unwrap_or(Value::Undefined);
        match arr {
            Value::Object(o) => Ok(Value::Object(o)),
            Value::String(s) => {
                let chars: Vec<Value> = s.chars()
                    .map(|c| Value::String(c.to_string()))
                    .collect();
                Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(chars.len())))))
            }
            _ => Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0))))),
        }
    }))));
    
    ctx.set_global("Array".to_string(), Value::Object(Rc::clone(&array)));
    
    // Array.prototype methods
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Array)));
    
    // push
    proto.borrow_mut().set("push", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let mut o = o.borrow_mut();
            let len = o.elements.len();
            for arg in args.iter().skip(1) {
                o.elements.push(arg.clone());
            }
            let new_len = o.elements.len();
            o.properties.insert("length".to_string(), Value::Number(new_len as f64));
            Ok(Value::Number(len as f64))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // pop
    proto.borrow_mut().set("pop", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let mut o = o.borrow_mut();
            let val = o.elements.pop().unwrap_or(Value::Undefined);
            let new_len = o.elements.len();
            o.properties.insert("length".to_string(), Value::Number(new_len as f64));
            Ok(val)
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // shift
    proto.borrow_mut().set("shift", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let mut o = o.borrow_mut();
            if o.elements.is_empty() {
                return Ok(Value::Undefined);
            }
            let val = o.elements.remove(0);
            let new_len = o.elements.len();
            o.properties.insert("length".to_string(), Value::Number(new_len as f64));
            Ok(val)
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // unshift
    proto.borrow_mut().set("unshift", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let mut o = o.borrow_mut();
            let len = o.elements.len();
            for arg in args.iter().skip(1).rev() {
                o.elements.insert(0, arg.clone());
            }
            let new_len = o.elements.len();
            o.properties.insert("length".to_string(), Value::Number(new_len as f64));
            Ok(Value::Number(len as f64))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // map
    proto.borrow_mut().set("map", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            let results: Vec<Value> = o.elements.iter().enumerate()
                .map(|(i, elem)| {
                    call_callback(callback.clone(), vec![elem.clone(), Value::Number(i as f64)])
                })
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_default();
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(results.len())))))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // filter
    proto.borrow_mut().set("filter", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            let mut results = Vec::new();
            for (i, elem) in o.elements.iter().enumerate() {
                let elem_val = elem.clone();
                let idx_val = Value::Number(i as f64);
                let keep = call_callback(callback.clone(), vec![elem_val.clone(), idx_val])
                    .map(|v| to_bool(&v))
                    .unwrap_or(false);
                if keep {
                    results.push(elem_val);
                }
            }
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(results.len())))))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // forEach
    proto.borrow_mut().set("forEach", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            for (i, elem) in o.elements.iter().enumerate() {
                let _ = call_callback(callback.clone(), vec![elem.clone(), Value::Number(i as f64)]);
            }
        }
        Ok(Value::Undefined)
    }))));
    
    // find
    proto.borrow_mut().set("find", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            for (i, elem) in o.elements.iter().enumerate() {
                if call_callback(callback.clone(), vec![elem.clone(), Value::Number(i as f64)])
                    .map(|v| to_bool(&v))
                    .unwrap_or(false)
                {
                    return Ok(elem.clone());
                }
            }
        }
        Ok(Value::Undefined)
    }))));
    
    // indexOf
    proto.borrow_mut().set("indexOf", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let search = args.get(1).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            let idx = o.elements.iter().position(|e| e == &search).map(|i| i as f64).unwrap_or(-1.0);
            Ok(Value::Number(idx))
        } else {
            Ok(Value::Number(-1.0))
        }
    }))));
    
    // includes
    proto.borrow_mut().set("includes", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let search = args.get(1).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            Ok(Value::Boolean(o.elements.contains(&search)))
        } else {
            Ok(Value::Boolean(false))
        }
    }))));
    
    // join
    proto.borrow_mut().set("join", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let sep = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            let parts: Vec<String> = o.elements.iter()
                .map(|v| to_js_string(v))
                .collect();
            Ok(Value::String(parts.join(&sep)))
        } else {
            Ok(Value::String(String::new()))
        }
    }))));
    
    // slice
    proto.borrow_mut().set("slice", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
        let end = args.get(2).map(|v| to_number(v) as usize).unwrap_or(usize::MAX);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            let len = o.elements.len();
            let start = if start > len { len } else { start };
            let end = if end > len { len } else { end };
            let slice: Vec<Value> = o.elements[start..end].to_vec();
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(slice.len())))))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // splice
    proto.borrow_mut().set("splice", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
        let delete_count = args.get(2).map(|v| to_number(v) as usize).unwrap_or(usize::MAX);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let mut o = o.borrow_mut();
            let len = o.elements.len();
            let start = if start > len { len } else { start };
            let _deleted: Vec<Value> = o.elements.splice(start..std::cmp::min(start + delete_count, len), args.iter().skip(3).cloned()).collect();
            let new_len = o.elements.len();
            o.properties.insert("length".to_string(), Value::Number(new_len as f64));
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // flat
    proto.borrow_mut().set("flat", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let depth = args.get(1).map(|v| to_number(v) as usize).unwrap_or(1);
        if let Value::Object(o) = args.first().cloned().unwrap_or(Value::Undefined) {
            let o = o.borrow();
            fn flatten(arr: &[Value], depth: usize) -> Vec<Value> {
                let mut result = Vec::new();
                for v in arr {
                    match v {
                        Value::Object(ref obj) if depth > 0 && obj.borrow().kind == ObjectKind::Array => {
                            result.extend(flatten(&obj.borrow().elements, depth - 1));
                        }
                        _ => result.push(v.clone()),
                    }
                }
                result
            }
            let flat = flatten(&o.elements, depth);
            Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(flat.len())))))
        } else {
            Ok(Value::Undefined)
        }
    }))));
    
    // length getter
    proto.borrow_mut().set("length", Value::Number(0.0));
    
    ctx.set_global("Array_prototype".to_string(), Value::Object(proto));
}

fn register_map_and_set(ctx: &mut crate::js_runtime::Context) {
    // Map methods need access to "this" via thread-local storage
    
    // Create Map prototype using std::sync::Arc for thread safety
    let map_proto = Object::new(ObjectKind::Ordinary);
    let map_proto_arc = std::sync::Arc::new(std::sync::Mutex::new(map_proto));
    
    // Map.set
    map_proto_arc.lock().unwrap().set("set", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let key = args.first().map(|v| to_js_string(v)).unwrap_or_default();
            let value = args.get(1).cloned().unwrap_or(Value::Undefined);
            let mut obj = o.borrow_mut();
            obj.set(&format!("__key_{}", key), value);
            let size = obj.get("size").map(|v| to_number(&v) as i32).unwrap_or(0);
            obj.set("size", Value::Number((size + 1) as f64));
        }
        Ok(Value::Undefined)
    }))));
    
    // Map.get
    map_proto_arc.lock().unwrap().set("get", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let key = args.first().map(|v| to_js_string(v)).unwrap_or_default();
            let obj = o.borrow();
            return Ok(obj.get(&format!("__key_{}", key)).unwrap_or(Value::Undefined));
        }
        Ok(Value::Undefined)
    }))));
    
    // Map.has
    map_proto_arc.lock().unwrap().set("has", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let key = args.first().map(|v| to_js_string(v)).unwrap_or_default();
            let obj = o.borrow();
            return Ok(Value::Boolean(obj.has(&format!("__key_{}", key))));
        }
        Ok(Value::Boolean(false))
    }))));
    
    // Map.delete
    map_proto_arc.lock().unwrap().set("delete", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let key = args.first().map(|v| to_js_string(v)).unwrap_or_default();
            let mut obj = o.borrow_mut();
            let had = obj.has(&format!("__key_{}", key));
            obj.delete(&format!("__key_{}", key));
            if had {
                let size = obj.get("size").map(|v| to_number(&v) as i32).unwrap_or(0);
                obj.set("size", Value::Number((size - 1).max(0) as f64));
            }
            return Ok(Value::Boolean(had));
        }
        Ok(Value::Boolean(false))
    }))));
    
    // Map constructor
    let map_proto_for_ctor = std::sync::Arc::clone(&map_proto_arc);
    ctx.set_global("Map".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
        let mut map_obj = Object::new(ObjectKind::Map);
        map_obj.properties.insert("size".to_string(), Value::Number(0.0));
        // Convert Arc<Mutex<Object>> to Rc<RefCell<Object>> for storage
        let proto_obj = map_proto_for_ctor.lock().unwrap().clone();
        map_obj.prototype = Some(Rc::new(RefCell::new(proto_obj)));
        Ok(Value::Object(Rc::new(RefCell::new(map_obj))))
    }))));
    
    // Create Set prototype using std::sync::Arc for thread safety
    let set_proto = Object::new(ObjectKind::Ordinary);
    let set_proto_arc = std::sync::Arc::new(std::sync::Mutex::new(set_proto));
    
    // Set.add
    set_proto_arc.lock().unwrap().set("add", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let value_str = to_js_string(&value);
            let mut obj = o.borrow_mut();
            if !obj.has(&format!("__elem_{}", value_str)) {
                let count = obj.get("size").map(|v| to_number(&v) as i32).unwrap_or(0);
                obj.set(&format!("__elem_{}", value_str), Value::Boolean(true));
                obj.set("size", Value::Number((count + 1) as f64));
            }
        }
        Ok(Value::Undefined)
    }))));
    
    // Set.has
    set_proto_arc.lock().unwrap().set("has", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let value_str = to_js_string(&value);
            let obj = o.borrow();
            return Ok(Value::Boolean(obj.has(&format!("__elem_{}", value_str))));
        }
        Ok(Value::Boolean(false))
    }))));
    
    // Set.delete
    set_proto_arc.lock().unwrap().set("delete", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let this_val = crate::js_runtime::interpreter::get_native_this();
        if let Some(Value::Object(o)) = this_val {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let value_str = to_js_string(&value);
            let mut obj = o.borrow_mut();
            let had = obj.has(&format!("__elem_{}", value_str));
            obj.delete(&format!("__elem_{}", value_str));
            if had {
                let count = obj.get("size").map(|v| to_number(&v) as i32).unwrap_or(0);
                obj.set("size", Value::Number((count - 1).max(0) as f64));
            }
            return Ok(Value::Boolean(had));
        }
        Ok(Value::Boolean(false))
    }))));
    
    // Set constructor
    let set_proto_for_ctor = std::sync::Arc::clone(&set_proto_arc);
    ctx.set_global("Set".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
        let mut set_obj = Object::new(ObjectKind::Set);
        set_obj.properties.insert("size".to_string(), Value::Number(0.0));
        // Convert Arc<Mutex<Object>> to Rc<RefCell<Object>> for storage
        let proto_obj = set_proto_for_ctor.lock().unwrap().clone();
        set_obj.prototype = Some(Rc::new(RefCell::new(proto_obj)));
        Ok(Value::Object(Rc::new(RefCell::new(set_obj))))
    }))));
}

fn register_date(ctx: &mut crate::js_runtime::Context) {
    let date_obj = Object::new(ObjectKind::Date);
    let date = Rc::new(RefCell::new(date_obj));
    
    // Date.now
    date.borrow_mut().set("now", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
        Ok(Value::Number(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0)))
    }))));
    
    // Date.parse
    date.borrow_mut().set("parse", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
        Ok(Value::Number(0.0))
    }))));
    
    ctx.set_global("Date".to_string(), Value::Object(date));
}

fn register_error(ctx: &mut crate::js_runtime::Context) {
    // Create Error prototype with message and toString
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    proto.borrow_mut().set("message", Value::String(String::new()));
    
    // Error.prototype.toString
    let _proto_for_tostring = Rc::clone(&proto);
    proto.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let this_val = args.first().cloned().unwrap_or(Value::Undefined);
        let name = if let Value::Object(o) = &this_val {
            o.borrow().get("name").map(|v| to_js_string(&v)).unwrap_or_else(|| "Error".to_string())
        } else {
            "Error".to_string()
        };
        let message = if let Value::Object(o) = &this_val {
            o.borrow().get("message").map(|v| to_js_string(&v)).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(Value::String(format!("{}: {}", name, message)))
    }))));
    
    // Error constructor
    let proto_clone = Rc::clone(&proto);
    ctx.set_global("Error".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
        let message = _args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let mut err_obj = Object::new(ObjectKind::Ordinary);
        err_obj.set("message", Value::String(message));
        err_obj.set("name", Value::String("Error".to_string()));
        // Set prototype so instanceof Error works
        let proto_obj = proto_clone.borrow().clone();
        err_obj.prototype = Some(Rc::new(RefCell::new(proto_obj)));
        Ok(Value::Object(Rc::new(RefCell::new(err_obj))))
    }))));
}

fn register_global_functions(ctx: &mut crate::js_runtime::Context) {
    // parseInt
    ctx.set_global("parseInt".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let radix = args.get(1).map(|v| to_number(v) as u32).unwrap_or(10);
        let n = if radix == 10 {
            s.trim().parse::<f64>().unwrap_or(f64::NAN)
        } else {
            // Simple hex parsing for common cases
            if s.trim().starts_with("0x") || s.trim().starts_with("0X") {
                u64::from_str_radix(&s.trim()[2..], 16).map(|n| n as f64).unwrap_or(f64::NAN)
            } else {
                u64::from_str_radix(&s.trim(), radix).map(|n| n as f64).unwrap_or(f64::NAN)
            }
        };
        Ok(Value::Number(n))
    }))));
    
    // parseFloat
    ctx.set_global("parseFloat".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        Ok(Value::Number(s.trim().parse::<f64>().unwrap_or(f64::NAN)))
    }))));
    
    // isNaN
    ctx.set_global("isNaN".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    }))));
    
    // isFinite
    ctx.set_global("isFinite".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Boolean(!n.is_nan() && !n.is_infinite()))
    }))));
    
    // Number
    ctx.set_global("Number".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(0.0);
        Ok(Value::Number(n))
    }))));
    
    // String
    ctx.set_global("String".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        Ok(Value::String(s))
    }))));
    
    // Boolean
    ctx.set_global("Boolean".to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let b = args.first().map(|v| to_bool(v)).unwrap_or(false);
        Ok(Value::Boolean(b))
    }))));
    
    // undefined
    ctx.set_global("undefined".to_string(), Value::Undefined);
    
    // Infinity
    ctx.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
    
    // NaN
    ctx.set_global("NaN".to_string(), Value::Number(f64::NAN));
    
    // globalThis
    ctx.set_global("globalThis".to_string(), Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Global)))));
}

// Helper functions

fn call_callback(callback: Value, args: Vec<Value>) -> Result<Value, JsError> {
    match callback {
        Value::Function(f) => {
            crate::js_runtime::interpreter::call_value(Value::Function(f), args)
        }
        Value::NativeFunction(nf) => nf.call(args),
        _ => Err(JsError("Callback is not a function".to_string())),
    }
}

fn json_value_to_serde(v: &Value) -> serde_json::Value {
    match v {
        Value::Undefined => serde_json::Value::Null,
        Value::Null => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => serde_json::json!(*n),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Object(o) => {
            let o = o.borrow();
            if o.kind == ObjectKind::Array {
                serde_json::Value::Array(o.elements.iter().map(json_value_to_serde).collect())
            } else {
                let mut map = serde_json::Map::new();
                for (k, v) in &o.properties {
                    map.insert(k.clone(), json_value_to_serde(v));
                }
                serde_json::Value::Object(map)
            }
        }
        Value::Function(_) => serde_json::Value::Null,
        Value::NativeFunction(_) => serde_json::Value::Null,
        Value::Symbol(_) => serde_json::Value::Null,
    }
}

fn serde_to_json_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            let elements: Vec<Value> = arr.into_iter().map(serde_to_json_value).collect();
            Value::Object(Rc::new(RefCell::new(Object::new_array(elements.len()))))
        }
        serde_json::Value::Object(map) => {
            let obj = Object::new(ObjectKind::Ordinary);
            let mut obj = obj;
            for (k, v) in map {
                obj.set(&k, serde_to_json_value(v));
            }
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}

// Simple random number generator (not cryptographically secure)
fn rand_simple() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    // Simple LCG
    let seed = nanos.wrapping_mul(1103515245).wrapping_add(12345);
    (seed % (1 << 31)) as f64 / (1 << 30) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_register() {
        let mut ctx = crate::js_runtime::Context::new().unwrap();
        register_builtins(&mut ctx);
        assert!(ctx.get_global("console").is_some());
        assert!(ctx.get_global("JSON").is_some());
        assert!(ctx.get_global("Math").is_some());
    }
}
