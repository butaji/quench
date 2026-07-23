//! Array built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub mod methods;

use methods::setup_prototype_methods;

/// Maximum length accepted by the Array constructor before it would
/// materialize an unreasonable number of elements (2^20).
const MAX_ARRAY_LENGTH: f64 = 1_048_576.0;

/// Reject array lengths that are too large to materialize with a RangeError.
fn check_array_length(n: f64) -> Result<(), JsError> {
    if n > MAX_ARRAY_LENGTH {
        let (_, js_err) =
            crate::value::error::create_js_error_with_type("Invalid array length", "RangeError");
        return Err(js_err);
    }
    Ok(())
}

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

    // Build the NativeConstructor first so we can reference it for both
    // the global binding and the prototype.constructor property.
    let array_proto_for_ctor = Rc::clone(&array_proto_rc);
    let array_constructor = NativeConstructor::new(
        move |args: Vec<Value>| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                return make_array_with_new(obj_rc, &args, &array_proto_for_ctor);
            }
            make_array_direct(&args, &array_proto_for_ctor)
        },
        Rc::clone(&array_proto_rc),
    );
    array_constructor.set_name("Array");

    // Set static methods on the constructor
    array_constructor.set_static_method(
        "isArray",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Value::Boolean(
                matches!(arg, Value::Object(ref o) if o.borrow().kind == ObjectKind::Array),
            ))
        }))),
    );
    array_constructor.set_static_method(
        "from",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let items = args.first().cloned().unwrap_or(Value::Undefined);
            let arr = match items {
                Value::Object(o) => {
                    let elements: Vec<Value> = o.borrow().elements.clone();
                    Object::new_array_from(elements)
                }
                _ => Object::new_array(0),
            };
            Ok(Value::Object(Rc::new(RefCell::new(arr))))
        }))),
    );
    array_constructor.set_static_method(
        "of",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let arr = Object::new_array_from(args.to_vec());
            Ok(Value::Object(Rc::new(RefCell::new(arr))))
        }))),
    );

    // Set Array.prototype.constructor = the NativeConstructor (non-enumerable)
    let array_constructor_rc = Rc::new(array_constructor.clone());
    array_proto_rc.borrow_mut().set_builtin_method(
        "constructor",
        Value::NativeConstructor(Rc::clone(&array_constructor_rc)),
    );

    // Register Array as the NativeConstructor (typeof returns "function")
    ctx.set_global(
        "Array".to_string(),
        Value::NativeConstructor(array_constructor_rc),
    );
}

fn make_array_with_new(
    obj_rc: Rc<RefCell<Object>>,
    args: &[Value],
    proto: &Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    obj_rc.borrow_mut().prototype = Some(Rc::clone(proto));
    obj_rc.borrow_mut().kind = ObjectKind::Array;
    if args.is_empty() {
        obj_rc.borrow_mut().elements = vec![];
        obj_rc.borrow_mut().set("length", Value::Number(0.0));
    } else if args.len() == 1 {
        if let Value::Number(n) = args[0] {
            if n == n.floor() && (0.0..4294967296.0).contains(&n) {
                check_array_length(n)?;
                obj.elements = vec![Value::Undefined; n as usize];
                obj.define_array_length(n);
            } else {
                return Err(JsError("Invalid array length".to_string()));
            }
        } else {
            obj.elements = vec![args[0].clone()];
            obj.define_array_length(1.0);
        }
    } else if args.len() > 1 {
        obj.elements = args.to_vec();
        obj.define_array_length(args.len() as f64);
    }
    drop(obj);
    Ok(Value::Object(obj_rc))
}

fn make_array_direct(args: &[Value], proto: &Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let mut arr = if args.is_empty() {
        Object::new_array(0)
    } else if args.len() == 1 {
        if let Value::Number(n) = args[0] {
            if n == n.floor() && (0.0..4294967296.0).contains(&n) {
                check_array_length(n)?;
                Object::new_array(n as usize)
            } else {
                return Err(JsError("Invalid array length".to_string()));
            }
        } else {
            Object::new_array_from(vec![args[0].clone()])
        }
    } else {
        Object::new_array_from(args.to_vec())
    };
    arr.prototype = Some(Rc::clone(proto));
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

fn setup_array_length_getter(array_proto: &Rc<RefCell<Object>>) {
    array_proto.borrow_mut().set(
        "length",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
            match crate::builtins::get_native_this() {
                Some(Value::Object(o)) => Ok(Value::Number(o.borrow().elements.len() as f64)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

fn setup_array_prototype_global(array_proto: &Rc<RefCell<Object>>) {
    ARRAY_PROTOTYPE.with(|ap| {
        *ap.borrow_mut() = Some(Rc::clone(array_proto));
    });
}

/// Wire `Array.prototype[Symbol.iterator]` after `Symbol` is registered.
pub fn register_array_iterator() {
    let Some(array_proto) = get_array_prototype() else {
        return;
    };
    let Some(Value::Symbol(sym)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    else {
        return;
    };
    let key = sym.property_key();
    let iter_fn = NativeFunction::new(array_values_iterator);
    array_proto
        .borrow_mut()
        .set_builtin_method(&key, Value::NativeFunction(Rc::new(iter_fn)));
}

fn array_values_iterator(_args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Value::Object(arr_rc) = this_val else {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Array.prototype.values called on incompatible receiver",
            "TypeError",
        );
        return Err(js_err);
    };
    let elements = arr_rc.borrow().elements.clone();
    Ok(crate::builtins::map::helpers::make_iterator(elements))
}
