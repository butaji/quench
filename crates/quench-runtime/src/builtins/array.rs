//! Array built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
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
    let array_proto = Object::new(ObjectKind::Array);
    let array_proto_rc = Rc::new(RefCell::new(array_proto));

    setup_prototype_methods(&array_proto_rc);
    setup_array_length_getter(&array_proto_rc);

    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        array_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    setup_array_prototype_global(&array_proto_rc);

    // Create a wrapper object that has both the static methods AND the callable constructor
    let array_wrapper = Object::new(ObjectKind::Ordinary);
    let array_wrapper_rc = Rc::new(RefCell::new(array_wrapper));

    // Set up static methods
    array_wrapper_rc.borrow_mut().set("prototype", Value::Object(Rc::clone(&array_proto_rc)));
    array_wrapper_rc.borrow_mut().set("isArray", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(matches!(arg, Value::Object(ref o) if o.borrow().kind == ObjectKind::Array)))
    }))));
    array_wrapper_rc.borrow_mut().set("from", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
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
    array_wrapper_rc.borrow_mut().set("of", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arr = Object::new_array_from(args.to_vec());
        Ok(Value::Object(Rc::new(RefCell::new(arr))))
    }))));

    // Create the native constructor with the prototype
    let array_proto_clone = Rc::clone(&array_proto_rc);
    let array_constructor = NativeConstructor::new(
        move |args: Vec<Value>| {
            let mut arr = if args.is_empty() {
                Object::new_array(0)
            } else if args.len() == 1 {
                if let Value::Number(n) = args[0] {
                    if n == n.floor() && (0.0..4294967296.0).contains(&n) {
                        Object::new_array(n as usize)
                    } else {
                        return Err(JsError("Invalid array length".to_string()));
                    }
                } else {
                    Object::new_array_from(vec![args[0].clone()])
                }
            } else {
                Object::new_array_from(args)
            };
            // Set prototype for instanceof checks
            arr.prototype = Some(Rc::clone(&array_proto_clone));
            Ok(Value::Object(Rc::new(RefCell::new(arr))))
        },
        Rc::clone(&array_proto_rc),
    );

    // Set the constructor property to point to the native constructor
    array_wrapper_rc.borrow_mut().set("constructor", Value::NativeConstructor(Rc::new(array_constructor)));

    // Register the wrapper object as Array
    ctx.set_global("Array".to_string(), Value::Object(array_wrapper_rc));
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
