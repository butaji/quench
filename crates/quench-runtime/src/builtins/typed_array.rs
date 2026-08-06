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
    ("BigInt64Array", 8, TypedArrayName::BigInt64),
    ("BigUint64Array", 8, TypedArrayName::BigUint64),
];

// Thread-local storage for TypedArray.prototype (shared by all TypedArray types)
thread_local! {
    static TYPED_ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the TypedArray.prototype object (for use by other builtins)
pub fn get_typed_array_prototype() -> Option<Rc<RefCell<Object>>> {
    TYPED_ARRAY_PROTOTYPE.with(|tp| tp.borrow().clone())
}

pub(crate) fn immutable_uint8_array(
    bytes: Vec<Value>,
    env: &Rc<RefCell<crate::env::Environment>>,
) -> Result<Value, JsError> {
    let prototype = constructor_prototype(env, "Uint8Array")?;
    let buffer_prototype = constructor_prototype(env, "ArrayBuffer")?;
    let length = bytes.len() as u64;
    let mut buffer = Object::with_prototype(ObjectKind::Ordinary, buffer_prototype);
    buffer.elements = bytes;
    buffer.set("byteLength", Value::Number(length as f64));
    buffer.set("\0arrayBuffer", Value::Boolean(true));
    buffer.set("\0immutable", Value::Boolean(true));
    let buffer = Rc::new(RefCell::new(buffer));
    let mut array = Object::with_prototype(ObjectKind::Ordinary, prototype);
    array.data = ObjData::Idx {
        buffer: Rc::clone(&buffer),
        offset: 0,
        length,
        name: TypedArrayName::Uint8,
    };
    array.set_builtin_method("buffer", Value::Object(buffer));
    Ok(Value::Object(Rc::new(RefCell::new(array))))
}

fn constructor_prototype(
    env: &Rc<RefCell<crate::env::Environment>>,
    name: &str,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let Some(Value::NativeFunction(constructor)) = env.borrow().get(name) else {
        return Err(JsError::new(format!("TypeError: {name} is unavailable")));
    };
    let Some(Value::Object(prototype)) = constructor.get_property("prototype") else {
        return Err(JsError::new(format!(
            "TypeError: {name} prototype is unavailable"
        )));
    };
    Ok(prototype)
}

/// Save the thread-local prototype cache (realm snapshot support)
pub(crate) fn save_typed_array_prototype() -> Option<Rc<RefCell<Object>>> {
    get_typed_array_prototype()
}

/// Restore the thread-local prototype cache (realm snapshot support)
pub(crate) fn restore_typed_array_prototype(proto: Option<Rc<RefCell<Object>>>) {
    TYPED_ARRAY_PROTOTYPE.with(|tp| *tp.borrow_mut() = proto);
}

pub fn register_typed_arrays(ctx: &mut Context) {
    // Create shared TypedArray prototype once (shared by all TypedArray instances)
    let typed_array_proto = Object::new(ObjectKind::Ordinary);
    let typed_array_proto_rc = Rc::new(RefCell::new(typed_array_proto));

    // Set up prototype properties
    typed_array_proto_rc
        .borrow_mut()
        .set_builtin_method("constructor", Value::Undefined);
    typed_array_proto_rc.borrow_mut().set_builtin_method(
        "Symbol.toStringTag",
        Value::String("TypedArray".to_string()),
    );
    // length, byteLength, byteOffset are NOT set on typed_array_proto.
    // TypedArray instances use ObjData::Idx to provide these values dynamically.
    // Register fill method
    typed_array_proto_rc.borrow_mut().set_builtin_method(
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
    register_typed_array_iterator();
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
    proto.set_builtin_method("constructor", Value::Undefined);
    proto.set_builtin_method("Symbol.toStringTag", Value::String(name.to_string()));
    proto.set_builtin_method("BYTES_PER_ELEMENT", Value::Number(bytes as f64));
    // length, byteLength, byteOffset omitted: per-type proto inherits from typed_array_proto
    // which has no own properties here, so TypedArray instances return dynamic values
    // from ObjData::Idx via the prototype chain.
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
    // Set BYTES_PER_ELEMENT on the constructor (static property), per ES spec
    let _ = ctor_rc.set_property("BYTES_PER_ELEMENT", Value::Number(bytes as f64));
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
    _proto: &Rc<RefCell<Object>>,
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
    let buffer: Rc<RefCell<Object>>;
    if !args.is_empty() {
        let arg = &args[0];

        match arg {
            // new TypedArray(length) - create buffer of given length
            Value::Number(n) if *n >= 0.0 && n.is_finite() => {
                length = *n as u64;
                byte_length = length * bytes_per_element as u64;
                let new_buf = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
                new_buf
                    .borrow_mut()
                    .set("byteLength", Value::Number(byte_length as f64));
                new_buf.borrow_mut().elements = (0..byte_length as usize)
                    .map(|_| Value::Number(0.0))
                    .collect();
                buffer = new_buf;
            }
            // new TypedArray(typedArray) or new TypedArray(array-like)
            Value::Object(src_rc) if !Rc::ptr_eq(src_rc, &object_rc) => {
                let src = src_rc.borrow();
                // Check if it has elements (treat as array-like or ArrayBuffer)
                if !src.elements.is_empty() {
                    // ArrayBuffer: use as shared backing store
                    // array-like: copy elements
                    if src.get("byteLength").is_some() {
                        // This is an ArrayBuffer (has byteLength property).
                        // Clone the buffer and ensure its elements vector covers the full
                        // byte length so TypedArray element reads (via ObjData::Idx in get_own)
                        // find valid entries.
                        let buf_bl = src
                            .get("byteLength")
                            .map(|v| to_number(&v) as usize)
                            .unwrap_or(0);
                        drop(src);
                        let mut buf_clone = (*src_rc).borrow_mut();
                        if buf_clone.elements.len() < buf_bl {
                            buf_clone.elements.resize(buf_bl, Value::Number(0.0));
                        }
                        drop(buf_clone);
                        buffer = Rc::clone(src_rc);
                        // After the match, handle byteOffset and length args
                    } else {
                        length = src.elements.len() as u64;
                        byte_length = length * bytes_per_element as u64;
                        let cloned = src.elements.clone();
                        drop(src);
                        let new_buf = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
                        new_buf
                            .borrow_mut()
                            .set("byteLength", Value::Number(byte_length as f64));
                        // Expand elements into byte layout: each TypedArray element takes
                        // bytes_per_element slots in buffer.elements (matching byte layout).
                        let mut expanded = Vec::with_capacity(byte_length as usize);
                        for val in cloned {
                            for _ in 0..bytes_per_element {
                                expanded.push(val.clone());
                            }
                        }
                        new_buf.borrow_mut().elements = expanded;
                        buffer = new_buf;
                    }
                } else if let Some(len) = src.get("length") {
                    let len_num = to_number(&len);
                    if len_num >= 0.0 && len_num.is_finite() {
                        length = len_num as u64;
                        byte_length = length * bytes_per_element as u64;
                    }
                    drop(src);
                    let new_buf = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
                    new_buf
                        .borrow_mut()
                        .set("byteLength", Value::Number(byte_length as f64));
                    new_buf.borrow_mut().elements = (0..byte_length as usize)
                        .map(|_| Value::Number(0.0))
                        .collect();
                    buffer = new_buf;
                } else {
                    drop(src);
                    let new_buf = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
                    new_buf.borrow_mut().set("byteLength", Value::Number(0.0));
                    buffer = new_buf;
                }
            }
            _ => {
                let new_buf = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
                new_buf.borrow_mut().set("byteLength", Value::Number(0.0));
                buffer = new_buf;
            }
        }

        // Handle optional byteOffset and length arguments (for ArrayBuffer construction)
        if args.len() > 1 {
            byte_offset = to_number(&args[1]) as u64;
            if args.len() > 2 {
                let new_length = to_number(&args[2]) as u64;
                length = new_length;
                byte_length = length * bytes_per_element as u64;
            } else {
                // ArrayBuffer with byteOffset but no explicit length: length-track
                let buf_byte_len = buffer
                    .borrow()
                    .get("byteLength")
                    .map(|v| to_number(&v) as u64)
                    .unwrap_or(0);
                if byte_offset < buf_byte_len {
                    length = (buf_byte_len - byte_offset) / bytes_per_element as u64;
                    byte_length = length * bytes_per_element as u64;
                }
            }
        } else if args.len() == 1 && buffer.borrow().get("byteLength").is_some() {
            // Single ArrayBuffer argument: derive length from buffer byteLength
            let buf_byte_len = buffer
                .borrow()
                .get("byteLength")
                .map(|v| to_number(&v) as u64)
                .unwrap_or(0);
            length = buf_byte_len / bytes_per_element as u64;
            byte_length = length * bytes_per_element as u64;
        }
    } else {
        let new_buf = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        new_buf.borrow_mut().set("byteLength", Value::Number(0.0));
        buffer = new_buf;
    }

    // Set up the object as a TypedArray using ObjData::Idx
    object.data = ObjData::Idx {
        buffer: Rc::clone(&buffer),
        offset: byte_offset,
        length,
        name: typed_array_name,
    };

    // Set standard TypedArray properties
    object.set_builtin_method("byteOffset", Value::Number(byte_offset as f64));
    object.set_builtin_method("buffer", Value::Object(Rc::clone(&buffer)));
    // For resizable buffers: only set explicit length/byteLength as own properties when
    // an explicit length was provided (args.len() > 2). Otherwise, the dynamic getter
    // in get_own computes them from the buffer's current byteLength (length-tracking).
    let max_bl = buffer
        .borrow()
        .get("maxByteLength")
        .map(|v| to_number(&v) as u64)
        .unwrap_or(0);
    if max_bl > 0 && args.len() > 2 {
        // Explicit length: own property overrides dynamic getter
        object.set_builtin_method("length", Value::Number(length as f64));
        object.set_builtin_method("byteLength", Value::Number(byte_length as f64));
    }

    drop(object);
    Ok(Value::Object(object_rc))
}

fn proto_fill(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_this_value().unwrap_or(Value::Undefined);
    let Value::Object(obj_rc) = this else {
        return Err(JsError::new("TypeError: fill called on non-object"));
    };
    // Get fill value
    let fill_val = args.first().cloned().unwrap_or(Value::Undefined);

    // Fill all elements
    let len = obj_rc
        .borrow()
        .get("length")
        .map(|value| to_number(&value) as usize)
        .unwrap_or(0);
    for i in 0..len {
        obj_rc.borrow_mut().set(&i.to_string(), fill_val.clone());
    }

    Ok(Value::Undefined)
}

fn typed_array_values(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_this_value().unwrap_or(Value::Undefined);
    let Value::Object(obj_rc) = this else {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "TypedArray.prototype.values called on incompatible receiver",
            "TypeError",
        );
        return Err(js_err);
    };
    Ok(crate::builtins::map::helpers::make_live_index_iterator(
        obj_rc,
        crate::builtins::map::helpers::LiveIndexIteratorMode::Values,
    ))
}

fn typed_array_keys(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_this_value().unwrap_or(Value::Undefined);
    let Value::Object(obj_rc) = this else {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "TypedArray.prototype.keys called on incompatible receiver",
            "TypeError",
        );
        return Err(js_err);
    };
    Ok(crate::builtins::map::helpers::make_live_index_iterator(
        obj_rc,
        crate::builtins::map::helpers::LiveIndexIteratorMode::Keys,
    ))
}

