//! ArrayBuffer builtin with resizable buffer support.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{to_number, NativeFunction, ObjData, Object, ObjectKind, Value};

fn is_array_buffer(object: &Object) -> bool {
    object.get_own("\0arrayBuffer").is_some()
}

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
            sliced.set("\0arrayBuffer", Value::Boolean(true));
            sliced.elements = this_obj.borrow().elements[start..end].to_vec();
            crate::builtins::object::set_boxed_value(&mut sliced, Value::Number(sliced_len));
            Ok(Value::Object(Rc::new(RefCell::new(sliced))))
        }))),
    );

    for (name, is_slice) in [
        ("transfer", false),
        ("transferToFixedLength", false),
        ("transferToImmutable", false),
        ("sliceToImmutable", true),
    ] {
        let proto = Rc::clone(&proto_rc);
        proto_rc.borrow_mut().set(
            name,
            Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
                let Value::Object(this_obj) = this_val else {
                    return Err(crate::JsError::new(
                        "TypeError: ArrayBuffer method requires an ArrayBuffer receiver",
                    ));
                };
                let mut obj = this_obj.borrow_mut();
                let start = args
                    .first()
                    .map(|value| to_number(value) as usize)
                    .unwrap_or(0);
                let end = args
                    .get(1)
                    .map(|value| to_number(value) as usize)
                    .unwrap_or(obj.elements.len())
                    .min(obj.elements.len());
                let start = start.min(end);
                let elements = obj.elements[start..end].to_vec();
                if !is_slice {
                    obj.elements.clear();
                    obj.set("byteLength", Value::Number(0.0));
                }
                let length = elements.len() as f64;
                let mut result = Object::new(ObjectKind::Ordinary);
                result.prototype = Some(Rc::clone(&proto));
                result.elements = elements;
                result.set("byteLength", Value::Number(length));
                result.set("\0arrayBuffer", Value::Boolean(true));
                crate::builtins::object::set_boxed_value(&mut result, Value::Number(length));
                Ok(Value::Object(Rc::new(RefCell::new(result))))
            }))),
        );
    }

    // ArrayBuffer.prototype.resizable getter
    let resizable_getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(o) = this_val else {
            return Err(crate::JsError::new(
                "TypeError: ArrayBuffer.prototype.resizable requires an ArrayBuffer receiver",
            ));
        };
        let o = o.borrow();
        if !is_array_buffer(&o) {
            return Err(crate::JsError::new(
                "TypeError: ArrayBuffer.prototype.resizable requires an ArrayBuffer receiver",
            ));
        }
        let max_bl = o
            .get_own_value("maxByteLength")
            .map(|v| to_number(&v))
            .unwrap_or(0.0);
        Ok(Value::Boolean(max_bl > 0.0))
    })));
    proto_rc
        .borrow_mut()
        .set_getter_func("resizable", resizable_getter);

    // ArrayBuffer.prototype.maxByteLength getter
    let max_bl_getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(o) = this_val else {
            return Err(crate::JsError::new(
                "TypeError: ArrayBuffer.prototype.maxByteLength requires an ArrayBuffer receiver",
            ));
        };
        let o = o.borrow();
        Ok(o.get_own_value("maxByteLength")
            .unwrap_or(Value::Number(0.0)))
    })));
    proto_rc
        .borrow_mut()
        .set_getter_func("maxByteLength", max_bl_getter);

    // ArrayBuffer.prototype.resize(newByteLength)
    proto_rc.borrow_mut().set(
        "resize",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(this_obj) = this_val else {
                return Err(crate::JsError::new(
                    "TypeError: ArrayBuffer.prototype.resize requires an ArrayBuffer receiver",
                ));
            };
            let new_len = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            if new_len < 0 {
                return Err(crate::JsError::new(
                    "RangeError: ArrayBuffer.prototype.resize requires a non-negative length",
                ));
            }
            let max_bl = this_obj
                .borrow()
                .get_own_value("maxByteLength")
                .map(|v| to_number(&v) as i64)
                .unwrap_or(0);
            if max_bl <= 0 {
                return Err(crate::JsError::new(
                    "TypeError: ArrayBuffer.prototype.resize called on non-resizable ArrayBuffer",
                ));
            }
            if new_len > max_bl {
                return Err(crate::JsError::new(
                    "RangeError: ArrayBuffer.prototype.resize: new length exceeds maxByteLength",
                ));
            }
            let mut obj = this_obj.borrow_mut();
            let old_len = obj.elements.len();
            obj.set("byteLength", Value::Number(new_len as f64));
            if new_len > old_len as i64 {
                // Grow: append zero-initialized elements
                obj.elements.resize(new_len as usize, Value::Number(0.0));
            } else {
                // Shrink: truncate
                obj.elements.truncate(new_len as usize);
            }
            Ok(Value::Undefined)
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
                this_obj
                    .borrow_mut()
                    .set("\0arrayBuffer", Value::Boolean(true));
                // Read maxByteLength from options argument
                let max_bl = args
                    .get(1)
                    .and_then(|v| {
                        if let Value::Object(o) = v {
                            Some(
                                o.borrow()
                                    .get("maxByteLength")
                                    .map(|v| to_number(&v))
                                    .unwrap_or(0.0),
                            )
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0);
                this_obj.borrow_mut().set("byteLength", Value::Number(len));
                if max_bl > 0.0 {
                    this_obj
                        .borrow_mut()
                        .set("maxByteLength", Value::Number(max_bl));
                }
                let len_usize = len as usize;
                let mut elements = Vec::new();
                if elements.try_reserve_exact(len_usize).is_err() {
                    let (error, js_error) = crate::value::error::create_js_error_with_type(
                        "ArrayBuffer allocation is too large",
                        "RangeError",
                    );
                    crate::value::set_thrown_value(error);
                    return Err(js_error);
                }
                elements.resize(len_usize, Value::Number(0.0));
                this_obj.borrow_mut().elements = elements;
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
    let _ = ab_fn_rc.set_property("prototype", Value::Object(Rc::clone(&proto_rc)));
    let _ = ab_fn_rc.set_property(
        "isView",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let is_view = matches!(args.first(), Some(Value::Object(object)) if {
                let object = object.borrow();
                matches!(object.data, ObjData::Idx { .. })
                    || object.get_own("\0dataView").is_some()
            });
            Ok(Value::Boolean(is_view))
        }))),
    );
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
    fn array_buffer_is_view_rejects_ordinary_objects() {
        assert_eq!(eval_ok("ArrayBuffer.isView({})"), Value::Boolean(false));
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
    fn array_buffer_resizable_rejects_prototype_lookalikes() {
        assert!(eval_err("Object.create(ArrayBuffer.prototype).resizable"));
    }

    #[test]
    fn array_buffer_transfer_methods_are_functions() {
        let result = eval_ok(
            "typeof ArrayBuffer.prototype.transfer === 'function' && typeof ArrayBuffer.prototype.transferToFixedLength === 'function' && typeof ArrayBuffer.prototype.sliceToImmutable === 'function'",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_constructor_without_new_throws() {
        assert!(eval_err("ArrayBuffer(8)"));
    }

    #[test]
    fn array_buffer_resizable() {
        let result = eval_ok("var ab = new ArrayBuffer(8, { maxByteLength: 16 }); ab.resizable");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_resizable_false() {
        let result = eval_ok("var ab = new ArrayBuffer(8); ab.resizable");
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn array_buffer_max_byte_length() {
        let result =
            eval_ok("var ab = new ArrayBuffer(8, { maxByteLength: 16 }); ab.maxByteLength");
        assert_eq!(result.to_string(), "16");
    }

    #[test]
    fn array_buffer_rejects_unallocatable_length() {
        let result = eval_err("new ArrayBuffer(9007199254740991)");
        assert!(result);
    }

    #[test]
    fn array_buffer_allocation_failure_is_range_error_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("try { new ArrayBuffer(9007199254740991); false } catch (error) { error instanceof RangeError }")
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_resize_grow() {
        let result = eval_ok(
            "var ab = new ArrayBuffer(4, { maxByteLength: 8 }); \
             ab.resize(6); ab.byteLength",
        );
        assert_eq!(result.to_string(), "6");
    }

    #[test]
    fn array_buffer_resize_shrink() {
        let result = eval_ok(
            "var ab = new ArrayBuffer(8, { maxByteLength: 16 }); \
             ab.resize(4); ab.byteLength",
        );
        assert_eq!(result.to_string(), "4");
    }

    #[test]
    fn array_buffer_resize_preserves_data() {
        let result = eval_ok(
            "var ab = new ArrayBuffer(4, { maxByteLength: 8 }); \
             var ta = new Uint8Array(ab); ta[0] = 42; ta[1] = 43; \
             ab.resize(6); ta[0] + ',' + ta[1] + ',' + ta[2]",
        );
        assert_eq!(result.to_string(), "42,43,0");
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
