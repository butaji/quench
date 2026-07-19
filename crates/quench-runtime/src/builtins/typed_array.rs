//! TypedArray constructors (Uint8Array, Int8Array, etc.) for the test262 harness.
//!
//! This implements minimal TypedArray support sufficient for harness files.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::object::{ObjData, TypedArrayName};
use crate::value::{to_number, JsError, NativeFunction, Object, ObjectKind, Value};

const CONSTRUCTORS: &[(&str, usize, TypedArrayName)] = &[
    ("Uint8Array", 1, TypedArrayName::Uint8),
    ("Int8Array", 1, TypedArrayName::Int8),
    ("Uint16Array", 2, TypedArrayName::Uint16),
    ("Int16Array", 2, TypedArrayName::Int16),
    ("Uint32Array", 4, TypedArrayName::Uint32),
    ("Int32Array", 4, TypedArrayName::Int32),
    ("Float32Array", 4, TypedArrayName::Float32),
    ("Float64Array", 8, TypedArrayName::Float64),
    ("Uint8ClampedArray", 1, TypedArrayName::Uint8Clamped),
];

// Thread-local storage for TypedArray.prototype (shared by all TypedArray types)
thread_local! {
    static TYPED_ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the TypedArray.prototype object (for use by other builtins)
pub fn get_typed_array_prototype() -> Option<Rc<RefCell<Object>>> {
    TYPED_ARRAY_PROTOTYPE.with(|tp| tp.borrow().clone())
}

pub fn register_typed_arrays(ctx: &mut Context) {
    // Create shared TypedArray prototype once
    let typed_array_proto = Object::new(ObjectKind::Ordinary);
    let typed_array_proto_rc = Rc::new(RefCell::new(typed_array_proto));

    // Set up prototype properties
    typed_array_proto_rc
        .borrow_mut()
        .set("constructor", Value::Undefined);
    typed_array_proto_rc.borrow_mut().set(
        "Symbol.toStringTag",
        Value::String("TypedArray".to_string()),
    );
    typed_array_proto_rc
        .borrow_mut()
        .set("byteLength", Value::Number(0.0));
    typed_array_proto_rc
        .borrow_mut()
        .set("byteOffset", Value::Number(0.0));
    typed_array_proto_rc
        .borrow_mut()
        .set("length", Value::Number(0.0));

    // Set prototype chain to Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        typed_array_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    // Store prototype for global access
    TYPED_ARRAY_PROTOTYPE.with(|tp| {
        *tp.borrow_mut() = Some(Rc::clone(&typed_array_proto_rc));
    });

    // Register each TypedArray constructor
    for &(name, bytes, typed_array_name) in CONSTRUCTORS {
        let proto_clone = Rc::clone(&typed_array_proto_rc);
        let ctor = make_typed_array_constructor(name, bytes, typed_array_name, proto_clone);
        ctx.set_global(name.to_string(), ctor);
    }
}

fn make_typed_array_constructor(
    name: &str,
    bytes: usize,
    typed_array_name: TypedArrayName,
    shared_proto: Rc<RefCell<Object>>,
) -> Value {
    // Create prototype object for this specific TypedArray type
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("constructor", Value::Undefined);
    proto.set("Symbol.toStringTag", Value::String(name.to_string()));
    proto.set("BYTES_PER_ELEMENT", Value::Number(bytes as f64));
    proto.set("name", Value::String(name.to_string()));
    proto.set("length", Value::Number(0.0));
    proto.set("byteLength", Value::Number(0.0));
    proto.set("byteOffset", Value::Number(0.0));
    proto.prototype = Some(shared_proto);

    let proto_rc = Rc::new(RefCell::new(proto));

    // Create constructor function
    let bytes_owned = bytes;
    let typed_array_name_owned = typed_array_name;
    let proto_for_closure = Rc::clone(&proto_rc);
    let proto_for_constructor = Rc::clone(&proto_rc);

    let ctor_fn = NativeFunction::new_with_prototype(
        move |args| {
            construct_typed_array(
                args,
                bytes_owned,
                typed_array_name_owned,
                &proto_for_closure,
            )
        },
        proto_for_constructor,
    );

    // Wrap in Rc as required by Value::NativeFunction
    let ctor_rc = Rc::new(ctor_fn);
    // Set constructor property on prototype to point to constructor
    proto_rc.borrow_mut().properties.insert(
        "constructor".to_string(),
        Value::NativeFunction(Rc::clone(&ctor_rc)),
    );

    Value::NativeFunction(ctor_rc)
}

fn construct_typed_array(
    args: Vec<Value>,
    bytes_per_element: usize,
    typed_array_name: TypedArrayName,
    proto: &Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Value::Object(object_rc) = this else {
        return Err(crate::JsError::new(
            "TypeError: TypedArray constructor requires 'new'",
        ));
    };

    let mut object = object_rc.borrow_mut();

    // Default values
    let mut length: u64 = 0;
    let mut byte_length: u64 = 0;
    let mut byte_offset: u64 = 0;

    // Create a backing buffer (minimal ArrayBuffer-like object)
    let buffer = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    buffer.borrow_mut().set("byteLength", Value::Number(0.0));

    // Parse arguments
    if !args.is_empty() {
        let arg = &args[0];

        match arg {
            // new TypedArray(length) - create buffer of given length
            Value::Number(n) if *n >= 0.0 && n.is_finite() => {
                length = *n as u64;
                byte_length = length * bytes_per_element as u64;
                buffer
                    .borrow_mut()
                    .set("byteLength", Value::Number(byte_length as f64));
            }
            // new TypedArray(typedArray) or new TypedArray(array-like)
            Value::Object(src_rc) if !Rc::ptr_eq(&src_rc, &object_rc) => {
                let src = src_rc.borrow();
                // Check if it has elements (treat as array-like)
                if !src.elements.is_empty() {
                    length = src.elements.len() as u64;
                    byte_length = length * bytes_per_element as u64;
                    buffer
                        .borrow_mut()
                        .set("byteLength", Value::Number(byte_length as f64));
                } else if let Some(len) = src.get("length") {
                    let len_num = to_number(&len);
                    if len_num >= 0.0 && len_num.is_finite() {
                        length = len_num as u64;
                        byte_length = length * bytes_per_element as u64;
                        buffer
                            .borrow_mut()
                            .set("byteLength", Value::Number(byte_length as f64));
                    }
                }
            }
            _ => {}
        }

        // Handle optional byteOffset argument: new TypedArray(buffer, byteOffset)
        if args.len() > 1 {
            byte_offset = to_number(&args[1]) as u64;
        }
        // Handle optional length argument: new TypedArray(buffer, byteOffset, length)
        if args.len() > 2 {
            let new_length = to_number(&args[2]) as u64;
            length = new_length;
            byte_length = length * bytes_per_element as u64;
            buffer
                .borrow_mut()
                .set("byteLength", Value::Number(byte_length as f64));
        }
    }

    // Set up the object as a TypedArray using ObjData::Idx
    object.data = ObjData::Idx {
        buffer,
        offset: byte_offset,
        length,
        name: typed_array_name,
    };
    object.prototype = Some(Rc::clone(proto));

    // Set standard TypedArray properties
    object.set("length", Value::Number(length as f64));
    object.set("byteLength", Value::Number(byte_length as f64));
    object.set("byteOffset", Value::Number(byte_offset as f64));
    object.set(
        "buffer",
        Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))),
    );

    drop(object);
    Ok(Value::Object(object_rc))
}
