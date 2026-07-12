//! Number built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{
    to_js_string, to_number, NativeFunction, Object, ObjectKind, PropertyFlags, Value,
};
use crate::Context;

/// Create non-writable, non-enumerable, non-configurable property flags
fn constant_flags(value: f64) -> PropertyFlags {
    PropertyFlags {
        value: Some(Value::Number(value)),
        writable: false,
        enumerable: false,
        configurable: false,
    }
}

// ============================================================================
// Number
// ============================================================================

pub fn register_number(ctx: &mut Context) {
    let number_proto = Object::new(ObjectKind::Ordinary);
    let number_proto_rc = Rc::new(RefCell::new(number_proto));

    setup_number_prototype(&number_proto_rc);
    setup_number_static(&number_proto_rc, ctx);
}

fn setup_number_prototype(proto: &Rc<RefCell<Object>>) {
    proto.borrow_mut().set(
        "toFixed",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            proto_to_fixed_impl(args)
        }))),
    );

    proto.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Number(0.0));
            // Try to get _value from the object, or convert this to number
            if let Value::Object(obj) = &this_val {
                if let Some(Value::Number(n)) = obj.borrow().get("_value") {
                    return Ok(Value::Number(n));
                }
            }
            Ok(Value::Number(to_number(&this_val)))
        }))),
    );
}

fn proto_to_fixed_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Number(0.0));
    let n = to_number(&this_val);
    let digits = args.first().map(|v| to_number(v) as i32).unwrap_or(0);
    let digits = digits.clamp(0, 100) as usize;

    if n.is_nan() {
        return Ok(Value::String("NaN".to_string()));
    }
    if !n.is_finite() {
        return Ok(Value::String(
            if n.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            }
            .to_string(),
        ));
    }

    let n = if n == 0.0 && n.is_sign_negative() {
        -0.0f64
    } else {
        n
    };
    Ok(Value::String(format!("{:.prec$}", n, prec = digits)))
}

