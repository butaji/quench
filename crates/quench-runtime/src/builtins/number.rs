//! Number built-in

use std::cell::RefCell;
use std::f64::consts::PI;
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

    // Number.prototype must inherit from Object.prototype so that inherited
    // properties (e.g. Object.prototype.x via Object.defineProperty) are found
    // in the prototype chain of boxed Number objects.
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        number_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

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
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            // Per spec, Number.prototype.toString unwraps the boxed number and
            // returns its ToString representation. This must take precedence
            // over Object.prototype.toString (which yields "[object Number]").
            // We must not go through to_js_string here — it calls the object's
            // own toString, which would recurse forever for boxed numbers.
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Number(0.0));
            let n = to_number(&this_val);
            // Reuse the same number→string rules as to_js_string (NaN, ±Infinity,
            // integer-only, and general float formatting). -0 must stringify
            // as "0" per spec.
            let s = if n.is_nan() {
                "NaN".to_string()
            } else if n == f64::INFINITY {
                "Infinity".to_string()
            } else if n == f64::NEG_INFINITY {
                "-Infinity".to_string()
            } else if n == 0.0 {
                "0".to_string()
            } else if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{:.0}", n)
            } else {
                n.to_string()
            };
            Ok(Value::String(s))
        }))),
    );

    proto.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Number(0.0));
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
    let proto_for_closure = Rc::clone(proto);
    let mut number_ctor = crate::value::NativeConstructor::new(
        move |args: Vec<Value>| {
            let n = args.first().map(to_number).unwrap_or(0.0);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::Number(n),
                );
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::Number);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_for_closure));
                }
                Ok(Value::Object(this_obj))
            } else {
                Ok(Value::Number(n))
            }
        },
        Rc::clone(proto),
    );
    number_ctor.set_name("Number");

    // Create number_obj and define all properties first
    let number_obj = Object::new(ObjectKind::Ordinary);
    let number_obj = Rc::new(RefCell::new(number_obj));
    number_obj.borrow_mut().define(
        "prototype",
        Value::Object(Rc::clone(proto)),
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
            value: None,
        },
    );

    // Number constants
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

    // Static methods
    let static_flags = PropertyFlags {
        writable: false,
        enumerable: false,
        configurable: false,
        value: None,
    };
    number_obj.borrow_mut().define(
        "isInteger",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            // Per spec: only returns true if arg is already Number type and integer
            Ok(Value::Boolean(matches!(
                args.first(),
                Some(Value::Number(n)) if n.is_finite() && n.fract() == 0.0
            )))
        }))),
        static_flags.clone(),
    );
    number_obj.borrow_mut().define(
        "isNaN",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            // Per spec: returns true only if arg is already Number type AND is NaN
            Ok(Value::Boolean(
                matches!(args.first(), Some(Value::Number(n)) if n.is_nan()),
            ))
        }))),
        static_flags.clone(),
    );
    number_obj.borrow_mut().define(
        "isFinite",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            // Per spec: only returns true if arg is already Number type and finite
            Ok(Value::Boolean(matches!(
                args.first(),
                Some(Value::Number(n)) if n.is_finite()
            )))
        }))),
        static_flags.clone(),
    );
    number_obj.borrow_mut().define(
        "isSafeInteger",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            // Per spec: only returns true if arg is already Number and safe integer
            Ok(Value::Boolean(matches!(
                args.first(),
                Some(Value::Number(n))
                    if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0
            )))
        }))),
        static_flags.clone(),
    );
    number_obj.borrow_mut().define(
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
        static_flags.clone(),
    );
    number_obj.borrow_mut().define(
        "parseFloat",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let s = args.first().map(to_js_string).unwrap_or_default();
            Ok(Value::Number(crate::builtins::date::spec_parse_float(&s)))
        }))),
        static_flags.clone(),
    );

    // Add static methods to number_ctor before it's moved
    number_ctor.set_static_method("isInteger", number_obj.borrow().get("isInteger").unwrap());
    number_ctor.set_static_method("isNaN", number_obj.borrow().get("isNaN").unwrap());
    number_ctor.set_static_method("isFinite", number_obj.borrow().get("isFinite").unwrap());
    number_ctor.set_static_method(
        "isSafeInteger",
        number_obj.borrow().get("isSafeInteger").unwrap(),
    );
    number_ctor.set_static_method("parseInt", number_obj.borrow().get("parseInt").unwrap());
    number_ctor.set_static_method("parseFloat", number_obj.borrow().get("parseFloat").unwrap());
    number_ctor.set_static_method("MAX_VALUE", Value::Number(f64::MAX));
    number_ctor.set_static_method("MIN_VALUE", Value::Number(f64::MIN_POSITIVE));
    number_ctor.set_static_method("NaN", Value::Number(f64::NAN));
    number_ctor.set_static_method("NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY));
    number_ctor.set_static_method("POSITIVE_INFINITY", Value::Number(f64::INFINITY));
    number_ctor.set_static_method("EPSILON", Value::Number(f64::EPSILON));
    number_ctor.set_static_method("MAX_SAFE_INTEGER", Value::Number(9007199254740991.0));
    number_ctor.set_static_method("MIN_SAFE_INTEGER", Value::Number(-9007199254740991.0));

    // Number.prototype.constructor = Number
    proto.borrow_mut().set(
        "constructor",
        Value::NativeConstructor(Rc::new(number_ctor.clone())),
    );

    // Set constructor on number_obj
    let number_fn = Value::NativeConstructor(Rc::new(number_ctor.clone()));
    number_obj.borrow_mut().define(
        "constructor",
        number_fn,
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
            value: None,
        },
    );

    // Set Number as the constructor function
    ctx.set_global(
        "Number".to_string(),
        Value::NativeConstructor(Rc::new(number_ctor)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> Value {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    fn eval_bool(src: &str) -> bool {
        match eval(src) {
            Value::Boolean(b) => b,
            other => panic!("expected boolean from {:?}, got {:?}", src, other),
        }
    }

    fn eval_num(src: &str) -> f64 {
        match eval(src) {
            Value::Number(n) => n,
            other => panic!("expected number from {:?}, got {:?}", src, other),
        }
    }

    #[test]
    fn test_number_constructor_returns_primitive() {
        let n = eval("Number(42)");
        assert!(matches!(n, Value::Number(42.0)));
    }

    #[test]
    fn test_number_string_not_a_number() {
        let n = eval("Number('Not-a-Number')");
        match n {
            Value::Number(n) => assert!(n.is_nan(), "Number('Not-a-Number') should be NaN"),
            _ => panic!("expected Number, got {:?}", n),
        }
    }

    #[test]
    fn test_number_is_nan_true() {
        // NaN itself
        assert!(eval_bool("Number.isNaN(NaN)"));
        // Number.NaN
        assert!(eval_bool("Number.isNaN(Number.NaN)"));
        // 0/0
        assert!(eval_bool("Number.isNaN(0/0)"));
        // Infinity/Infinity
        assert!(eval_bool("Number.isNaN(Infinity/Infinity)"));
        // Number("Not-a-Number") - string that doesn't parse as number
        assert!(eval_bool("Number.isNaN(Number('Not-a-Number'))"));
        // NaN * 0
        assert!(eval_bool("Number.isNaN(NaN * 0)"));
    }

    #[test]
    fn test_number_is_nan_false() {
        // Regular numbers
        assert!(!eval_bool("Number.isNaN(0)"));
        assert!(!eval_bool("Number.isNaN(1)"));
        assert!(!eval_bool("Number.isNaN(-1)"));
        assert!(!eval_bool("Number.isNaN(1.5)"));
        assert!(!eval_bool("Number.isNaN(Infinity)"));
        assert!(!eval_bool("Number.isNaN(-Infinity)"));
        // Strings that parse as numbers
        assert!(!eval_bool("Number.isNaN('42')"));
        assert!(!eval_bool("Number.isNaN('NaN')"));
        assert!(!eval_bool("Number.isNaN('123.456')"));
        // Other types
        assert!(!eval_bool("Number.isNaN(null)"));
        assert!(!eval_bool("Number.isNaN(undefined)"));
        assert!(!eval_bool("Number.isNaN({})"));
        assert!(!eval_bool("Number.isNaN([])"));
    }

    #[test]
    fn test_number_is_finite() {
        // Per spec: Number.isFinite does NOT coerce (unlike global isFinite)
        assert!(!eval_bool("Number.isFinite(NaN)"));
        assert!(!eval_bool("Number.isFinite(Infinity)"));
        assert!(!eval_bool("Number.isFinite(-Infinity)"));
        assert!(eval_bool("Number.isFinite(0)"));
        assert!(eval_bool("Number.isFinite(1)"));
        assert!(eval_bool("Number.isFinite(1.5)"));
        assert!(eval_bool("Number.isFinite(-1)"));
        // These are strings/objects — no coercion per spec → false
        assert!(!eval_bool("Number.isFinite('42')"));
        assert!(!eval_bool("Number.isFinite(null)"));
    }

    #[test]
    fn test_number_is_integer() {
        assert!(eval_bool("Number.isInteger(0)"));
        assert!(eval_bool("Number.isInteger(1)"));
        assert!(eval_bool("Number.isInteger(-1)"));
        assert!(eval_bool("Number.isInteger(1e10)"));
        assert!(!eval_bool("Number.isInteger(1.5)"));
        assert!(!eval_bool("Number.isInteger(NaN)"));
        assert!(!eval_bool("Number.isInteger(Infinity)"));
        assert!(!eval_bool("Number.isInteger('42')"));
        assert!(!eval_bool("Number.isInteger(null)"));
    }

    #[test]
    fn test_number_is_safe_integer() {
        // Per spec: no coercion
        assert!(eval_bool("Number.isSafeInteger(0)"));
        assert!(eval_bool("Number.isSafeInteger(1)"));
        assert!(eval_bool("Number.isSafeInteger(9007199254740991)"));
        assert!(eval_bool("Number.isSafeInteger(-9007199254740991)"));
        assert!(!eval_bool("Number.isSafeInteger(9007199254740992)")); // above max safe
        assert!(!eval_bool("Number.isSafeInteger(NaN)"));
        assert!(!eval_bool("Number.isSafeInteger(Infinity)"));
        assert!(!eval_bool("Number.isSafeInteger('42')"));
        assert!(!eval_bool("Number.isSafeInteger(null)"));
    }

    #[test]
    fn test_number_constants() {
        assert_eq!(eval_num("Number.MAX_VALUE"), f64::MAX);
        assert_eq!(eval_num("Number.MIN_VALUE"), f64::MIN_POSITIVE);
        let nan: f64 = eval_num("Number.NaN");
        assert!(nan.is_nan());
        assert_eq!(eval_num("Number.POSITIVE_INFINITY"), f64::INFINITY);
        assert_eq!(eval_num("Number.NEGATIVE_INFINITY"), f64::NEG_INFINITY);
    }

    #[test]
    fn test_number_parse_float() {
        assert_eq!(eval_num("Number.parseFloat('123.456')"), 123.456);
        assert_eq!(eval_num("Number.parseFloat('42')"), 42.0);
        assert_eq!(eval_num("Number.parseFloat('3.14abc')"), PI);
        let nan: f64 = eval_num("Number.parseFloat('not a number')");
        assert!(nan.is_nan());
    }

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
        let nan_result = proto_to_fixed_impl(vec![]);
        assert!(nan_result.is_ok());
    }

    #[test]
    fn test_number_prototype_to_string_unwraps_boxed_value() {
        // Per ECMA-262, Number.prototype.toString returns ToString(thisNumberValue),
        // not "[object Number]" (which is what Object.prototype.toString yields).
        let s = eval("new Number(-1.1).toString()");
        assert_eq!(s, Value::String("-1.1".to_string()));
        assert_eq!(
            eval("new Number(42).toString()"),
            Value::String("42".to_string())
        );
        assert_eq!(
            eval("String(new Number(-1.1))"),
            Value::String("-1.1".to_string())
        );
        // parseFloat on a boxed Number must equal parseFloat on the unwrapped string.
        assert_eq!(eval_num("parseFloat(new Number(-1.1))"), -1.1);
        assert!(eval_num("parseFloat(new Number(Infinity))").is_infinite());
        assert!(eval_num("parseFloat(new Number(-Infinity))").is_infinite());
        assert!(eval_num("parseFloat(new Number(-Infinity))").is_sign_negative());
    }
}
