//! ArrayBuffer builtin with resizable buffer support.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{to_number, NativeFunction, ObjData, Object, ObjectKind, Value};

fn is_array_buffer(object: &Object) -> bool {
    object.get_own("\0arrayBuffer").is_some()
}

fn array_buffer_type_error(message: &str) -> crate::JsError {
    let (error, js_error) = crate::value::error::create_js_error_with_type(message, "TypeError");
    crate::value::set_thrown_value(error);
    js_error
}

fn array_buffer_range_error(message: &str) -> crate::JsError {
    let (error, js_error) = crate::value::error::create_js_error_with_type(message, "RangeError");
    crate::value::set_thrown_value(error);
    js_error
}

pub fn register_array_buffer(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let proto_clone = Rc::clone(&proto_rc);
    let byte_length_getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(object) = this_val else {
            return Err(array_buffer_type_error(
                "ArrayBuffer.prototype.byteLength requires an ArrayBuffer receiver",
            ));
        };
        if !is_array_buffer(&object.borrow()) {
            return Err(array_buffer_type_error(
                "ArrayBuffer.prototype.byteLength requires an ArrayBuffer receiver",
            ));
        }
        let value = object
            .borrow()
            .get_own_value("byteLength")
            .unwrap_or(Value::Number(0.0));
        Ok(value)
    })));
    proto_rc
        .borrow_mut()
        .set_getter_func("byteLength", byte_length_getter);
    for name in ["detached", "immutable"] {
        let getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(object) = this_val else {
                return Err(array_buffer_type_error(
                    "ArrayBuffer getter requires an ArrayBuffer receiver",
                ));
            };
            if !is_array_buffer(&object.borrow()) {
                return Err(array_buffer_type_error(
                    "ArrayBuffer getter requires an ArrayBuffer receiver",
                ));
            }
            let detached = object.borrow().get_own_value("\0detached").is_some();
            Ok(Value::Boolean(detached))
        })));
        proto_rc.borrow_mut().set_getter_func(name, getter);
    }
    proto_rc.borrow_mut().set(
        "slice",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(this_obj) = this_val else {
                return Err(crate::JsError::new(
                    "TypeError: ArrayBuffer.prototype.slice requires an ArrayBuffer receiver",
                ));
            };
            if !is_array_buffer(&this_obj.borrow()) {
                return Err(array_buffer_type_error(
                    "ArrayBuffer.prototype.slice requires an ArrayBuffer receiver",
                ));
            }
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
            let species_constructor = if let Some(Value::Object(constructor)) =
                this_obj.borrow().get_own_value("constructor")
            {
                if let Some(Value::Symbol(species)) =
                    crate::builtins::symbol::get_well_known_symbol_no_ctx("species")
                {
                    let species_value = constructor.borrow().get(&species.property_key());
                    if !matches!(
                        &species_value,
                        None | Some(Value::Undefined) | Some(Value::Null)
                    ) && !species_value
                        .as_ref()
                        .is_some_and(crate::eval::class::helpers::is_constructor_value)
                    {
                        return Err(array_buffer_type_error(
                            "ArrayBuffer species is not a constructor",
                        ));
                    }
                    species_value
                } else {
                    None
                }
            } else {
                None
            };
            let sliced_len = (end - start) as f64;
            if let Some(constructor) = species_constructor {
                let new_obj = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
                let result = crate::eval::function::call_value_with_this(
                    constructor,
                    vec![Value::Number(sliced_len)],
                    Value::Object(Rc::clone(&new_obj)),
                )?;
                let Value::Object(result) = result else {
                    return Err(array_buffer_type_error(
                        "ArrayBuffer species constructor did not return an object",
                    ));
                };
                if !is_array_buffer(&result.borrow())
                    || Rc::ptr_eq(&result, &this_obj)
                    || result.borrow().elements.len() < sliced_len as usize
                {
                    return Err(array_buffer_type_error(
                        "ArrayBuffer species result is not a valid ArrayBuffer",
                    ));
                }
            }
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
                    return Err(array_buffer_type_error(
                        "ArrayBuffer method requires an ArrayBuffer receiver",
                    ));
                };
                if !is_array_buffer(&this_obj.borrow()) {
                    return Err(array_buffer_type_error(
                        "ArrayBuffer method requires an ArrayBuffer receiver",
                    ));
                }
                if !is_slice {
                    if let Some(value) = args.first() {
                        let new_length = to_number(value);
                        if !new_length.is_finite() || !(0.0..=u32::MAX as f64).contains(&new_length)
                        {
                            return Err(array_buffer_range_error(
                                "ArrayBuffer transfer length is out of range",
                            ));
                        }
                    }
                }
                let mut obj = this_obj.borrow_mut();
                let elements = if is_slice {
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
                    obj.elements[start..end].to_vec()
                } else {
                    let requested = args
                        .first()
                        .map(|value| to_number(value) as usize)
                        .unwrap_or(obj.elements.len());
                    let mut elements = obj.elements.clone();
                    elements.resize(requested, Value::Number(0.0));
                    obj.elements.clear();
                    obj.set("byteLength", Value::Number(0.0));
                    obj.set("\0detached", Value::Boolean(true));
                    elements
                };
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
            return Err(array_buffer_type_error(
                "ArrayBuffer.prototype.resizable requires an ArrayBuffer receiver",
            ));
        };
        let o = o.borrow();
        if !is_array_buffer(&o) {
            return Err(array_buffer_type_error(
                "ArrayBuffer.prototype.resizable requires an ArrayBuffer receiver",
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
            return Err(array_buffer_type_error(
                "ArrayBuffer.prototype.maxByteLength requires an ArrayBuffer receiver",
            ));
        };
        let o = o.borrow();
        if !is_array_buffer(&o) {
            return Err(array_buffer_type_error(
                "ArrayBuffer.prototype.maxByteLength requires an ArrayBuffer receiver",
            ));
        }
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
                return Err(array_buffer_type_error(
                    "ArrayBuffer.prototype.resize requires an ArrayBuffer receiver",
                ));
            };
            let new_len = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            if new_len < 0 {
                return Err(array_buffer_range_error(
                    "ArrayBuffer.prototype.resize requires a non-negative length",
                ));
            }
            let max_bl = this_obj
                .borrow()
                .get_own_value("maxByteLength")
                .map(|v| to_number(&v) as i64)
                .unwrap_or(0);
            if max_bl <= 0 {
                return Err(array_buffer_type_error(
                    "ArrayBuffer.prototype.resize called on non-resizable ArrayBuffer",
                ));
            }
            if new_len > max_bl {
                return Err(array_buffer_range_error(
                    "ArrayBuffer.prototype.resize: new length exceeds maxByteLength",
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
                if !len.is_finite() || !(0.0..=u32::MAX as f64).contains(&len) {
                    return Err(array_buffer_range_error(
                        "ArrayBuffer length is out of range",
                    ));
                }
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::Number(len),
                );
                this_obj
                    .borrow_mut()
                    .set("\0arrayBuffer", Value::Boolean(true));
                // Read maxByteLength from options argument
                let max_bl = args.get(1).and_then(|v| {
                    if let Value::Object(o) = v {
                        o.borrow().get("maxByteLength").and_then(|value| {
                            (!matches!(value, Value::Undefined)).then(|| to_number(&value))
                        })
                    } else {
                        None
                    }
                });
                if let Some(max_bl) = max_bl {
                    if !max_bl.is_finite() || !(len..=u32::MAX as f64).contains(&max_bl) {
                        return Err(array_buffer_range_error(
                            "ArrayBuffer maxByteLength is out of range",
                        ));
                    }
                }
                this_obj.borrow_mut().set("byteLength", Value::Number(len));
                if let Some(max_bl) = max_bl {
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

    let byte_length_getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(object) = this_val else {
            return Err(array_buffer_type_error(
                "SharedArrayBuffer.prototype.byteLength requires a SharedArrayBuffer receiver",
            ));
        };
        let value = object
            .borrow()
            .get_own_value("byteLength")
            .unwrap_or(Value::Number(0.0));
        Ok(value)
    })));
    proto_rc
        .borrow_mut()
        .set_getter_func("byteLength", byte_length_getter);

    let growable_getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(object) = this_val else {
            return Err(array_buffer_type_error(
                "SharedArrayBuffer.prototype.growable requires a SharedArrayBuffer receiver",
            ));
        };
        let value = object
            .borrow()
            .get_own_value("maxByteLength")
            .map(|value| to_number(&value) > 0.0)
            .unwrap_or(false);
        Ok(Value::Boolean(value))
    })));
    proto_rc
        .borrow_mut()
        .set_getter_func("growable", growable_getter);

    let max_byte_length_getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(object) = this_val else {
            return Err(array_buffer_type_error(
                "SharedArrayBuffer.prototype.maxByteLength requires a SharedArrayBuffer receiver",
            ));
        };
        if object.borrow().get_own("\0sharedArrayBuffer").is_none() {
            return Err(array_buffer_type_error(
                "SharedArrayBuffer.prototype.maxByteLength requires a SharedArrayBuffer receiver",
            ));
        }
        let value = object
            .borrow()
            .get_own_value("maxByteLength")
            .unwrap_or(Value::Number(0.0));
        Ok(value)
    })));
    proto_rc
        .borrow_mut()
        .set_getter_func("maxByteLength", max_byte_length_getter);

    proto_rc.borrow_mut().set(
        "grow",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(object) = this_val else {
                return Err(array_buffer_type_error(
                    "SharedArrayBuffer.prototype.grow requires a SharedArrayBuffer receiver",
                ));
            };
            if object.borrow().get_own("\0sharedArrayBuffer").is_none() {
                return Err(array_buffer_type_error(
                    "SharedArrayBuffer.prototype.grow requires a SharedArrayBuffer receiver",
                ));
            }
            let new_len = args.first().map(to_number).unwrap_or(f64::NAN);
            let max_len = object
                .borrow()
                .get_own_value("maxByteLength")
                .map(|value| to_number(&value))
                .unwrap_or(0.0);
            if !new_len.is_finite()
                || new_len < 0.0
                || new_len.fract() != 0.0
                || new_len > max_len
                || new_len > usize::MAX as f64
            {
                return Err(array_buffer_range_error(
                    "SharedArrayBuffer grow length is out of range",
                ));
            }
            let new_len = new_len as usize;
            let mut object = object.borrow_mut();
            object.elements.resize(new_len, Value::Number(0.0));
            object.set("byteLength", Value::Number(new_len as f64));
            Ok(Value::Undefined)
        }))),
    );

    let slice_proto = Rc::clone(&proto_rc);
    proto_rc.borrow_mut().set(
        "slice",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(this_obj) = this_val else {
                return Err(array_buffer_type_error(
                    "SharedArrayBuffer.prototype.slice requires a SharedArrayBuffer receiver",
                ));
            };
            if this_obj.borrow().get_own("\0sharedArrayBuffer").is_none() {
                return Err(array_buffer_type_error(
                    "SharedArrayBuffer.prototype.slice requires a SharedArrayBuffer receiver",
                ));
            }
            let len = this_obj
                .borrow()
                .get_own_value("byteLength")
                .map(|value| to_number(&value) as usize)
                .unwrap_or(0);
            let start = args
                .first()
                .map(|value| to_number(value) as isize)
                .unwrap_or(0)
                .clamp(0, len as isize) as usize;
            let end = args
                .get(1)
                .map(|value| to_number(value) as isize)
                .unwrap_or(len as isize)
                .clamp(start as isize, len as isize) as usize;
            let mut result = Object::new(ObjectKind::Ordinary);
            result.prototype = Some(Rc::clone(&slice_proto));
            result.elements = this_obj.borrow().elements[start..end].to_vec();
            result.set("byteLength", Value::Number((end - start) as f64));
            result.set("\0sharedArrayBuffer", Value::Boolean(true));
            Ok(Value::Object(Rc::new(RefCell::new(result))))
        }))),
    );

    let proto_clone = Rc::clone(&proto_rc);
    let mut sab_native = NativeFunction::new_with_prototype(
        move |args| {
            let len = args.first().map(to_number).unwrap_or(0.0);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                if !len.is_finite() || len < 0.0 || len > usize::MAX as f64 {
                    return Err(array_buffer_range_error(
                        "SharedArrayBuffer allocation is too large",
                    ));
                }
                let len_usize = len as usize;
                let mut elements = Vec::new();
                if elements.try_reserve_exact(len_usize).is_err() {
                    return Err(array_buffer_range_error(
                        "SharedArrayBuffer allocation is too large",
                    ));
                }
                elements.resize(len_usize, Value::Number(0.0));
                this_obj.borrow_mut().set("byteLength", Value::Number(len));
                this_obj.borrow_mut().elements = elements;
                if let Some(Value::Object(options)) = args.get(1) {
                    if let Some(value) = options.borrow().get_own_value("maxByteLength") {
                        let max_len = to_number(&value);
                        if !max_len.is_finite()
                            || max_len < len
                            || max_len.fract() != 0.0
                            || max_len > usize::MAX as f64
                        {
                            return Err(array_buffer_range_error(
                                "SharedArrayBuffer maxByteLength is out of range",
                            ));
                        }
                        this_obj
                            .borrow_mut()
                            .set("maxByteLength", Value::Number(max_len));
                    }
                }
                this_obj
                    .borrow_mut()
                    .set("\0sharedArrayBuffer", Value::Boolean(true));
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
    fn array_buffer_constructor_rejects_negative_length_with_range_error() {
        let result = eval_ok(
            "try { new ArrayBuffer(-1); false } catch (error) { error instanceof RangeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_constructor_rejects_smaller_max_byte_length() {
        let result = eval_ok(
            "try { new ArrayBuffer(8, { maxByteLength: 4 }); false } catch (error) { error instanceof RangeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn shared_array_buffer_byte_length_prototype_property_is_an_accessor() {
        let result = eval_ok(
            "typeof Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'byteLength').get",
        );
        assert_eq!(result, Value::String("function".to_string()));
    }

    #[test]
    fn shared_array_buffer_growable_prototype_property_is_an_accessor() {
        let result = eval_ok(
            "typeof Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'growable').get",
        );
        assert_eq!(result, Value::String("function".to_string()));
    }

    #[test]
    fn shared_array_buffer_max_byte_length_prototype_property_is_an_accessor() {
        let result = eval_ok(
            "typeof Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, 'maxByteLength').get",
        );
        assert_eq!(result, Value::String("function".to_string()));
    }

    #[test]
    fn shared_array_buffer_slice_returns_shared_array_buffer() {
        let result = eval_ok(
            "var sab = new SharedArrayBuffer(4); var result = sab.slice(1, 3); typeof sab.slice === 'function' && result instanceof SharedArrayBuffer && result.byteLength === 2",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn shared_array_buffer_rejects_unallocatable_length_before_allocation() {
        let result = eval_ok(
            "try { new SharedArrayBuffer(9007199254740991); false } catch (error) { error instanceof RangeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn shared_array_buffer_grow_updates_length_and_zero_fills() {
        let result = eval_ok(
            "var sab = new SharedArrayBuffer(2, { maxByteLength: 4 }); sab.grow(4); sab.byteLength === 4 && sab.growable === true",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_resizable_rejects_prototype_lookalikes() {
        assert!(eval_err("Object.create(ArrayBuffer.prototype).resizable"));
    }

    #[test]
    fn array_buffer_byte_length_prototype_property_is_an_accessor() {
        assert_eq!(
            eval_ok(
                "typeof Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get"
            ),
            Value::String("function".to_string())
        );
    }

    #[test]
    fn array_buffer_detached_prototype_property_is_an_accessor() {
        assert_eq!(
            eval_ok(
                "typeof Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached').get"
            ),
            Value::String("function".to_string())
        );
    }

    #[test]
    fn array_buffer_accessor_rejection_is_a_type_error_object() {
        let result = eval_ok(
            "try { Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get.call({}); false } catch (error) { error instanceof TypeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_slice_rejects_non_constructor_species() {
        let result = eval_ok(
            "var buffer = new ArrayBuffer(1); buffer.constructor = { [Symbol.species]: 1 }; try { buffer.slice(); false } catch (error) { error instanceof TypeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_transfer_rejects_prototype_lookalikes() {
        let result = eval_ok(
            "try { Object.create(ArrayBuffer.prototype).transfer(); false } catch (error) { error instanceof TypeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_transfer_rejects_excessive_new_length() {
        let result = eval_ok(
            "try { new ArrayBuffer(1).transfer(9007199254740991); false } catch (error) { error instanceof RangeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_resize_range_errors_are_objects() {
        let result = eval_ok(
            "var buffer = new ArrayBuffer(1, { maxByteLength: 2 }); try { buffer.resize(-1); false } catch (error) { error instanceof RangeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_slice_rejects_species_result_without_array_buffer_data() {
        let result = eval_ok(
            "var constructor = { [Symbol.species]: function() { return {}; } }; var buffer = new ArrayBuffer(1); buffer.constructor = constructor; try { buffer.slice(); false } catch (error) { error instanceof TypeError }",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_transfer_grows_and_detaches_source() {
        let result = eval_ok(
            "var source = new ArrayBuffer(2); var target = source.transfer(4); target.byteLength === 4 && source.byteLength === 0 && source.detached === true",
        );
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_byte_length_getter_length_is_non_writable() {
        let result = eval_ok(
            "var getter = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get; Object.getOwnPropertyDescriptor(getter, 'length').writable === false",
        );
        assert_eq!(result, Value::Boolean(true));
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
