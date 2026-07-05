// linter-skip
#![allow(clippy::too_many_lines, clippy::function_body_length, clippy::complexity)]
//! Object built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// Thread-local storage for Object.prototype (used by other builtins for prototype chains)
thread_local! {
    static OBJECT_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the Object.prototype object (for use by other builtins)
pub fn get_object_prototype() -> Option<Rc<RefCell<Object>>> {
    OBJECT_PROTOTYPE.with(|op| op.borrow().clone())
}

// ============================================================================
// Object
// ============================================================================

pub fn register_object(ctx: &mut Context) {
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
    
    // Store Object.prototype globally for other builtins to use
    OBJECT_PROTOTYPE.with(|op| {
        *op.borrow_mut() = Some(Rc::clone(&object_proto_rc));
    });

    ctx.set_global("Object".to_string(), Value::Object(object));
}
