//! Minimal WeakRef builtin for subclassing and basic construction.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{NativeFunction, Object, ObjectKind, Value};

pub fn register_weak_ref(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let proto_clone = Rc::clone(&proto_rc);
    proto_rc.borrow_mut().set(
        "deref",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(this_obj) = this_val else {
                return Ok(Value::Undefined);
            };
            let target = this_obj
                .borrow()
                .get("__target")
                .unwrap_or(Value::Undefined);
            Ok(target)
        }))),
    );

    let mut wr_native = NativeFunction::new_with_prototype(
        move |args| {
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                this_obj.borrow_mut().set("__target", target);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Err(crate::JsError::new(
                    "TypeError: WeakRef constructor requires 'new'",
                ))
            }
        },
        Rc::clone(&proto_rc),
    );
    wr_native.name = "WeakRef".to_string();
    let wr_fn_rc = Rc::new(wr_native);
    let _ = wr_fn_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    ctx.set_global("WeakRef".to_string(), Value::NativeFunction(wr_fn_rc));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn eval_ok(src: &str) -> Value {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    #[test]
    fn weak_ref_subclass_instanceof() {
        let ok = eval_ok(
            "class Sub extends WeakRef {} var o = {}; var s = new Sub(o); s instanceof Sub && s instanceof WeakRef",
        );
        assert_eq!(ok, Value::Boolean(true));
    }
}
