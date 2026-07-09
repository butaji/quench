//! Object built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

thread_local! {
    static OBJECT_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

pub fn get_object_prototype() -> Option<Rc<RefCell<Object>>> {
    OBJECT_PROTOTYPE.with(|op| op.borrow().clone())
}

pub fn register_object(ctx: &mut Context) {
    let object = Object::new(ObjectKind::Ordinary);
    let object = Rc::new(RefCell::new(object));

    register_object_methods(&object);
    register_object_prototype(&object);

    ctx.set_global("Object".to_string(), Value::Object(object));
}

fn register_object_methods(object: &Rc<RefCell<Object>>) {
    object.borrow_mut().set("keys", Value::NativeFunction(Rc::new(NativeFunction::new(object_keys))));
    object.borrow_mut().set("values", Value::NativeFunction(Rc::new(NativeFunction::new(object_values))));
    object.borrow_mut().set("entries", Value::NativeFunction(Rc::new(NativeFunction::new(object_entries))));
    object.borrow_mut().set("assign", Value::NativeFunction(Rc::new(NativeFunction::new(object_assign))));
    object.borrow_mut().set("create", Value::NativeFunction(Rc::new(NativeFunction::new(object_create))));
    object.borrow_mut().set("defineProperty", Value::NativeFunction(Rc::new(NativeFunction::new(object_define_property))));
    object.borrow_mut().set("freeze", Value::NativeFunction(Rc::new(NativeFunction::new(object_freeze))));
    object.borrow_mut().set("isFrozen", Value::NativeFunction(Rc::new(NativeFunction::new(object_is_frozen))));
}

fn object_keys(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().ok_or_else(|| JsError::from("Object.keys requires argument"))?;
    if let Value::Object(o) = obj {
        let keys: Vec<Value> = o.borrow().own_keys().into_iter().map(Value::String).collect();
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array_from(keys)))))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

fn object_values(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().ok_or_else(|| JsError::from("Object.values requires argument"))?;
    if let Value::Object(o) = obj {
        let obj = o.borrow();
        let values: Vec<Value> = obj.own_keys()
            .into_iter()
            .map(|k| obj.get(&k).unwrap_or(Value::Undefined))
            .collect();
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array_from(values)))))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

fn object_entries(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().ok_or_else(|| JsError::from("Object.entries requires argument"))?;
    if let Value::Object(o) = obj {
        let obj = o.borrow();
        let entries: Vec<Value> = obj.own_keys()
            .into_iter()
            .map(|k| Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
                Value::String(k.clone()),
                obj.get(&k).unwrap_or(Value::Undefined)
            ])))))
            .collect();
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array_from(entries)))))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

fn object_assign(args: Vec<Value>) -> Result<Value, JsError> {
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
}

fn object_create(args: Vec<Value>) -> Result<Value, JsError> {
    let proto = args.first().and_then(|v| {
        if let Value::Object(o) = v { Some(Rc::clone(o)) } else { None }
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
}

fn object_define_property(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let prop = args.get(1).map(to_js_string).unwrap_or_default();
    let value = args.get(2).and_then(|v| {
        if let Value::Object(o) = v { o.borrow().properties.get("value").cloned() } else { None }
    }).unwrap_or(Value::Undefined);
    if let Value::Object(o) = &obj {
        o.borrow_mut().set(&prop, value);
    }
    Ok(obj)
}

fn object_freeze(args: Vec<Value>) -> Result<Value, JsError> {
    Ok(args.first().cloned().unwrap_or(Value::Undefined))
}

fn object_is_frozen(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Boolean(false))
}

fn register_object_prototype(object: &Rc<RefCell<Object>>) {
    let object_proto = Object::new(ObjectKind::Ordinary);
    let object_proto_rc = Rc::new(RefCell::new(object_proto));
    object.borrow_mut().set("prototype", Value::Object(Rc::clone(&object_proto_rc)));
    OBJECT_PROTOTYPE.with(|op| {
        *op.borrow_mut() = Some(Rc::clone(&object_proto_rc));
    });
}
