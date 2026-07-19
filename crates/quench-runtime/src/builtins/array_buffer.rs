//! Minimal ArrayBuffer builtin.
//!
//! Only construction with a byte length is supported — enough for the
//! test262 harness (`detachArrayBuffer.js` and friends). Resizable buffers
//! and views (TypedArray/DataView) are intentionally out of scope here and
//! tracked as regular test-by-test work.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{to_number, NativeFunction, Object, ObjectKind, Value};

pub fn register_array_buffer(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let proto_clone = Rc::clone(&proto_rc);
    let ab_fn_rc = Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let len = args.first().map(to_number).unwrap_or(0.0);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::Number(len),
                );
                this_obj.borrow_mut().set("byteLength", Value::Number(len));
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Err(crate::JsError::new(
                    "TypeError: ArrayBuffer constructor requires 'new'",
                ))
            }
        },
        Rc::clone(&proto_rc),
    ));
    // Set prototype property so eval_new can find it
    let _ = ab_fn_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    let ab_fn = Value::NativeFunction(ab_fn_rc);

    ctx.set_global("ArrayBuffer".to_string(), ab_fn);
}
