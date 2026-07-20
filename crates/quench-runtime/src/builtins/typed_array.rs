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
    // Create shared TypedArray prototype once (shared by all TypedArray instances)
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
    // Register fill method
    typed_array_proto_rc.borrow_mut().set(
        "fill",
        Value::NativeFunction(Rc::new(NativeFunction::new(proto_fill))),
    );

    // Set prototype chain to Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        typed_array_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    // Store prototype for global access
    TYPED_ARRAY_PROTOTYPE.with(|tp| {
        *tp.borrow_mut() = Some(Rc::clone(&typed_array_proto_rc));
    });

    // Create the TypedArray shared prototype function.
    // The test harness does: var TypedArray = Object.getPrototypeOf(Int8Array).
    // This must be a callable function so typeof TypedArray === "function".
    // All TypedArray constructors (Uint8Array, Int8Array, etc.) share this as their prototype.
    let typed_array_ctor = NativeFunction::new(|_args| {
        Err(JsError::new(
            "TypeError: Abstract TypedArray called directly",
        ))
    });
    let mut typed_array_ctor_rc = Rc::new(typed_array_ctor);
    // TypedArray.prototype = typed_array_proto (for instanceof: instance -> typed_array_proto -> Object)
    let _ = typed_array_ctor_rc
        .set_property("prototype", Value::Object(Rc::clone(&typed_array_proto_rc)));
    let _ = typed_array_ctor_rc.set_property("name", Value::String("TypedArray".to_string()));
    // TypedArray's own [[Prototype]] = typed_array_proto
    // (so Object.getPrototypeOf(TypedArray) === TypedArray.prototype)
    Rc::get_mut(&mut typed_array_ctor_rc)
        .unwrap()
        .set_own_prototype(Rc::clone(&typed_array_proto_rc));

    // Register TypedArray as a global (for typeof and Object.getPrototypeOf checks)
    ctx.set_global(
        "TypedArray".to_string(),
        Value::NativeFunction(Rc::clone(&typed_array_ctor_rc)),
    );

    // Register each TypedArray constructor with [[Prototype]] = typed_array_ctor_rc.
    // This makes Object.getPrototypeOf(Uint8Array) === TypedArray work,
    // because Object.getPrototypeOf returns the function's internal prototype.
    for &(name, bytes, typed_array_name) in CONSTRUCTORS {
        let ctor = make_typed_array_constructor(
            name,
            bytes,
            typed_array_name,
            Rc::clone(&typed_array_ctor_rc),
            Rc::clone(&typed_array_proto_rc),
        );
        ctx.set_global(name.to_string(), ctor);
    }
}

