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
    proto_rc.borrow_mut().set(
        "slice",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(this_obj) = this_val else {
                return Err(crate::JsError::new(
                    "TypeError: ArrayBuffer.prototype.slice requires an ArrayBuffer receiver",
                ));
            };
            let len = this_obj
                .borrow()
                .get("byteLength")
                .map(|v| to_number(&v) as usize)
                .unwrap_or(0);
            let start = args
                .first()
                .map(|v| to_number(v) as isize)
                .unwrap_or(0)
                .clamp(0, len as isize) as usize;
            let end = args
                .get(1)
                .map(|v| to_number(v) as isize)
                .unwrap_or(len as isize)
                .clamp(start as isize, len as isize) as usize;
            let sliced_len = (end - start) as f64;
            let proto = this_obj.borrow().prototype.clone();
            let mut sliced = Object::new(ObjectKind::Ordinary);
            if let Some(p) = proto {
                sliced.prototype = Some(p);
            }
            sliced.set("byteLength", Value::Number(sliced_len));
            crate::builtins::object::set_boxed_value(&mut sliced, Value::Number(sliced_len));
            Ok(Value::Object(Rc::new(RefCell::new(sliced))))
        }))),
    );

    let mut ab_native = NativeFunction::new_with_prototype(
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
    );
    ab_native.name = "ArrayBuffer".to_string();
    let ab_fn_rc = Rc::new(ab_native);
    // Set prototype property so eval_new can find it
    let _ = ab_fn_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    let ab_fn = Value::NativeFunction(ab_fn_rc);

    ctx.set_global("ArrayBuffer".to_string(), ab_fn);
    register_shared_array_buffer(ctx);
}

fn register_shared_array_buffer(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let proto_clone = Rc::clone(&proto_rc);
    let mut sab_native = NativeFunction::new_with_prototype(
        move |args| {
            let len = args.first().map(to_number).unwrap_or(0.0);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                this_obj.borrow_mut().set("byteLength", Value::Number(len));
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Err(crate::JsError::new(
                    "TypeError: SharedArrayBuffer constructor requires 'new'",
                ))
            }
        },
        Rc::clone(&proto_rc),
    );
    sab_native.name = "SharedArrayBuffer".to_string();
    let sab_fn_rc = Rc::new(sab_native);
    let _ = sab_fn_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    ctx.set_global(
        "SharedArrayBuffer".to_string(),
        Value::NativeFunction(sab_fn_rc),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn eval_ok(src: &str) -> Value {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    fn eval_err(src: &str) -> bool {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).is_err()
    }

    #[test]
    fn array_buffer_exists_as_global() {
        let result = eval_ok("typeof ArrayBuffer");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn array_buffer_constructor_name() {
        let result = eval_ok("ArrayBuffer.name");
        assert!(!result.to_string().is_empty());
    }

    #[test]
    fn array_buffer_constructor_with_length() {
        let result = eval_ok("(new ArrayBuffer(8)).byteLength");
        assert_eq!(result.to_string(), "8");
    }

    #[test]
    fn array_buffer_constructor_with_zero_length() {
        let result = eval_ok("(new ArrayBuffer(0)).byteLength");
        assert_eq!(result.to_string(), "0");
    }

    #[test]
    fn array_buffer_constructor_without_new_throws() {
        assert!(eval_err("ArrayBuffer(8)"));
    }

    #[test]
    fn array_buffer_subclass_auto_super() {
        assert_eq!(
            eval_ok("class AB extends ArrayBuffer {} new AB(4).byteLength").to_string(),
            "4"
        );
    }

    #[test]
    fn array_buffer_subclass_slice() {
        let result = eval_ok("class AB extends ArrayBuffer {} (new AB(4)).slice(0, 1).byteLength");
        assert_eq!(result.to_string(), "1");
    }

    #[test]
    fn array_buffer_regular_subclassing() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                r#"
            class AB extends ArrayBuffer {
                constructor() {
                    super(4);
                }
            }
            var ab = new AB();
            [ab instanceof AB, ab instanceof ArrayBuffer, ab.byteLength];
        "#,
            )
            .unwrap();
        match r {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert_eq!(
                    arr.elements.first().map(|v| v.to_string()),
                    Some("true".to_string()),
                    "ab instanceof AB should be true"
                );
                assert_eq!(
                    arr.elements.get(1).map(|v| v.to_string()),
                    Some("true".to_string()),
                    "ab instanceof ArrayBuffer should be true"
                );
                assert_eq!(
                    arr.elements.get(2).map(|v| v.to_string()),
                    Some("4".to_string()),
                    "ab.byteLength should be 4"
                );
            }
            _ => panic!("expected array result, got {:?}", r),
        }
    }

    #[test]
    fn dataview_regular_subclassing() {
        let mut ctx = Context::new().unwrap();
        // First create an ArrayBuffer, then subclass DataView
        let r = ctx
            .eval(
                r#"
            var buffer = new ArrayBuffer(1);
            class DV extends DataView {}
            var dv = new DV(buffer);
            [dv.buffer === buffer, dv instanceof DV, dv instanceof DataView];
        "#,
            )
            .unwrap();
        match r {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert_eq!(
                    arr.elements.first().map(|v| v.to_string()),
                    Some("true".to_string()),
                    "dv.buffer === buffer should be true"
                );
                assert_eq!(
                    arr.elements.get(1).map(|v| v.to_string()),
                    Some("true".to_string()),
                    "dv instanceof DV should be true"
                );
                assert_eq!(
                    arr.elements.get(2).map(|v| v.to_string()),
                    Some("true".to_string()),
                    "dv instanceof DataView should be true"
                );
            }
            _ => panic!("expected array result, got {:?}", r),
        }
    }

    #[test]
    fn array_buffer_subclass_default_constructor() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                r#"
            class AB extends ArrayBuffer {}
            var ab = new AB(4);
            [ab instanceof AB, ab instanceof ArrayBuffer, ab.byteLength];
        "#,
            )
            .unwrap();
        match r {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert_eq!(
                    arr.elements.first().map(|v| v.to_string()),
                    Some("true".to_string()),
                    "ab instanceof AB should be true"
                );
                assert_eq!(
                    arr.elements.get(1).map(|v| v.to_string()),
                    Some("true".to_string()),
                    "ab instanceof ArrayBuffer should be true"
                );
                assert_eq!(
                    arr.elements.get(2).map(|v| v.to_string()),
                    Some("4".to_string()),
                    "ab.byteLength should be 4"
                );
            }
            _ => panic!("expected array result, got {:?}", r),
        }
    }
}
