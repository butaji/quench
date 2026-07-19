//! BigInt built-in

use std::cell::RefCell;
use std::rc::Rc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::value::{
    create_js_error_with_type, to_number, to_primitive, NativeFunction, Object, ObjectKind,
    PropertyFlags, Value,
};
use crate::Context;

/// Convert a BigInt to a JS Value
pub fn bigint_to_value(bi: BigInt) -> Value {
    Value::BigInt(Rc::new(bi))
}

/// Convert a number to BigInt
pub fn n_to_bigint(n: f64) -> BigInt {
    BigInt::from(n as i64)
}

/// Convert a BigInt to f64 (for loose equality comparison)
pub fn bigint_to_f64(bi: &BigInt) -> f64 {
    // Check if BigInt fits in i64
    if let Some(i) = bi.to_i64() {
        i as f64
    } else {
        // For very large BigInts, return infinity (they can't equal a Number)
        if bi >= &BigInt::from(0) {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }
    }
}

// ============================================================================
// BigInt
// ============================================================================

pub fn register_bigint(ctx: &mut Context) {
    let bigint_proto = Object::new(ObjectKind::Ordinary);
    let bigint_proto_rc = Rc::new(RefCell::new(bigint_proto));

    setup_bigint_prototype(&bigint_proto_rc);

    // BigInt.prototype must inherit from Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        bigint_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    setup_bigint_static(&bigint_proto_rc, ctx);
}

fn setup_bigint_prototype(proto: &Rc<RefCell<Object>>) {
    proto.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this()
                .unwrap_or(Value::BigInt(Rc::new(BigInt::from(0))));
            let bi = to_bigint_value(&this_val)?;
            Ok(Value::String(format!("{}n", bi)))
        }))),
    );

    proto.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this()
                .unwrap_or(Value::BigInt(Rc::new(BigInt::from(0))));
            let bi = to_bigint_value(&this_val)?;
            Ok(Value::BigInt(Rc::new(bi)))
        }))),
    );
}

fn setup_bigint_static(proto: &Rc<RefCell<Object>>, ctx: &mut Context) {
    // BigInt() constructor
    let proto_for_closure = Rc::clone(proto);
    let mut bigint_ctor = crate::value::NativeConstructor::new(
        move |args: Vec<Value>| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);

            // If called as a constructor (new BigInt), it must create a BigInt object
            if let Value::Object(this_obj) = &this_val {
                let result = to_bigint_from_args(&args)?;
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    bigint_to_value(result),
                );
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::BigInt);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&proto_for_closure));
                }
                Ok(Value::Object(this_obj.clone()))
            } else {
                // Called as a function: return primitive BigInt
                let result = to_bigint_from_args(&args)?;
                Ok(bigint_to_value(result))
            }
        },
        Rc::clone(proto),
    );
    bigint_ctor.set_name("BigInt");

    // Create bigint_obj and define all properties first
    let bigint_obj = Object::new(ObjectKind::Ordinary);
    let bigint_obj = Rc::new(RefCell::new(bigint_obj));
    bigint_obj.borrow_mut().define(
        "prototype",
        Value::Object(Rc::clone(proto)),
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
            value: None,
        },
    );

    // Static properties
    let static_flags = PropertyFlags {
        writable: false,
        enumerable: false,
        configurable: false,
        value: None,
    };

    // BigInt.asIntN(bits, bigint) - static method
    bigint_obj.borrow_mut().define(
        "asIntN",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let bits = args.first().map(to_number).unwrap_or(0.0);
            let n = args.get(1).map(to_bigint_value).transpose()?;
            let n = n.unwrap_or_else(|| BigInt::from(0));
            Ok(bigint_as_int_n(&n, bits))
        }))),
        static_flags.clone(),
    );

    // BigInt.asUintN(bits, bigint) - static method
    bigint_obj.borrow_mut().define(
        "asUintN",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let bits = args.first().map(to_number).unwrap_or(0.0);
            let n = args.get(1).map(to_bigint_value).transpose()?;
            let n = n.unwrap_or_else(|| BigInt::from(0));
            Ok(bigint_as_uint_n(&n, bits))
        }))),
        static_flags.clone(),
    );

    // Add static methods to bigint_ctor before it's moved
    bigint_ctor.set_static_method("asIntN", bigint_obj.borrow().get("asIntN").unwrap());
    bigint_ctor.set_static_method("asUintN", bigint_obj.borrow().get("asUintN").unwrap());

    // BigInt.prototype.constructor = BigInt
    proto.borrow_mut().set(
        "constructor",
        Value::NativeConstructor(Rc::new(bigint_ctor.clone())),
    );

    // Set constructor on bigint_obj
    let bigint_fn = Value::NativeConstructor(Rc::new(bigint_ctor.clone()));
    bigint_obj.borrow_mut().define(
        "constructor",
        bigint_fn,
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
            value: None,
        },
    );

    // Set BigInt as the constructor function
    ctx.set_global(
        "BigInt".to_string(),
        Value::NativeConstructor(Rc::new(bigint_ctor)),
    );
}

