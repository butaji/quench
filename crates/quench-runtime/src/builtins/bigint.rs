//! BigInt built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::operators::parse_bigint_string;
use crate::value::{
    create_js_error_with_type, to_number, to_primitive, NativeFunction, Object, ObjectKind,
    PropertyFlags, Value,
};
use crate::Context;

use num_bigint::BigInt;

const BIGINT_TO_INDEX_MAX: f64 = 9007199254740991.0;

/// Configure a function-like built-in method on prototype/objects.
fn method_flags() -> PropertyFlags {
    PropertyFlags {
        writable: true,
        enumerable: false,
        configurable: true,
        value: None,
    }
}

fn set_function_metadata(function: &NativeFunction, name: &str, length: f64) {
    function.define_property(
        "name",
        Value::String(name.to_string()),
        PropertyFlags {
            value: Some(Value::String(name.to_string())),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    function.define_property(
        "length",
        Value::Number(length),
        PropertyFlags {
            value: Some(Value::Number(length)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
}

fn bigint_to_index(bits_arg: Option<&Value>) -> Result<u64, crate::value::JsError> {
    let value = bits_arg.unwrap_or(&Value::Undefined);
    let primitive = match value {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => to_primitive(value, Some("number"))?,
        _ => value.clone(),
    };
    if let Value::BigInt(_) = primitive {
        let (_, err) = create_js_error_with_type("Cannot convert BigInt to Number", "TypeError");
        return Err(err);
    }
    if let Value::Symbol(_) = primitive {
        let (_, err) = create_js_error_with_type("Cannot convert symbol to Number", "TypeError");
        return Err(err);
    }

    let number = to_number(&primitive);
    let integer_index = if number.is_nan() { 0.0 } else { number.trunc() };
    if !integer_index.is_finite() || integer_index < 0.0 || integer_index > BIGINT_TO_INDEX_MAX {
        let (_, err) = create_js_error_with_type("Cannot convert to index", "RangeError");
        return Err(err);
    }
    Ok(integer_index as u64)
}

fn bigint_to_integer_or_infinity(
    value: Option<&Value>,
    default_to_ten: bool,
) -> Result<f64, crate::value::JsError> {
    let value = value.unwrap_or(&Value::Undefined);
    let primitive = match value {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => to_primitive(value, Some("number"))?,
        _ => value.clone(),
    };
    if let Value::BigInt(_) = primitive {
        let (_, err) = create_js_error_with_type("Cannot convert BigInt to Number", "TypeError");
        return Err(err);
    }
    if let Value::Symbol(_) = primitive {
        let (_, err) = create_js_error_with_type("Cannot convert symbol to Number", "TypeError");
        return Err(err);
    }

    let mut number = to_number(&primitive);
    if number.is_nan() && default_to_ten {
        number = 10.0;
    }
    Ok(number.trunc())
}

/// Convert an optional BigInt constructor argument to bigint.
fn bigint_constructor_argument(value: Option<&Value>) -> Result<BigInt, crate::value::JsError> {
    match value {
        Some(value) => to_bigint_value(value),
        None => {
            let (_, err) =
                create_js_error_with_type("Cannot convert undefined to BigInt", "TypeError");
            Err(err)
        }
    }
}

fn bigint_argument(value: Option<&Value>) -> Result<BigInt, crate::value::JsError> {
    let value = value.unwrap_or(&Value::Undefined);
    let primitive = match value {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => to_primitive(value, Some("number"))?,
        _ => value.clone(),
    };
    if matches!(primitive, Value::Number(_) | Value::Null) {
        let (_, err) = create_js_error_with_type("Cannot convert value to BigInt", "TypeError");
        return Err(err);
    }
    to_bigint_value(&primitive)
}

fn this_bigint_value(value: &Value) -> Result<BigInt, crate::value::JsError> {
    match value {
        Value::BigInt(_) => to_bigint_value(value),
        Value::Object(object)
            if object.borrow().exotic_kind == Some(crate::value::kind::ExoticKind::BigInt) =>
        {
            to_bigint_value(value)
        }
        _ => {
            let (_, err) = create_js_error_with_type("this value is not a BigInt", "TypeError");
            Err(err)
        }
    }
}

pub fn bigint_to_f64(bi: &BigInt) -> f64 {
    if bi >= &BigInt::from(i64::MAX) {
        f64::INFINITY
    } else if bi <= &BigInt::from(i64::MIN) {
        f64::NEG_INFINITY
    } else {
        bi.to_string().parse::<f64>().unwrap_or(0.0)
    }
}

fn bigint_as_int_n(n: &BigInt, bits: u64) -> Value {
    if bits == 0 {
        return Value::BigInt(Rc::new(BigInt::from(0)));
    }
    let bits_usize = bits as usize;
    let one: BigInt = BigInt::from(1);
    let mask = (one.clone() << bits_usize) - BigInt::from(1);
    let masked = n & mask;
    let half_mask = one.clone() << (bits_usize - 1);

    if masked >= half_mask {
        let value = masked - (one << bits_usize);
        Value::BigInt(Rc::new(value))
    } else {
        Value::BigInt(Rc::new(masked))
    }
}

fn bigint_as_uint_n(n: &BigInt, bits: u64) -> Value {
    if bits == 0 {
        return Value::BigInt(Rc::new(BigInt::from(0)));
    }
    let bits_usize = bits as usize;
    let mask = (BigInt::from(1) << bits_usize) - BigInt::from(1);
    let masked = n & mask;
    Value::BigInt(Rc::new(masked))
}

/// Convert a BigInt to a JS Value
pub fn bigint_to_value(bi: BigInt) -> Value {
    Value::BigInt(Rc::new(bi))
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
    let to_string_fn = Rc::new(NativeFunction::new(|args: Vec<Value>| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let bi = this_bigint_value(&this_val)?;
        let radix = bigint_to_integer_or_infinity(args.first(), true)?;
        if radix < 2.0 || radix > 36.0 {
            let (_, err) = create_js_error_with_type("Cannot convert radix to range", "RangeError");
            return Err(err);
        }
        let radix = u32::try_from(radix as i64).unwrap_or(10);
        Ok(Value::String(bi.to_str_radix(radix)))
    }));
    set_function_metadata(&to_string_fn, "toString", 0.0);
    proto.borrow_mut().define(
        "__toString",
        Value::NativeFunction(to_string_fn),
        method_flags(),
    );

    let value_of_fn = Rc::new(NativeFunction::new(|_args: Vec<Value>| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let bi = this_bigint_value(&this_val)?;
        Ok(Value::BigInt(Rc::new(bi)))
    }));
    set_function_metadata(&value_of_fn, "valueOf", 0.0);
    proto.borrow_mut().define(
        "__valueOf",
        Value::NativeFunction(value_of_fn),
        method_flags(),
    );

    if let Some(Value::Symbol(sym)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        proto.borrow_mut().define(
            &sym.property_key(),
            Value::String("BigInt".to_string()),
            PropertyFlags {
                writable: false,
                enumerable: false,
                configurable: true,
                value: None,
            },
        );
    }
}

fn setup_bigint_static(proto: &Rc<RefCell<Object>>, ctx: &mut Context) {
    // BigInt() constructor
    let proto_for_closure = Rc::clone(proto);
    let bigint_ctor = Rc::new(crate::value::NativeConstructor::new(
        move |args: Vec<Value>| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);

            // If called as a constructor (new BigInt), it must create a BigInt object.
            if let Value::Object(this_obj) = &this_val {
                let result = bigint_constructor_argument(args.first())?;
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
                let result = bigint_constructor_argument(args.first())?;
                Ok(bigint_to_value(result))
            }
        },
        Rc::clone(proto),
    ));
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

    let as_int_n = Rc::new(NativeFunction::new(|args| {
        let bits = bigint_to_index(args.first())?;
        let bigint = bigint_argument(args.get(1))?;
        Ok(bigint_as_int_n(&bigint, bits))
    }));
    set_function_metadata(&as_int_n, "asIntN", 2.0);
    let as_uint_n = Rc::new(NativeFunction::new(|args| {
        let bits = bigint_to_index(args.first())?;
        let bigint = bigint_argument(args.get(1))?;
        Ok(bigint_as_uint_n(&bigint, bits))
    }));
    set_function_metadata(&as_uint_n, "asUintN", 2.0);

    let static_flags = PropertyFlags {
        writable: false,
        enumerable: false,
        configurable: false,
        value: None,
    };
    bigint_obj.borrow_mut().define(
        "__asIntN",
        Value::NativeFunction(Rc::clone(&as_int_n)),
        static_flags.clone(),
    );
    bigint_obj.borrow_mut().define(
        "__asUintN",
        Value::NativeFunction(Rc::clone(&as_uint_n)),
        static_flags,
    );

    bigint_ctor.set_static_method("__asIntN", bigint_obj.borrow().get("__asIntN").unwrap());
    bigint_ctor.set_static_method("__asUintN", bigint_obj.borrow().get("__asUintN").unwrap());
    let bigint_ctor_value = Value::NativeConstructor(Rc::clone(&bigint_ctor));

    // BigInt.prototype.constructor = BigInt
    proto.borrow_mut().define(
        "constructor",
        bigint_ctor_value.clone(),
        PropertyFlags {
            writable: true,
            enumerable: false,
            configurable: true,
            value: None,
        },
    );

    bigint_obj.borrow_mut().define(
        "constructor",
        bigint_ctor_value.clone(),
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
            value: None,
        },
    );

    // Set BigInt as the constructor function
    ctx.set_global("BigInt".to_string(), bigint_ctor_value.clone());
}

