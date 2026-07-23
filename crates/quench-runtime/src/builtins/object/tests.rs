//! Tests for Object built-in

use crate::value::convert::to_bool;

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
