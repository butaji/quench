//! Tests for Object built-in

use crate::value::convert::to_bool;

#[test]
fn extended_uint8_array_subclass_to_string_tag() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval(
            "class ExtendedUint8Array extends Uint8Array { \
               constructor() { super(10); this[0] = 255; this[1] = 0xFFA; } \
             } \
             Object.prototype.toString.call(new ExtendedUint8Array())",
        )
        .unwrap();
    assert_eq!(
        result,
        crate::value::Value::String("[object Uint8Array]".into())
    );
}

#[test]
fn object_subclass_instanceof_object() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval(
            "class Subclass extends Object {} \
             var sub = new Subclass(); \
             sub instanceof Subclass && sub instanceof Object",
        )
        .unwrap();
    assert!(to_bool(&result));
}
