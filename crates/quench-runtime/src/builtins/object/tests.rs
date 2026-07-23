//! Tests for Object built-in

use crate::Context;

fn eval(src: &str) -> Result<crate::Value, crate::value::JsError> {
    Context::new().unwrap().eval(src)
}

/// super(42) should NOT throw — the Object constructor called via
/// super() must handle primitive arguments without error.
#[test]
fn object_super_with_number_arg_ok() {
    let r = eval(
        "class MyObj extends Object { \
         constructor() { super(42); } \
         } \
         new MyObj()",
    );
    assert!(r.is_ok(), "super(42) should succeed: {:?}", r.err());
    let v = r.unwrap();
    assert!(matches!(v, crate::Value::Object(_)));
}

/// super() (no argument) should work.
#[test]
fn object_super_no_arg_ok() {
    let r = eval(
        "class MyObj extends Object { \
         constructor() { super(); } \
         } \
         new MyObj()",
    );
    assert!(r.is_ok(), "super() should succeed: {:?}", r.err());
}

/// super('hello') should NOT throw.
#[test]
fn object_super_with_string_arg_ok() {
    let r = eval(
        "class MyObj extends Object { \
         constructor() { super('hello'); } \
         } \
         new MyObj()",
    );
    assert!(r.is_ok(), "super('hello') should succeed: {:?}", r.err());
}

/// A derived class constructor that returns a non-object (42) should
/// throw TypeError per ES §9.2.2 [[Construct]] step 13.b.
#[test]
fn derived_constructor_returns_primitive_throws_typeerror() {
    let r = eval(
        "class Obj extends Object { \
         constructor() { return 42; } \
         } \
         new Obj()",
    );
    assert!(r.is_err(), "constructor returning 42 should throw");
    let err = r.unwrap_err().to_string();
    assert!(
        err.contains("TypeError"),
        "error should be TypeError, got: {err}"
    );
}

/// A base class constructor that returns a primitive (non-undefined,
/// non-object) should also throw TypeError.
#[test]
fn base_constructor_returns_primitive_throws_typeerror() {
    let r = eval(
        "class Base { \
         constructor() { return 42; } \
         } \
         new Base()",
    );
    assert!(r.is_err(), "constructor returning 42 should throw");
}
