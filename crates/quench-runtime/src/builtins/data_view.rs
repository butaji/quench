//! Minimal DataView builtin for subclassing and buffer views.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{to_number, NativeFunction, Object, ObjectKind, Value};

fn type_error(message: &str) -> crate::JsError {
    let (thrown, error) = crate::value::error::create_js_error_with_type(message, "TypeError");
    crate::value::set_thrown_value(thrown);
    error
}

pub fn register_data_view(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let proto_clone = Rc::clone(&proto_rc);
    proto_rc.borrow_mut().set(
        "getUint8",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(view) = this_val else {
                return Err(crate::JsError::new(
                    "TypeError: DataView.prototype.getUint8 requires a DataView receiver",
                ));
            };
            let offset = args.first().map(to_number).unwrap_or(0.0) as usize;
            let view = view.borrow();
            let byte_offset = view
                .get_own_value("byteOffset")
                .map(|value| to_number(&value) as usize)
                .unwrap_or(0);
            let Some(Value::Object(buffer)) = view.get_own_value("buffer") else {
                return Err(crate::JsError::new("TypeError: detached DataView"));
            };
            let buffer = buffer.borrow();
            let index = byte_offset.saturating_add(offset);
            let Some(value) = buffer.elements.get(index) else {
                return Err(crate::JsError::new(
                    "RangeError: Offset is outside the bounds of the DataView",
                ));
            };
            Ok(Value::Number(to_number(value)))
        }))),
    );
    let mut dv_native = NativeFunction::new_with_prototype(
        move |args| {
            let Some(buffer) = args.first() else {
                return Err(type_error(
                    "TypeError: DataView constructor requires an ArrayBuffer argument",
                ));
            };
            let byte_offset = args.get(1).map(to_number).unwrap_or(0.0);
            let buffer_len = buffer_byte_length(buffer);
            let byte_length = args
                .get(2)
                .map(to_number)
                .unwrap_or(buffer_len - byte_offset);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                this_obj
                    .borrow_mut()
                    .set("\0dataView", Value::Boolean(true));
                this_obj.borrow_mut().set("buffer", buffer.clone());
                this_obj
                    .borrow_mut()
                    .set("byteOffset", Value::Number(byte_offset));
                this_obj
                    .borrow_mut()
                    .set("byteLength", Value::Number(byte_length));
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Err(crate::JsError::new(
                    "TypeError: DataView constructor requires 'new'",
                ))
            }
        },
        Rc::clone(&proto_rc),
    );
    dv_native.name = "DataView".to_string();
    let dv_fn_rc = Rc::new(dv_native);
    let _ = dv_fn_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    ctx.set_global("DataView".to_string(), Value::NativeFunction(dv_fn_rc));
}

fn buffer_byte_length(buffer: &Value) -> f64 {
    if let Value::Object(o) = buffer {
        o.borrow()
            .get("byteLength")
            .map(|v| to_number(&v))
            .unwrap_or(0.0)
    } else {
        0.0
    }
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
    fn data_view_subclass_regular_subclassing() {
        let same = eval_ok(
            "class DV extends DataView {} var b = new ArrayBuffer(1); new DV(b).buffer === b",
        );
        assert_eq!(same, Value::Boolean(true));
    }

    #[test]
    fn data_view_constructor_without_buffer_throws() {
        assert!(eval_err("class DV extends DataView {} new DV()"));
        assert!(crate::value::take_thrown_value().is_some());
    }

    #[test]
    fn data_view_subclass_instanceof() {
        let ok = eval_ok(
            "class Sub extends DataView {} var s = new Sub(new ArrayBuffer(1)); s instanceof Sub && s instanceof DataView",
        );
        assert_eq!(ok, Value::Boolean(true));
    }

    #[test]
    fn data_view_get_uint8_reads_zero_initialized_byte() {
        let value = eval_ok("new DataView(new ArrayBuffer(1)).getUint8(0)");
        assert_eq!(value, Value::Number(0.0));
    }
}
