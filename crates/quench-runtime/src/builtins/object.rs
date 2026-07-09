//! Object built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_bool, JsError, NativeFunction, Object, ObjectKind, PropertyFlags, Value};
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
    object.borrow_mut().set("getOwnPropertyDescriptor", Value::NativeFunction(Rc::new(NativeFunction::new(object_get_own_property_descriptor))));
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

    let desc = args.get(2).ok_or_else(|| JsError::from("Object.defineProperty: descriptor required"))?;

    let mut flags = PropertyFlags::default_data();

    if let Value::Object(desc_obj) = desc {
        let desc_borrowed = desc_obj.borrow();
        if let Some(val) = desc_borrowed.properties.get("value") {
            flags.value = Some(val.clone());
        }
        if let Some(writable) = desc_borrowed.properties.get("writable") {
            flags.writable = to_bool(writable);
        }
        if let Some(enumerable) = desc_borrowed.properties.get("enumerable") {
            flags.enumerable = to_bool(enumerable);
        }
        if let Some(configurable) = desc_borrowed.properties.get("configurable") {
            flags.configurable = to_bool(configurable);
        }
    }

    let value = flags.value.clone().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &obj {
        o.borrow_mut().define(&prop, value, flags);
    }
    Ok(obj)
}

fn object_get_own_property_descriptor(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().ok_or_else(|| JsError::from("Object.getOwnPropertyDescriptor requires argument"))?;
    let prop = args.get(1).map(to_js_string).unwrap_or_default();

    if let Value::Object(o) = obj {
        let obj = o.borrow();

        // Check if property exists
        let has_property = obj.properties.contains_key(&prop)
            || (prop.parse::<usize>().map(|i| i < obj.elements.len()).unwrap_or(false));

        if !has_property {
            return Ok(Value::Undefined);
        }

        // Get value
        let value = obj.get(&prop).unwrap_or(Value::Undefined);

        // Check for existing descriptor flags
        let flags = obj.get_descriptor(&prop).unwrap_or_else(|| {
            PropertyFlags { value: Some(value.clone()), writable: true, enumerable: true, configurable: true }
        });

        // Build descriptor object
        let mut desc = Object::new(ObjectKind::Ordinary);
        desc.properties.insert("value".to_string(), flags.value.unwrap_or(value));
        desc.properties.insert("writable".to_string(), Value::Boolean(flags.writable));
        desc.properties.insert("enumerable".to_string(), Value::Boolean(flags.enumerable));
        desc.properties.insert("configurable".to_string(), Value::Boolean(flags.configurable));

        Ok(Value::Object(Rc::new(RefCell::new(desc))))
    } else {
        Ok(Value::Undefined)
    }
}

fn object_freeze(args: Vec<Value>) -> Result<Value, JsError> {
    Ok(args.first().cloned().unwrap_or(Value::Undefined))
}

fn object_is_frozen(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Boolean(false))
}

fn object_prototype_has_own_property(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().map(to_js_string).unwrap_or_default();
    if let Value::Object(o) = &this_val {
        let obj = o.borrow();
        if obj.properties.contains_key(&key) {
            return Ok(Value::Boolean(true));
        }
        if let Ok(idx) = key.parse::<usize>() {
            if idx < obj.elements.len() {
                return Ok(Value::Boolean(true));
            }
        }
    }
    Ok(Value::Boolean(false))
}

fn object_prototype_is_prototype_of(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Some(Value::Object(v)) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    let mut current = v.borrow().prototype.clone();
    while let Some(proto) = current {
        if Rc::ptr_eq(&proto, match &this_val {
            Value::Object(o) => o,
            _ => return Ok(Value::Boolean(false)),
        }) {
            return Ok(Value::Boolean(true));
        }
        current = proto.borrow().prototype.clone();
    }
    Ok(Value::Boolean(false))
}

fn object_prototype_property_is_enumerable(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().map(to_js_string).unwrap_or_default();
    if let Value::Object(o) = &this_val {
        let obj = o.borrow();
        if obj.properties.contains_key(&key) && key != "length" {
            return Ok(Value::Boolean(true));
        }
        if let Ok(idx) = key.parse::<usize>() {
            if idx < obj.elements.len() && obj.kind == ObjectKind::Array {
                return Ok(Value::Boolean(true));
            }
        }
    }
    Ok(Value::Boolean(false))
}

fn register_object_prototype(object: &Rc<RefCell<Object>>) {
    let object_proto = Object::new(ObjectKind::Ordinary);
    let object_proto_rc = Rc::new(RefCell::new(object_proto));
    object.borrow_mut().set("prototype", Value::Object(Rc::clone(&object_proto_rc)));
    OBJECT_PROTOTYPE.with(|op| {
        *op.borrow_mut() = Some(Rc::clone(&object_proto_rc));
    });

    // Object.prototype.toString
    object_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        Ok(Value::String(to_js_string(&this_val)))
    }))));

    // Object.prototype.valueOf
    object_proto_rc.borrow_mut().set("valueOf", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(crate::builtins::get_native_this().unwrap_or(Value::Undefined))
    }))));

    // Object.prototype.hasOwnProperty
    object_proto_rc.borrow_mut().set("hasOwnProperty", Value::NativeFunction(Rc::new(NativeFunction::new(object_prototype_has_own_property))));

    // Object.prototype.isPrototypeOf
    object_proto_rc.borrow_mut().set("isPrototypeOf", Value::NativeFunction(Rc::new(NativeFunction::new(object_prototype_is_prototype_of))));

    // Object.prototype.propertyIsEnumerable
    object_proto_rc.borrow_mut().set("propertyIsEnumerable", Value::NativeFunction(Rc::new(NativeFunction::new(object_prototype_property_is_enumerable))));
}
