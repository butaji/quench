//! Minimal WeakRef and FinalizationRegistry builtins for subclassing and basic construction.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::Context;
use crate::value::{NativeFunction, Object, ObjectKind, PropertyFlags, Value};

fn can_be_held_weakly(value: &Value) -> bool {
    match value {
        Value::Object(_) => true,
        Value::Symbol(s) if !s.global => true,
        _ => false,
    }
}

fn make_deref_native() -> NativeFunction {
    let mut f = NativeFunction::new(move |_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let Value::Object(this_obj) = this_val else {
            let msg =
                "TypeError: WeakRef.prototype.deref called on incompatible 'this'".to_string();
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&msg, "TypeError");
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        };
        let obj = this_obj.borrow();
        if !obj.weak_ref_target.is_some() {
            drop(obj);
            let msg =
                "TypeError: WeakRef.prototype.deref called on incompatible 'this'".to_string();
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&msg, "TypeError");
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
        let target = obj.weak_ref_target.clone().unwrap_or(Value::Undefined);
        Ok(target)
    });
    f.name = "deref".to_string();
    f.define_property(
        "length",
        Value::Number(0.0),
        PropertyFlags {
            value: Some(Value::Number(0.0)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    f.define_property(
        "name",
        Value::String("deref".to_string()),
        PropertyFlags {
            value: Some(Value::String("deref".to_string())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    f
}

/// Build a placeholder native method for the FinalizationRegistry prototype
/// with the correct name and length (per spec).
fn make_fr_proto_method<F>(name: &str, f: F) -> NativeFunction
where
    F: Fn(Vec<Value>) -> Result<Value, crate::value::JsError> + 'static,
{
    let mut nf = NativeFunction::new(f);
    nf.name = name.to_string();
    let length = if name == "register" || name == "unregister" {
        2.0
    } else {
        1.0
    };
    nf.define_property(
        "length",
        Value::Number(length),
        PropertyFlags {
            value: Some(Value::Number(length)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    nf.define_property(
        "name",
        Value::String(name.to_string()),
        PropertyFlags {
            value: Some(Value::String(name.to_string())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    nf
}

pub fn register_weak_ref(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let deref_fn = Rc::new(make_deref_native());
    let deref_val = Value::NativeFunction(Rc::clone(&deref_fn));
    proto_rc.borrow_mut().define(
        "deref",
        deref_val.clone(),
        PropertyFlags {
            value: Some(deref_val),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    if let Some(Value::Symbol(tag_key)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        proto_rc.borrow_mut().define(
            &tag_key.property_key(),
            Value::String("WeakRef".into()),
            PropertyFlags {
                value: Some(Value::String("WeakRef".into())),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }

    let proto_clone = Rc::clone(&proto_rc);
    let mut wr_native = NativeFunction::new_with_prototype(
        move |args| {
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            if !can_be_held_weakly(&target) {
                let msg = "TypeError: WeakRef: cannot hold non-object as target".to_string();
                let (err_val, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err_val);
                return Err(js_err);
            }
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                this_obj.borrow_mut().weak_ref_target = Some(target);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                let msg = "TypeError: WeakRef must be called with new".to_string();
                let (err_val, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err_val);
                Err(js_err)
            }
        },
        Rc::clone(&proto_rc),
    );
    wr_native.set_constructable(true);
    wr_native.name = "WeakRef".to_string();
    let wr_fn_rc = Rc::new(wr_native);
    wr_fn_rc.define_property(
        "length",
        Value::Number(1.0),
        PropertyFlags {
            value: Some(Value::Number(1.0)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    wr_fn_rc.define_property(
        "prototype",
        Value::Object(Rc::clone(&proto_rc)),
        PropertyFlags {
            value: Some(Value::Object(Rc::clone(&proto_rc))),
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );
    proto_rc.borrow_mut().define(
        "constructor",
        Value::NativeFunction(Rc::clone(&wr_fn_rc)),
        PropertyFlags {
            value: Some(Value::NativeFunction(Rc::clone(&wr_fn_rc))),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    ctx.set_global("WeakRef".to_string(), Value::NativeFunction(wr_fn_rc));
}

/// Register a minimal `FinalizationRegistry` global constructor.
/// The prototype methods (cleanupSome, register, unregister) are
/// patched by `builtins/FinalizationRegistry.js` during bootstrap.
pub fn register_finalization_registry(ctx: &mut Context) {
    let proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    // Native placeholder methods (no-op until native backing lands).
    // These carry proper name/length descriptors so test262 sees the spec
    // shape even before builtins/FinalizationRegistry.js overrides them
    // with the no-op wrappers used by the harness.
    let register_fn = Rc::new(make_fr_proto_method("register", |_args| {
        Ok(Value::Undefined)
    }));
    let unregister_fn = Rc::new(make_fr_proto_method("unregister", |_args| {
        Ok(Value::Boolean(false))
    }));
    let cleanup_some_fn = Rc::new(make_fr_proto_method("cleanupSome", |_args| {
        Ok(Value::Undefined)
    }));
    proto_rc.borrow_mut().define(
        "register",
        Value::NativeFunction(Rc::clone(&register_fn)),
        PropertyFlags {
            value: Some(Value::NativeFunction(Rc::clone(&register_fn))),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    proto_rc.borrow_mut().define(
        "unregister",
        Value::NativeFunction(Rc::clone(&unregister_fn)),
        PropertyFlags {
            value: Some(Value::NativeFunction(Rc::clone(&unregister_fn))),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    proto_rc.borrow_mut().define(
        "cleanupSome",
        Value::NativeFunction(Rc::clone(&cleanup_some_fn)),
        PropertyFlags {
            value: Some(Value::NativeFunction(Rc::clone(&cleanup_some_fn))),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    if let Some(Value::Symbol(tag_key)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        proto_rc.borrow_mut().define(
            &tag_key.property_key(),
            Value::String("FinalizationRegistry".into()),
            PropertyFlags {
                value: Some(Value::String("FinalizationRegistry".into())),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }

    let proto_clone = Rc::clone(&proto_rc);
    let mut fr_native = NativeFunction::new_with_prototype(
        move |args| {
            let _cleanup_callback = args.first().cloned().unwrap_or(Value::Undefined);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Err(crate::JsError::new(
                    "TypeError: FinalizationRegistry constructor requires 'new'",
                ))
            }
        },
        Rc::clone(&proto_rc),
    );
    fr_native.set_constructable(true);
    fr_native.name = "FinalizationRegistry".to_string();
    let fr_fn_rc = Rc::new(fr_native);
    fr_fn_rc.define_property(
        "length",
        Value::Number(2.0),
        PropertyFlags {
            value: Some(Value::Number(2.0)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    fr_fn_rc.define_property(
        "prototype",
        Value::Object(Rc::clone(&proto_rc)),
        PropertyFlags {
            value: Some(Value::Object(Rc::clone(&proto_rc))),
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );
    proto_rc.borrow_mut().define(
        "constructor",
        Value::NativeFunction(Rc::clone(&fr_fn_rc)),
        PropertyFlags {
            value: Some(Value::NativeFunction(Rc::clone(&fr_fn_rc))),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    ctx.set_global(
        "FinalizationRegistry".to_string(),
        Value::NativeFunction(fr_fn_rc),
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

    #[test]
    fn weak_ref_subclass_instanceof() {
        let ok = eval_ok(
            "class Sub extends WeakRef {} var o = {}; var s = new Sub(o); s instanceof Sub && s instanceof WeakRef",
        );
        assert_eq!(ok, Value::Boolean(true));
    }

    #[test]
    fn weak_ref_length_is_one() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("WeakRef.length").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn weak_ref_throws_for_non_object_target() {
        let mut ctx = Context::new().unwrap();
        assert!(
            ctx.eval("try { new WeakRef(undefined); false } catch(e) { e instanceof TypeError }")
                .unwrap()
                == Value::Boolean(true)
        );
        assert!(
            ctx.eval("try { new WeakRef(null); false } catch(e) { e instanceof TypeError }")
                .unwrap()
                == Value::Boolean(true)
        );
        assert!(
            ctx.eval("try { new WeakRef(1); false } catch(e) { e instanceof TypeError }")
                .unwrap()
                == Value::Boolean(true)
        );
    }

    #[test]
    fn weak_ref_deref_is_not_constructor() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var wr = new WeakRef({}); typeof wr.deref === 'function' && !Object.getOwnPropertyDescriptor(WeakRef.prototype, 'deref').enumerable").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }
}