/// Convert a JS value to a BigInt
pub fn to_bigint_value(val: &Value) -> Result<BigInt, crate::value::JsError> {
    match val {
        Value::BigInt(bi) => Ok(bi.as_ref().clone()),
        Value::Number(n) => {
            if n.is_nan() || n.is_infinite() || n.fract() != 0.0 {
                let (_, err) =
                    create_js_error_with_type("Cannot convert number to BigInt", "TypeError");
                Err(err)
            } else {
                Ok(BigInt::from(*n as i64))
            }
        }
        Value::String(s) => {
            let s = s.trim();
            // Remove trailing 'n' if present
            let s = s.strip_suffix('n').unwrap_or(s);
            match s.parse::<BigInt>() {
                Ok(bi) => Ok(bi),
                Err(_) => {
                    let (_, err) = create_js_error_with_type(
                        &format!("Invalid BigInt value: {}", s),
                        "TypeError",
                    );
                    Err(err)
                }
            }
        }
        Value::Boolean(b) => Ok(if *b { BigInt::from(1) } else { BigInt::from(0) }),
        Value::Null => Ok(BigInt::from(0)),
        Value::Undefined => {
            let (_, err) =
                create_js_error_with_type("Cannot convert undefined to BigInt", "TypeError");
            Err(err)
        }
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_) => {
            let prim = to_primitive(val, Some("number"))?;
            to_bigint_value(&prim)
        }
        Value::Symbol(_) => {
            let (_, err) =
                create_js_error_with_type("Cannot convert Symbol to BigInt", "TypeError");
            Err(err)
        }
    }
}

/// Convert arguments to a BigInt value (for BigInt constructor)
fn to_bigint_from_args(args: &[Value]) -> Result<BigInt, crate::value::JsError> {
    if args.is_empty() {
        return Ok(BigInt::from(0));
    }
    to_bigint_value(&args[0])
}

/// BigInt.asIntN - convert BigInt to signed integer with specified bit width
fn bigint_as_int_n(n: &BigInt, bits: f64) -> Value {
    let bits_int = bits as u32;
    if bits_int == 0 {
        return Value::BigInt(Rc::new(BigInt::from(0)));
    }

    let mask = (BigInt::from(1) << bits_int) - BigInt::from(1);
    let masked = n & mask;

    // If the high bit is set, interpret as negative
    let half_mask = BigInt::from(1) << (bits_int - 1);
    if masked >= half_mask.clone() {
        let result = masked - (BigInt::from(1) << bits_int);
        Value::BigInt(Rc::new(result))
    } else {
        Value::BigInt(Rc::new(masked))
    }
}

/// BigInt.asUintN - convert BigInt to unsigned integer with specified bit width
fn bigint_as_uint_n(n: &BigInt, bits: f64) -> Value {
    let bits_int = bits as u32;
    if bits_int == 0 {
        return Value::BigInt(Rc::new(BigInt::from(0)));
    }

    let mask = (BigInt::from(1) << bits_int) - BigInt::from(1);
    let masked = n & mask;
    Value::BigInt(Rc::new(masked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> Value {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    #[test]
    fn test_bigint_literal() {
        let result = eval("123n");
        assert!(matches!(result, Value::BigInt(_)));
    }

    #[test]
    fn test_bigint_constructor() {
        let result = eval("BigInt(42)");
        assert!(matches!(result, Value::BigInt(_)));
    }

    #[test]
    fn test_bigint_as_int_n() {
        let result = eval("BigInt.asIntN(8, 256n)");
        // 256 = 0x100, with 8 bits should wrap to 0
        assert!(matches!(result, Value::BigInt(_)));
    }

    #[test]
    fn test_bigint_as_uint_n() {
        let result = eval("BigInt.asUintN(8, 256n)");
        // 256 = 0x100, with 8 bits should wrap to 0
        assert!(matches!(result, Value::BigInt(_)));
    }

    #[test]
    fn test_bigint_to_string() {
        let result = eval("(123n).toString()");
        assert_eq!(result, Value::String("123n".to_string()));
    }
}