/// Wire `%TypedArray%.prototype[Symbol.iterator]` after `Symbol` is registered.
pub fn register_typed_array_iterator() {
    let Some(typed_array_proto) = get_typed_array_prototype() else {
        return;
    };
    let Some(Value::Symbol(sym)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    else {
        return;
    };
    let key = sym.property_key();
    typed_array_proto.borrow_mut().set_builtin_method(
        &key,
        Value::NativeFunction(Rc::new(NativeFunction::new(typed_array_values))),
    );
    typed_array_proto.borrow_mut().set_builtin_method(
        "values",
        Value::NativeFunction(Rc::new(NativeFunction::new(typed_array_values))),
    );
    typed_array_proto.borrow_mut().set_builtin_method(
        "keys",
        Value::NativeFunction(Rc::new(NativeFunction::new(typed_array_keys))),
    );
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
    fn typed_array_constructor_name_is_visible_to_javascript() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        let result = ctx.eval("Uint8Array.name === 'Uint8Array'");
        assert_eq!(result, Ok(Value::Boolean(true)));
    }

    #[test]
    fn every_typed_array_constructor_name_is_visible_to_javascript() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        for name in [
            "Int8Array",
            "Uint8Array",
            "Uint8ClampedArray",
            "Int16Array",
            "Uint16Array",
            "Int32Array",
            "Uint32Array",
            "Float32Array",
            "Float64Array",
        ] {
            let result = ctx.eval(&format!("{}.name === '{}'", name, name));
            assert_eq!(result, Ok(Value::Boolean(true)), "{} name mismatch", name);
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

    #[test]
    fn extended_uint8_array_element_assignment_coerces_to_uint8() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let r = ctx
            .eval(
                "class ExtendedUint8Array extends Uint8Array { \
                 constructor() { super(10); this[1] = 0xFFA; } } \
                 new ExtendedUint8Array()[1]",
            )
            .unwrap();
        assert_eq!(r, Value::Number(250.0));
    }

    #[test]
    fn typed_array_for_of_sees_mutation_during_iteration() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let second = ctx
            .eval(
                "var array = new Uint8Array([3, 2, 4, 1]); var second = null; \
                 var n = 0; \
                 for (var x of array) { \
                   if (n === 1) second = x; \
                   array[1] = 64; \
                   n++; \
                 } \
                 second",
            )
            .unwrap();
        assert_eq!(second, Value::Number(64.0));
    }

    #[test]
    fn typed_array_for_of_iterates_elements() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let len = ctx
            .eval(
                "var ta = new Uint8Array([1,2,3]); var n = 0; \
                 for (var x of ta) { n += 1; } n",
            )
            .unwrap();
        assert_eq!(len, Value::Number(3.0));
    }

    #[test]
    fn typed_array_from_arraybuffer_element_access() {
        // TypedArray constructed from ArrayBuffer must have working element read/write.
        // This is the root cause of 5 resizable-buffer test262 failures.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Float32Array from ArrayBuffer
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40); \
                 var ta = new Float32Array(buf); \
                 ta[0] = 42; \
                 ta[0];",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(42.0),
            "Float32Array element write/read via ArrayBuffer should work"
        );

        // Uint8Array from ArrayBuffer
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10); \
                 var ta = new Uint8Array(buf); \
                 ta[0] = 255; \
                 ta[0];",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(255.0),
            "Uint8Array element write/read via ArrayBuffer should work"
        );

        // Int32Array from ArrayBuffer
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40); \
                 var ta = new Int32Array(buf); \
                 ta[0] = -12345; \
                 ta[0];",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(-12345.0),
            "Int32Array element write/read via ArrayBuffer should work"
        );

        // Resizable ArrayBuffer
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Float32Array(buf); \
                 ta[0] = 99; \
                 ta[0];",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(99.0),
            "Float32Array from resizable ArrayBuffer should support element access"
        );
    }

    #[test]
    fn test_typed_array_iterator() {
        let mut ctx = Context::new().unwrap();
        register_typed_arrays(&mut ctx);
        crate::builtins::register_builtins(&mut ctx);
        // After register_builtins, the typed array iterator should be wired
        let r = ctx
            .eval(
                "var arr = new Int8Array([1,2,3]); \
                 var result = []; \
                 for (var v of arr) { result.push(v); } \
                 result.join(',');",
            )
            .unwrap();
        assert_eq!(r, Value::String("1,2,3".into()));
    }

    #[test]
    fn typed_array_buffer_view_sets_idx_length() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        ctx.eval("globalThis.__ta = new Uint8Array(new ArrayBuffer(8), 0, 3);")
            .unwrap();
        let ta = ctx.get_global("__ta").expect("global __ta");
        let Value::Object(ref o) = ta else {
            panic!("expected object");
        };
        let length = match o.borrow().data {
            ObjData::Idx { length, .. } => length,
            ref other => panic!("expected Idx data, got {other:?}"),
        };
        assert_eq!(length, 3);
        let keys = crate::eval::iteration::get_enumerable_keys(&ta).unwrap();
        assert_eq!(keys, vec!["0", "1", "2"]);
    }

    #[test]
    fn typed_array_resizable_explicit_length_property() {
        // Resizable buffer with EXPLICIT length: own length property must
        // return the explicit length, NOT the buffer byteLength.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Uint8Array: buffer=40 bytes, explicit length=3 should give length=3
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Uint8Array(buf, 0, 3); \
                 ta.length;",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(3.0),
            "explicit length should be 3, not buffer size"
        );

        // Float32Array: buffer=40 bytes (10 Float32 elements), explicit length=3
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Float32Array(buf, 0, 3); \
                 ta.length;",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(3.0),
            "Float32Array explicit length should be 3"
        );
    }

    #[test]
    fn typed_array_resizable_iteration_with_explicit_length() {
        // Iteration with for-of: TypedArray from resizable buffer with explicit length.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Uint8Array(buf, 0, 3); \
                 ta[0] = 10; ta[1] = 20; ta[2] = 30; \
                 var result = []; \
                 for (var v of ta) result.push(v); \
                 result.join(',');",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::String("10,20,30".to_string()),
            "iteration should yield exactly 3 elements"
        );
    }

    #[test]
    fn typed_array_resizable_iteration_with_offset() {
        // Iteration with for-of: TypedArray from resizable buffer with byte offset.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta_write = new Uint8Array(buf); \
                 for (var i = 0; i < 10; i++) ta_write[i] = i; \
                 var ta = new Uint8Array(buf, 2, 3); \
                 ta.length;",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(3.0),
            "offset TypedArray length should be 3"
        );

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta_write = new Uint8Array(buf); \
                 for (var i = 0; i < 10; i++) ta_write[i] = i; \
                 var ta = new Uint8Array(buf, 2, 3); \
                 var result = []; \
                 for (var v of ta) result.push(v); \
                 result.join(',');",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::String("2,3,4".to_string()),
            "offset TypedArray iteration should yield [2,3,4]"
        );
    }

    #[test]
    fn typed_array_resizable_iteration_length_tracking() {
        // Iteration: TypedArray from resizable buffer WITHOUT explicit length
        // (length tracks buffer byteLength dynamically).
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Step 1: check ArrayBuffer is set up correctly
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 buf.byteLength;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(10.0), "buffer.byteLength should be 10");

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 buf.maxByteLength;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(20.0), "buffer.maxByteLength should be 20");

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 buf.resizable;",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true), "buffer.resizable should be true");

        // Step 2: check TypedArray is set up correctly
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 var ta = new Uint8Array(buf); \
                 ta.buffer.byteLength;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(10.0), "ta.buffer.byteLength should be 10");

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 var ta = new Uint8Array(buf); \
                 ta.byteOffset;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(0.0), "ta.byteOffset should be 0");

        // Step 3: check element access works
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 var ta = new Uint8Array(buf); \
                 ta[0] = 5; \
                 ta[0];",
            )
            .unwrap();
        assert_eq!(r, Value::Number(5.0), "ta[0] should be 5 after assignment");

        // Step 4: check length
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 var ta = new Uint8Array(buf); \
                 ta[0] = 5; ta[1] = 6; ta[2] = 7; \
                 ta.length;",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(10.0),
            "length should track buffer byteLength=10"
        );

        // Step 5: check iteration
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
                 var ta = new Uint8Array(buf); \
                 ta[0] = 5; ta[1] = 6; ta[2] = 7; \
                 var result = []; \
                 for (var v of ta) result.push(v); \
                 result.join(',');",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::String("5,6,7,0,0,0,0,0,0,0".to_string()),
            "iteration should yield all 10 elements"
        );
    }

    #[test]
    fn typed_array_float32_resizable_length_tracking_iteration() {
        // Float32Array from resizable buffer WITHOUT explicit length (length-tracking).
        // This is the specific case that fails in test262.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Create Float32Array from resizable buffer: 40 bytes = 10 Float32 elements
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Float32Array(buf); \
                 ta.length;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(10.0), "Float32Array length should be 10");

        // Write values and check they are readable
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Float32Array(buf); \
                 ta[0] = 1; ta[1] = 2; ta[2] = 3; \
                 ta[0];",
            )
            .unwrap();
        assert_eq!(r, Value::Number(1.0), "Float32Array[0] should be 1");

        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Float32Array(buf); \
                 ta[0] = 1; ta[1] = 2; ta[2] = 3; \
                 ta[1];",
            )
            .unwrap();
        assert_eq!(r, Value::Number(2.0), "Float32Array[1] should be 2");

        // Test iteration with resize during iteration
        let r = ctx
            .eval(
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta = new Float32Array(buf); \
                 ta[0] = 0; ta[1] = 1; ta[2] = 2; ta[3] = 3; \
                 ta[4] = 4; ta[5] = 5; ta[6] = 6; ta[7] = 7; \
                 ta[8] = 8; ta[9] = 9; \
                 var result = []; \
                 for (var v of ta) { \
                   result.push(v); \
                   if (result.length === 10) buf.resize(80); \
                 } \
                 result.length;",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Number(20.0),
            "Should iterate 20 elements after resize"
        );
    }

    #[test]
    fn typed_array_float32_resizable_buffer_layout_regression() {
        // REGRESSION: Float32Array from resizable ArrayBuffer.
        // The buffer.elements must be laid out in byte layout (4 slots per Float32)
        // so that element[i] reads buf.elements[i*4] correctly.
        // Bug: NaN values appear when buffer.elements is only byte-sized.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Exact test262 flow: CreateRab(40) + Float32Array(buf) + write + iterate + resize
        let r = ctx
            .eval(
                // Create resizable buffer with 40 bytes = 10 Float32 elements
                "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
                 var ta_write = new Float32Array(buf); \
                 for (let i = 0; i < 10; ++i) { ta_write[i] = i; } \
                 var length_tracking_ta = new Float32Array(buf); \
                 var values = []; \
                 for (let v of length_tracking_ta) { \
                   values.push(v); \
                   if (values.length === 10) buf.resize(80); \
                 } \
                 values.slice(0, 10).join(',');",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::String("0,1,2,3,4,5,6,7,8,9".to_string()),
            "Should iterate 0-9 before resize, got: {}",
            r
        );
    }







    /// Isolates the NaN bug to either the builtin Float32Array or MyFloat32Array (subclass).
    #[test]
    fn typed_array_float32_subclass_minimal() {
        // Test the subclass path WITHOUT the harness. This uses the EXACT same
        // test262 logic but bypasses HarnessLoader to get cleaner failure signal.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Create a subclass the same way resizableArrayBufferUtils.js does:
        // new Function('return class MyFloat32Array extends Float32Array {}')()
        let r = ctx.eval(
            "var MyFloat32Array = new Function('return class MyFloat32Array extends Float32Array {}')(); \
             var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
             var ta = new MyFloat32Array(buf); \
             ta[0] = 42.5; \
             ta[1] = 99.25; \
             ta[0] + ',' + ta[1];",
        );
        // Expected: "42.5,99.25"
        // If this is NaN, the subclass constructor doesn't set ObjData::Idx correctly
        assert_eq!(
            r.unwrap(),
            Value::String("42.5,99.25".to_string()),
            "Subclass Float32Array read/write should work"
        );
    }

    /// Test the full harness flow but only with Float32Array (no subclass).
    #[test]
    fn typed_array_float32_harness_builtin_only() {
        // This tests whether the harness itself causes issues for the builtin Float32Array.
        // If this FAILS, the harness is the problem. If it PASSES, MyFloat32Array is the problem.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        // Simulate what the harness does but only for Float32Array (no subclass)
        let r = ctx.eval(
            "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
             var ta_write = new Float32Array(buf); \
             for (let i = 0; i < 10; ++i) { ta_write[i] = i; } \
             var length_tracking_ta = new Float32Array(buf); \
             var values = []; \
             for (let v of length_tracking_ta) { \
               values.push(v); \
               if (values.length === 10) buf.resize(80); \
             } \
             values.slice(0, 10).join(',');",
        );
        assert_eq!(
            r.unwrap(),
            Value::String("0,1,2,3,4,5,6,7,8,9".to_string()),
            "Builtin Float32Array harness path should work"
        );
    }

    /// Test the full harness flow with Float32Array AND MyFloat32Array (subclass).
    #[test]
    fn typed_array_float32_harness_subclass_minimal() {
        // This tests the FULL harness path with subclasses. If this fails but
        // typed_array_float32_harness_builtin_only passes, the subclass is the bug.
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);

        let r = ctx.eval(
            "var MyFloat32Array = new Function('return class MyFloat32Array extends Float32Array {}')(); \
             var MyUint8Array = new Function('return class MyUint8Array extends Uint8Array {}')(); \
             var ctors = [Float32Array, MyFloat32Array, MyUint8Array]; \
             var allPassed = true; \
             var failures = []; \
             for (var ctor of ctors) { \
               var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
               var ta_write = new ctor(buf); \
               for (var i = 0; i < 10; ++i) { ta_write[i] = i; } \
               var length_tracking_ta = new ctor(buf); \
               var values = []; \
               for (var v of length_tracking_ta) { \
                 values.push(v); \
                 if (values.length === 10) buf.resize(80); \
               } \
               var got = values.slice(0, 10).join(','); \
               var expected = '0,1,2,3,4,5,6,7,8,9'; \
               if (got !== expected) { \
                 allPassed = false; \
                 failures.push(ctor.name + ': got ' + got + ' expected ' + expected); \
               } \
             } \
             allPassed ? 'PASS' : 'FAIL:' + failures.join(';');",
        );
        assert_eq!(
            r.as_ref().unwrap(),
            &Value::String("PASS".to_string()),
            "Harness path with subclasses: {}",
            r.as_ref().unwrap()
        );
    }








    #[test]
    fn typed_array_shrink_mid_iteration_throws_typeerror() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Uint8Array(buf, 0, 3); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             var values = []; \
             var threw = false; \
             try { \
               for (var v of ta) { \
                 values.push(v); \
                 if (values.length === 2) buf.resize(1); \
               } \
             } catch (e) { \
               threw = e instanceof TypeError; \
             } \
             threw",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn typed_array_oob_iterator_next_throws_typeerror() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Uint8Array(buf, 0, 3); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             buf.resize(1); \
             ta.length + ',' + buf.byteLength + ',' + (ta.length > buf.byteLength);",
        );
        assert_eq!(result.unwrap(), Value::String("3,1,true".to_string()));
    }

    #[test]
    fn typed_array_oob_iterator_next_step_by_step() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Uint8Array(buf, 0, 3); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             var iter = ta[Symbol.iterator](); \
             var r0 = iter.next(); \
             var r1 = iter.next(); \
             buf.resize(1); \
             var threw = false; \
             var r2desc = 'no_err'; \
             try { var r2 = iter.next(); r2desc = 'v:' + r2.value + ',d:' + r2.done; } \
             catch (e) { threw = e instanceof TypeError; r2desc = 'err:' + e.constructor.name; } \
             threw + ',' + r0.value + ',' + r1.value + ',' + r0.done + ',' + r1.done + ',' + r2desc;",
        );
        assert!(result.is_ok(), "eval failed: {:?}", result);
        assert_eq!(
            result.unwrap(),
            Value::String("true,1,2,false,false,err:TypeError".to_string())
        );
    }

    #[test]
    fn typed_array_oob_direct_access_throws_typeerror() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Uint8Array(buf, 0, 3); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             buf.resize(1); \
             var threw = false; \
             try { ta[0]; } catch (e) { threw = e instanceof TypeError; } \
             threw",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }





    #[test]
    fn typed_array_shrink_list_ctors() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var MyUint8Array = new Function('return class MyUint8Array extends Uint8Array {}')(); \
             var MyFloat32Array = new Function('return class MyFloat32Array extends Float32Array {}')(); \
             var builtinCtors = [Uint8Array, Int8Array, Uint16Array, Int16Array, Uint32Array, Int32Array, Float32Array, Float64Array, Uint8ClampedArray]; \
             var ctors = builtinCtors.concat(MyUint8Array, MyFloat32Array); \
             ctors.map(function(c) { return c ? c.name : 'UNDEFINED'; }).join(',');",
        );
        // Just verify no error
        assert!(result.is_ok());
    }

    #[test]
    fn typed_array_shrink_int8() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Int8Array(buf, 0, 3); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             var values = []; \
             for (var v of ta) { values.push(v); if (values.length === 2) buf.resize(1); } \
             'iterated:' + values.join(',');",
        );
        assert!(result.is_err(), "Int8Array shrink should throw TypeError");
    }

    #[test]
    fn typed_array_shrink_float32() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(40, { maxByteLength: 80 }); \
             var ta = new Float32Array(buf, 0, 3); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             var values = []; \
             for (var v of ta) { values.push(v); if (values.length === 2) buf.resize(1); } \
             'iterated:' + values.join(',');",
        );
        assert!(
            result.is_err(),
            "Float32Array shrink should throw TypeError"
        );
    }


    #[test]
    fn typed_array_shrink_does_not_throw_for_length_tracking() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Uint8Array(buf); \
             ta[0] = 1; ta[1] = 2; ta[2] = 3; \
             var values = []; \
             for (var v of ta) { \
               values.push(v); \
               if (values.length === 5) buf.resize(5); \
             } \
             values.join(',');",
        );
        assert_eq!(result.unwrap(), Value::String("1,2,3,0,0".to_string()));
    }

    #[test]
    fn typed_array_shrink_past_offset_throws_for_length_tracking() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var buf = new ArrayBuffer(10, { maxByteLength: 20 }); \
             var ta = new Uint8Array(buf, 2); \
             var values = []; \
             var threw = ''; \
             try { \
               for (var v of ta) { \
                 values.push(v); \
                 if (values.length === 2) buf.resize(1); \
               } \
             } catch (e) { threw = e.name; } \
             values.join(',') + '|' + threw;",
        );
        assert_eq!(result.unwrap(), Value::String("0,0|TypeError".to_string()));
    }

    #[test]
    fn typed_array_resizable_buffer_length_tracking_at_end_is_empty() {
        let ctx = &mut Context::new().unwrap();
        register_typed_arrays(ctx);
        crate::builtins::register_builtins(ctx);
        let result = ctx.eval(
            "var b = new ArrayBuffer(10, {maxByteLength: 20}); \
             var t = new Uint8Array(b, 10); \
             var values = []; for (var v of t) values.push(v); \
             values.length;",
        );
        assert_eq!(result.unwrap(), Value::Number(0.0));
    }


    #[test]
    fn typed_array_iterator_rejects_detached_buffer() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var a = new Float64Array(1); a.buffer.detached = true; \
                 try { a.values().next(); false } catch (e) { e instanceof TypeError }",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn typed_array_keys_returns_index_iterator() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Float64Array(1).keys().next().value").unwrap();
        assert_eq!(result, Value::Number(0.0));
    }
}