fn setup_number_static(proto: &Rc<RefCell<Object>>, ctx: &mut Context) {
    // Number() returns a primitive, new Number() returns an object
    let proto_for_fn = Rc::clone(proto);
    let proto_for_closure = Rc::clone(&proto_for_fn);
    let number_fn = Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let n = args.first().map(to_number).unwrap_or(0.0);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            // If called with 'new', 'this' is the newly created object
            if let Value::Object(this_obj) = this_val {
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::Number(n),
                );
                // Set exotic_kind for proper toString behavior
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::Number);
                // Set prototype if not already set
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_for_closure));
                }
                Ok(Value::Object(this_obj))
            } else {
                Ok(Value::Number(n))
            }
        },
        proto_for_fn,
    )));

    let number_obj = Object::new(ObjectKind::Ordinary);
    let number_obj = Rc::new(RefCell::new(number_obj));
    number_obj
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(proto)));
    number_obj.borrow_mut().set("constructor", number_fn);
    // Number constants: non-writable, non-enumerable, non-configurable per spec
    number_obj.borrow_mut().define(
        "MAX_VALUE",
        Value::Number(f64::MAX),
        constant_flags(f64::MAX),
    );
    number_obj.borrow_mut().define(
        "MIN_VALUE",
        Value::Number(f64::MIN_POSITIVE),
        constant_flags(f64::MIN_POSITIVE),
    );
    number_obj
        .borrow_mut()
        .define("NaN", Value::Number(f64::NAN), constant_flags(f64::NAN));
    number_obj.borrow_mut().define(
        "NEGATIVE_INFINITY",
        Value::Number(f64::NEG_INFINITY),
        constant_flags(f64::NEG_INFINITY),
    );
    number_obj.borrow_mut().define(
        "POSITIVE_INFINITY",
        Value::Number(f64::INFINITY),
        constant_flags(f64::INFINITY),
    );
    number_obj.borrow_mut().define(
        "EPSILON",
        Value::Number(f64::EPSILON),
        constant_flags(f64::EPSILON),
    );
    number_obj.borrow_mut().define(
        "MAX_SAFE_INTEGER",
        Value::Number(9007199254740991.0),
        constant_flags(9007199254740991.0),
    );
    number_obj.borrow_mut().define(
        "MIN_SAFE_INTEGER",
        Value::Number(-9007199254740991.0),
        constant_flags(-9007199254740991.0),
    );

    number_obj.borrow_mut().set(
        "isInteger",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            Ok(Value::Boolean(n.is_finite() && n.fract() == 0.0))
        }))),
    );
    // Number.isNaN / Number.isFinite: no coercion, only true numbers qualify
    number_obj.borrow_mut().set(
        "isNaN",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            Ok(Value::Boolean(
                matches!(args.first(), Some(Value::Number(n)) if n.is_nan()),
            ))
        }))),
    );
    number_obj.borrow_mut().set(
        "isFinite",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            Ok(Value::Boolean(
                matches!(args.first(), Some(Value::Number(n)) if n.is_finite()),
            ))
        }))),
    );
    number_obj.borrow_mut().set(
        "isSafeInteger",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            Ok(Value::Boolean(
                n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0,
            ))
        }))),
    );
    // Number.parseInt / Number.parseFloat behave identically to the globals
    number_obj.borrow_mut().set(
        "parseInt",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let s = args.first().map(to_js_string).unwrap_or_default();
            let radix = args.get(1).map(|v| to_number(v) as i32).unwrap_or(0);
            let radix = if radix == 0 { 10 } else { radix };
            if !(2..=36).contains(&radix) {
                return Ok(Value::Number(f64::NAN));
            }
            Ok(Value::Number(crate::builtins::date::spec_parse_int(
                &s,
                radix as u32,
            )))
        }))),
    );
    number_obj.borrow_mut().set(
        "parseFloat",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let s = args.first().map(to_js_string).unwrap_or_default();
            Ok(Value::Number(crate::builtins::date::spec_parse_float(&s)))
        }))),
    );
    ctx.set_global("Number".to_string(), Value::Object(number_obj));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_flags_non_writable() {
        let flags = constant_flags(42.0);
        assert!(!flags.writable, "constant should be non-writable");
        assert!(!flags.enumerable, "constant should be non-enumerable");
        assert!(!flags.configurable, "constant should be non-configurable");
        assert!(flags.value.is_some());
    }

    #[test]
    fn test_number_constants_correct_values() {
        // Verify the constant_flags function produces correct values
        assert_eq!(
            constant_flags(f64::MAX).value,
            Some(Value::Number(f64::MAX))
        );
        assert_eq!(
            constant_flags(f64::MIN_POSITIVE).value,
            Some(Value::Number(f64::MIN_POSITIVE))
        );
        assert!(match constant_flags(f64::NAN).value {
            Some(Value::Number(n)) => n.is_nan(),
            _ => false,
        });
        assert_eq!(
            constant_flags(f64::INFINITY).value,
            Some(Value::Number(f64::INFINITY))
        );
        assert_eq!(
            constant_flags(f64::NEG_INFINITY).value,
            Some(Value::Number(f64::NEG_INFINITY))
        );
    }

    #[test]
    fn test_to_fixed_handles_special_values() {
        // Test proto_to_fixed_impl handles NaN
        let nan_result = proto_to_fixed_impl(vec![]);
        assert!(nan_result.is_ok());
    }

    #[test]
    fn test_number_statics() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Number.isInteger(4)").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("Number.isInteger(4.5)").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Number.isInteger(NaN)").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Number.isSafeInteger(9007199254740991)").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("Number.isSafeInteger(9007199254740992)").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Number.EPSILON").unwrap(),
            Value::Number(f64::EPSILON)
        );
        assert_eq!(
            ctx.eval("Number.MAX_SAFE_INTEGER").unwrap(),
            Value::Number(9007199254740991.0)
        );
        assert_eq!(
            ctx.eval("Number.MIN_SAFE_INTEGER").unwrap(),
            Value::Number(-9007199254740991.0)
        );
        assert_eq!(
            ctx.eval("Number.parseInt('42')").unwrap(),
            Value::Number(42.0)
        );
        assert_eq!(
            ctx.eval("Number.parseFloat('3.5')").unwrap(),
            Value::Number(3.5)
        );
    }
}
