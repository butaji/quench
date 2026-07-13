//! Minimal typed-array constructors used by the test262 harness.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{NativeFunction, Object, ObjectKind, Value};

const CONSTRUCTORS: &[(&str, usize)] = &[
    ("Uint8Array", 1),
    ("Int8Array", 1),
    ("Uint16Array", 2),
    ("Int16Array", 2),
    ("Uint32Array", 4),
    ("Int32Array", 4),
    ("Float32Array", 4),
    ("Float64Array", 8),
    ("Uint8ClampedArray", 1),
];

pub fn register_typed_arrays(ctx: &mut Context) {
    for &(name, bytes) in CONSTRUCTORS {
        ctx.set_global(name.to_string(), make_constructor(name, bytes));
    }
}

fn make_constructor(name: &'static str, bytes: usize) -> Value {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let ctor_proto = Rc::clone(&proto);
    let ctor = Rc::new(NativeFunction::new_with_prototype(
        move |_args| construct_typed_array(&ctor_proto),
        Rc::clone(&proto),
    ));
    ctor.set_property("name", Value::String(name.to_string()));
    ctor.set_property("prototype", Value::Object(proto));
    ctor.set_property("BYTES_PER_ELEMENT", Value::Number(bytes as f64));
    Value::NativeFunction(ctor)
}

fn construct_typed_array(proto: &Rc<RefCell<Object>>) -> Result<Value, crate::JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Value::Object(object) = this else {
        return Err(crate::JsError::new("TypeError: typed array requires 'new'"));
    };
    object.borrow_mut().prototype = Some(Rc::clone(proto));
    object.borrow_mut().set("length", Value::Number(0.0));
    Ok(Value::Object(object))
}
