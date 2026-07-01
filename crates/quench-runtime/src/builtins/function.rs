//! Function built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// Function
// ============================================================================

pub fn register_function(ctx: &mut Context) {
    // Function.prototype - the object that is the prototype of all function objects
    let function_proto = Object::new(ObjectKind::Function);
    let function_proto_rc = Rc::new(RefCell::new(function_proto));
    let function_proto_clone = Rc::clone(&function_proto_rc);

    // Function constructor with prototype
    let function_constructor = NativeConstructor::new(
        move |_args| {
            // Function constructor creates a new function from arguments
            // In practice, we just return an empty function
            let func = Object::with_prototype(ObjectKind::Function, Rc::clone(&function_proto_clone));
            let func_rc = Rc::new(RefCell::new(func));
            Ok(Value::Object(func_rc))
        },
        function_proto_rc,
    );

    ctx.set_global("Function".to_string(), Value::NativeConstructor(Rc::new(function_constructor)));
}
