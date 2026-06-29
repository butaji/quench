//! Built-in JavaScript objects and functions

use std::rc::Rc;
use std::cell::RefCell;

use crate::value::{Value, JsError, Object, ObjectKind, NativeFunction, to_js_string, to_number, to_bool};
use crate::Context;

/// Register all built-in globals into the context
pub fn register_builtins(ctx: &mut Context) {
    register_console(ctx);
    register_json(ctx);
    register_math(ctx);
    register_object(ctx);
    register_array(ctx);
    register_map_and_set(ctx);
    register_global_functions(ctx);
    register_error(ctx);
}

// ============================================================================
// Console
// ============================================================================

fn register_console(ctx: &mut Context) {
    let console = Object::new(ObjectKind::Ordinary);
    let console = Rc::new(RefCell::new(console));
    
    console.borrow_mut().set("log", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let msg = args.iter().map(|v| to_js_string(v)).collect::<Vec<_>>().join(" ");
        println!("{}", msg);
        Ok(Value::Undefined)
    }))));
    
    console.borrow_mut().set("error", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let msg = args.iter().map(|v| to_js_string(v)).collect::<Vec<_>>().join(" ");
        eprintln!("{}", msg);
        Ok(Value::Undefined)
    }))));
    
    console.borrow_mut().set("warn", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let msg = args.iter().map(|v| to_js_string(v)).collect::<Vec<_>>().join(" ");
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
        let text = args.first().map(|v| to_js_string(v)).unwrap_or_default();
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
                let x = args.first().map(|v| to_number(v)).unwrap_or(0.0);
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
        let base = args.first().map(|v| to_number(v)).unwrap_or(0.0);
        let exp = args.get(1).map(|v| to_number(v)).unwrap_or(1.0);
        Ok(Value::Number(base.powf(exp)))
    }))));
    
    math.borrow_mut().set("max", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let max = args.iter().map(|v| to_number(v)).fold(f64::NEG_INFINITY, f64::max);
        Ok(Value::Number(max))
    }))));
    
    math.borrow_mut().set("min", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let min = args.iter().map(|v| to_number(v)).fold(f64::INFINITY, f64::min);
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
        
        if let Some(props) = args.get(1) {
            if let Value::Object(props_obj) = props {
                for (k, v) in props_obj.borrow().properties.iter() {
                    obj.set(k, v.clone());
                }
            }
        }
        
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    }))));
    
    object.borrow_mut().set("defineProperty", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        let prop = args.get(1).map(|v| to_js_string(v)).unwrap_or_default();
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
        let _callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let _delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    
    ctx.register_native("setInterval", |args| {
        let _callback = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let _interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    
    ctx.register_native("clearTimeout", |_args| Ok(Value::Undefined));
    ctx.register_native("clearInterval", |_args| Ok(Value::Undefined));
    
    ctx.register_native("parseInt", |args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let n = s.trim().parse::<i64>().ok().map(|n| n as f64).unwrap_or(f64::NAN);
        Ok(Value::Number(n))
    });
    
    ctx.register_native("parseFloat", |args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let n = s.trim().parse::<f64>().unwrap_or(f64::NAN);
        Ok(Value::Number(n))
    });
    
    ctx.register_native("isNaN", |args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    });
    
    ctx.register_native("isFinite", |args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_finite()))
    });
    
    ctx.register_native("encodeURIComponent", |args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        Ok(Value::String(urlencoding::encode(&s).to_string()))
    });
    
    ctx.register_native("decodeURIComponent", |args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        let decoded = urlencoding::decode(&s).map(|d| d.to_string()).unwrap_or(s);
        Ok(Value::String(decoded))
    });
    
    let number_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let n = args.first().map(|v| to_number(v)).unwrap_or(0.0);
        Ok(Value::Number(n))
    })));
    ctx.set_global("Number".to_string(), number_fn);
    
    let string_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(|v| to_js_string(v)).unwrap_or_default();
        Ok(Value::String(s))
    })));
    ctx.set_global("String".to_string(), string_fn);
    
    let boolean_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let b = args.first().map(|v| to_bool(v)).unwrap_or(false);
        Ok(Value::Boolean(b))
    })));
    ctx.set_global("Boolean".to_string(), boolean_fn);
    
    let date_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64;
        let date_obj = Object::new(ObjectKind::Date);
        let date = Rc::new(RefCell::new(date_obj));
        date.borrow_mut().set("_timestamp", Value::Number(now));
        Ok(Value::Object(date))
    })));
    ctx.set_global("Date".to_string(), date_fn);
}

// ============================================================================
// Error
// ============================================================================

fn register_error(ctx: &mut Context) {
    let error = Object::new(ObjectKind::Ordinary);
    let error = Rc::new(RefCell::new(error));
    error.borrow_mut().set("message", Value::String(String::new()));
    ctx.set_global("Error".to_string(), Value::Object(error));
    
    let type_error = Object::new(ObjectKind::Ordinary);
    ctx.set_global("TypeError".to_string(), Value::Object(Rc::new(RefCell::new(type_error))));
    
    let ref_error = Object::new(ObjectKind::Ordinary);
    ctx.set_global("ReferenceError".to_string(), Value::Object(Rc::new(RefCell::new(ref_error))));
    
    let syntax_error = Object::new(ObjectKind::Ordinary);
    ctx.set_global("SyntaxError".to_string(), Value::Object(Rc::new(RefCell::new(syntax_error))));
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
            Value::Object(_) => {
                // Simple object serialization - just serialize properties as JSON string
                let s = to_js_string(self.0);
                serializer.serialize_str(&s)
            }
            Value::Function(_) => serializer.serialize_str("[Function]"),
            Value::NativeFunction(_) => serializer.serialize_str("[Function]"),
            Value::Symbol(s) => serializer.serialize_str(&format!("Symbol({})", s)),
        }
    }
}