fn make_typed_array_constructor(
    name: &str,
    bytes: usize,
    typed_array_name: TypedArrayName,
    typed_array_ctor: Rc<NativeFunction>,
    typed_array_proto: Rc<RefCell<Object>>,
) -> Value {
    // Create prototype object for this specific TypedArray type
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("constructor", Value::Undefined);
    proto.set("Symbol.toStringTag", Value::String(name.to_string()));
    proto.set("BYTES_PER_ELEMENT", Value::Number(bytes as f64));
    proto.set("length", Value::Number(0.0));
    proto.set("byteLength", Value::Number(0.0));
    proto.set("byteOffset", Value::Number(0.0));
    // Per-type prototype's [[Prototype]] = typed_array_proto
    proto.prototype = Some(typed_array_proto);

    let proto_rc = Rc::new(RefCell::new(proto));

    // Create constructor function with [[Prototype]] = typed_array_ctor
    // (so Object.getPrototypeOf(Uint8Array) === TypedArray)
    let bytes_owned = bytes;
    let typed_array_name_owned = typed_array_name;
    let proto_for_closure = Rc::clone(&proto_rc);

    let ctor_fn = NativeFunction::new_with_fn_as_prototype(
        move |args| {
            construct_typed_array(
                args,
                bytes_owned,
                typed_array_name_owned,
                &proto_for_closure,
            )
        },
        typed_array_ctor,
        Rc::clone(&proto_rc),
    );

    // Wrap in Rc as required by Value::NativeFunction
    let ctor_rc = Rc::new(ctor_fn);
    // Set name on the constructor function
    let _ = ctor_rc.set_property("name", Value::String(name.to_string()));
    // Set per-type prototype as the constructor's .prototype property
    let _ = ctor_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    // Set constructor property on per-type prototype to point back to constructor
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
    let mut src_elements: Vec<Value> = Vec::new();
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
            Value::Object(src_rc) if !Rc::ptr_eq(src_rc, &object_rc) => {
                let src = src_rc.borrow();
                // Check if it has elements (treat as array-like)
                if !src.elements.is_empty() {
                    length = src.elements.len() as u64;
                    byte_length = length * bytes_per_element as u64;
                    src_elements = src.elements.clone();
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

    // Populate elements from source array if provided
    object.elements = src_elements;

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

// ─── TypedArray prototype methods ───────────────────────────────────────────────

fn proto_fill(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_this_value().unwrap_or(Value::Undefined);
    let Value::Object(obj_rc) = this else {
        return Err(JsError::new("TypeError: fill called on non-object"));
    };
    let mut obj = obj_rc.borrow_mut();

    // Get fill value
    let fill_val = args.first().cloned().unwrap_or(Value::Undefined);

    // Fill all elements
    let len = obj.elements.len();
    for i in 0..len {
        obj.elements[i] = fill_val.clone();
    }

    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_array_constructor_name() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);

        let ta_names = [
            "Int8Array",
            "Uint8Array",
            "Int16Array",
            "Uint16Array",
            "Int32Array",
            "Uint32Array",
            "Float32Array",
            "Float64Array",
            "Uint8ClampedArray",
        ];
        for name in ta_names {
            let ctor = ctx.get_global(name).expect("constructor should exist");
            let ctor_name = match &ctor {
                Value::NativeFunction(nf) => nf.get_property("name"),
                _ => panic!("{} should be NativeFunction, got {:?}", name, ctor),
            };
            assert_eq!(
                ctor_name,
                Some(Value::String(name.to_string())),
                "TypedArray constructor {} should have name '{}', got {:?}",
                name,
                name,
                ctor_name
            );
        }
    }

    #[test]
    fn typed_array_constructor_is_callable() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);

        for name in ["Int8Array", "Float64Array"] {
            let ctor = ctx.get_global(name).expect("constructor should exist");
            assert!(ctor.is_callable(), "{} should be callable", name);
        }
    }

    #[test]
    fn typed_array_instance_has_fill_method() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);

        // Check what arr.fill resolves to
        let fill_val = ctx.eval("var arr = new Int8Array([0]); typeof arr.fill");
        assert_eq!(
            fill_val.unwrap().to_string(),
            "function",
            "arr.fill should be a function"
        );

        let result = ctx.eval("var arr = new Int8Array([0]); arr.fill(42); arr[0]");
        assert!(
            result.as_ref().is_ok(),
            "TypedArray.fill should work, got: {:?}",
            result
        );
    }

    #[test]
    fn typed_array_global_typed_array_is_function() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);

        let result = ctx.eval("typeof TypedArray");
        let js_result = result.as_ref().map_err(|e| e.to_string());
        assert_eq!(
            js_result.unwrap().to_string(),
            "function",
            "TypedArray should be a function, got: {:?}",
            result
        );
    }

    #[test]
    fn typed_array_global_typed_array_is_abstract_ctor() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);

        // TypedArray should be a function
        let result = ctx.eval("typeof TypedArray === 'function'");
        let js_result = result.as_ref().map_err(|e| e.to_string());
        assert_eq!(
            js_result.unwrap().to_string(),
            "true",
            "typeof TypedArray === 'function' should be true, got: {:?}",
            result
        );
    }

    #[test]
    fn typed_array_global_typed_array_prototype_chain() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);

        // Object.getPrototypeOf(Uint8Array) should be in TypedArray.prototype's chain
        let result = ctx.eval("Object.getPrototypeOf(Uint8Array) !== null");
        let js_result = result.as_ref().map_err(|e| e.to_string());
        assert_eq!(
            js_result.unwrap().to_string(),
            "true",
            "Object.getPrototypeOf(Uint8Array) should not be null, got: {:?}",
            result
        );
    }
}