/// Convert a JS value to a BigInt
pub fn to_bigint_value(val: &Value) -> Result<BigInt, crate::value::JsError> {
    match val {
        Value::BigInt(bi) => Ok(bi.as_ref().clone()),
        Value::Number(n) => {
            if !n.is_finite() || n.fract() != 0.0 {
                let (_, err) =
                    create_js_error_with_type("Cannot convert number to BigInt", "RangeError");
                return Err(err);
            }
            Ok(BigInt::from(*n as i64))
        }
        Value::String(s) => match parse_bigint_string(s) {
            Some(bi) => Ok(bi),
            None => {
                let (_, err) =
                    create_js_error_with_type("Cannot parse string to BigInt", "SyntaxError");
                Err(err)
            }
        },
        Value::Boolean(b) => Ok(if *b { BigInt::from(1) } else { BigInt::from(0) }),
        Value::Null => {
            let (_, err) = create_js_error_with_type("Cannot convert null to BigInt", "TypeError");
            Err(err)
        }
        Value::Undefined => {
            let (_, err) =
                create_js_error_with_type("Cannot convert undefined to BigInt", "TypeError");
            Err(err)
        }
        Value::Object(object)
            if object.borrow().exotic_kind == Some(crate::value::kind::ExoticKind::BigInt) =>
        {
            match object.borrow().get_own_value("_value") {
                Some(Value::BigInt(bi)) => Ok(bi.as_ref().clone()),
                _ => Err(crate::JsError::from("TypeError: not a BigInt object")),
            }
        }
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> Value {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    #[test]
    fn test_bigint_constructor_empty_and_spaces() {
        assert_eq!(eval("BigInt('')"), Value::BigInt(Rc::new(BigInt::from(0))));
        assert_eq!(
            eval("BigInt('   ')"),
            Value::BigInt(Rc::new(BigInt::from(0)))
        );
        assert_eq!(
            eval("BigInt('   -197   ')"),
            Value::BigInt(Rc::new(BigInt::from(-197)))
        );
        assert_eq!(
            eval("BigInt('0xa')"),
            Value::BigInt(Rc::new(BigInt::from(10)))
        );
    }

    #[test]
    fn test_bigint_constructor_syntax_error_for_invalid_strings() {
        assert_eq!(
            eval("try { BigInt('10n'); 'ok'; } catch (e) { e.name }"),
            Value::String("SyntaxError".to_string())
        );
        assert_eq!(
            eval("try { BigInt('-0x1'); 'ok'; } catch (e) { e.name }"),
            Value::String("SyntaxError".to_string())
        );
    }

    #[test]
    fn test_bigint_constructor_number_errors() {
        assert_eq!(
            eval("try { BigInt(NaN); 'ok'; } catch (e) { e.name }"),
            Value::String("RangeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt(1.1); 'ok'; } catch (e) { e.name }"),
            Value::String("RangeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt(Infinity); 'ok'; } catch (e) { e.name }"),
            Value::String("RangeError".to_string())
        );
    }

    #[test]
    fn test_bigint_constructor_number_and_no_args() {
        assert_eq!(
            eval("BigInt(Number.MAX_SAFE_INTEGER)"),
            Value::BigInt(Rc::new(BigInt::from(9007199254740991i64)))
        );
        assert_eq!(eval("BigInt(10)"), Value::BigInt(Rc::new(BigInt::from(10))));
        assert_eq!(
            eval("try { BigInt(); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_bigint_static_methods_reject_null() {
        assert_eq!(
            eval("try { BigInt.asIntN(0, null); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_bigint_static_methods_reject_numbers() {
        assert_eq!(
            eval("try { BigInt.asIntN(0, 0); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_bigint_constructor_uses_value_of_for_objects() {
        assert_eq!(
            eval("BigInt({ valueOf() { return 44; }, toString() { throw new Error(); } })"),
            Value::BigInt(Rc::new(BigInt::from(44)))
        );
    }

    #[test]
    fn test_bigint_constructor_length() {
        assert_eq!(eval("BigInt.length"), Value::Number(1.0));
    }

    #[test]
    fn test_bigint_constructor_name_is_configurable() {
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt, 'name').configurable"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn test_bigint_constructor_inherits_function_prototype() {
        assert_eq!(
            eval("Object.getPrototypeOf(BigInt) === Function.prototype"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn test_bigint_prototype_property_is_not_writable() {
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt, 'prototype').writable"),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_bigint_prototype_methods_reject_non_bigints() {
        assert_eq!(
            eval("try { BigInt.prototype.valueOf.call(false); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_bigint_wrapper_string_hint_uses_to_string_accessor() {
        assert_eq!(
            eval("let f = BigInt.prototype.toString; Object.defineProperty(BigInt.prototype, 'toString', { get() { return function() { return f.call(this) + 'foo'; }; } }); `${Object(1n)}`"),
            Value::String("1foo".to_string())
        );
    }

    #[test]
    fn test_bigint_wrapper_uses_bigint_prototype() {
        assert_eq!(
            eval("Object.getPrototypeOf(Object(1n)) === BigInt.prototype"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn test_number_plus_bigint_wrapper_throws() {
        assert_eq!(
            eval("try { 1 + Object(1n); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_number_plus_mutated_bigint_wrapper_throws() {
        assert_eq!(
            eval("let f = BigInt.prototype.valueOf; let v = function() { return f.call(this) * 2n; }; Object.defineProperty(BigInt.prototype, 'valueOf', { get() { return v; } }); try { 1 + Object(1n); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_number_plus_bigint_wrapper_throws_after_ordinary_to_primitive_sequence() {
        assert_eq!(
            eval("let s = BigInt.prototype.toString; let v = BigInt.prototype.valueOf; Object.defineProperty(BigInt.prototype, 'toString', { get() { return undefined; } }); Object.defineProperty(BigInt.prototype, 'valueOf', { get() { return function() { return v.call(this) * 2n; }; } }); try { 1 + Object(1n); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_bigint_constructor_static_property_descriptors() {
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt, 'length').value"),
            Value::Number(1.0)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt, 'name').value"),
            Value::String("BigInt".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt, 'prototype').writable"),
            Value::Boolean(false)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt, 'prototype').configurable"),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_bigint_prototype_to_string_and_length_name() {
        assert_eq!(
            eval("try { BigInt.prototype.toString(); 'ok' } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype, 'toString').writable"),
            Value::Boolean(true)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype.toString, 'name').value"),
            Value::String("toString".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype.toString, 'length').value"),
            Value::Number(0.0)
        );
    }

    #[test]
    fn test_bigint_prototype_methods_and_string_tag() {
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype, 'valueOf').writable"),
            Value::Boolean(true)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype.valueOf, 'length').value"),
            Value::Number(0.0)
        );
        assert_eq!(
            eval("BigInt.prototype[Symbol.toStringTag]"),
            Value::String("BigInt".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype, Symbol.toStringTag).writable"),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_bigint_prototype_constructor_descriptor() {
        assert_eq!(
            eval(
                "Object.getOwnPropertyDescriptor(BigInt.prototype, 'constructor').value === BigInt"
            ),
            Value::Boolean(true)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype, 'constructor').writable"),
            Value::Boolean(true)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype, 'constructor').enumerable"),
            Value::Boolean(false)
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.prototype, 'constructor').configurable"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn test_bigint_to_string() {
        assert_eq!(
            eval("(0n).toString(undefined)"),
            Value::String("0".to_string())
        );
        assert_eq!(
            eval("(-100n).toString(16)"),
            Value::String("-64".to_string())
        );
        assert_eq!(eval("(100n).toString(36)"), Value::String("2s".to_string()));
        assert_eq!(eval("(100n).toString()"), Value::String("100".to_string()));
        assert_eq!(
            eval("try { (0n).toString(Symbol()); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
        assert_eq!(
            eval("try { (0n).toString(0); 'ok'; } catch (e) { e.name }"),
            Value::String("RangeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt.prototype.toString.call({ valueOf() { return 1n; } }); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
    }

    #[test]
    fn test_bigint_asintn_properties() {
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.asIntN, 'name').value"),
            Value::String("asIntN".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.asIntN, 'length').value"),
            Value::Number(2.0)
        );
    }

    #[test]
    fn test_bigint_asuintn_properties() {
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.asUintN, 'name').value"),
            Value::String("asUintN".to_string())
        );
        assert_eq!(
            eval("Object.getOwnPropertyDescriptor(BigInt.asUintN, 'length').value"),
            Value::Number(2.0)
        );
    }

    #[test]
    fn test_bigint_as_int_and_uint_n() {
        assert_eq!(
            eval("BigInt.asIntN(2, 256n)"),
            Value::BigInt(Rc::new(BigInt::from(0)))
        );
        assert_eq!(
            eval("BigInt.asIntN(8, 0xabn)"),
            Value::BigInt(Rc::new(BigInt::from(-85)))
        );
        assert_eq!(
            eval("BigInt.asUintN(8, 0xabn)"),
            Value::BigInt(Rc::new(BigInt::from(171)))
        );
        assert_eq!(
            eval("try { BigInt.asIntN(); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt.asIntN(-1, 0n); 'ok'; } catch (e) { e.name }"),
            Value::String("RangeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt.asIntN(9007199254740992, 0n); 'ok'; } catch (e) { e.name }"),
            Value::String("RangeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt.asIntN(0n, 0n); 'ok'; } catch (e) { e.name }"),
            Value::String("TypeError".to_string())
        );
        assert_eq!(
            eval("try { BigInt.asIntN(2, '1n'); 'ok'; } catch (e) { e.name }"),
            Value::String("SyntaxError".to_string())
        );
    }
}
