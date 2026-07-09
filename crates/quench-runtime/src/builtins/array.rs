//! Array built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub mod methods;

use methods::setup_prototype_methods;

// Thread-local storage for Array.prototype (used by interpreter for array literal creation)
thread_local! {
    static ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the Array.prototype object (for use by interpreter)
pub fn get_array_prototype() -> Option<Rc<RefCell<Object>>> {
    ARRAY_PROTOTYPE.with(|ap| ap.borrow().clone())
}

// ============================================================================
// Array
// ============================================================================

pub fn register_array(ctx: &mut Context) {
    let array = Object::new(ObjectKind::Ordinary);
    let array = Rc::new(RefCell::new(array));

    register_array_static_methods(&array);
    let array_proto = Object::new(ObjectKind::Array);
    let array_proto_rc = Rc::new(RefCell::new(array_proto));

    setup_prototype_methods(&array_proto_rc);
    setup_array_length_getter(&array_proto_rc);

    array.borrow_mut().set("prototype", Value::Object(Rc::clone(&array_proto_rc)));

    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        array_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    setup_array_prototype_global(&array_proto_rc);

    ctx.set_global("Array".to_string(), Value::Object(array));
}

fn register_array_static_methods(array: &Rc<RefCell<Object>>) {
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
}

fn setup_array_length_getter(array_proto: &Rc<RefCell<Object>>) {
    array_proto.borrow_mut().set("length", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::Object(o)) => Ok(Value::Number(o.borrow().elements.len() as f64)),
            _ => Ok(Value::Undefined),
        }
    }))));
}

fn setup_array_prototype_global(array_proto: &Rc<RefCell<Object>>) {
    let global_proto = Rc::new(RefCell::new(Object::new(ObjectKind::Array)));
    global_proto.borrow_mut().prototype = Some(Rc::clone(array_proto));
    let proto_props = array_proto.borrow().properties.clone();
    for (k, v) in proto_props {
        global_proto.borrow_mut().set(&k, v);
    }
    ARRAY_PROTOTYPE.with(|ap| {
        *ap.borrow_mut() = Some(global_proto);
    });
}
