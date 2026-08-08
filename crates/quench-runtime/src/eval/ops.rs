//! Canonical ECMAScript abstract operations (spec ops).
//!
//! This module is the single source of truth for spec abstract operations.
//! All `eval/` nodes and JS builtins must use these, not local copies.
//!
//! Ops are exposed on `%ops%` (the JS-Rust bridge) so JS builtins can call them.
//! New op: add here → expose on `%ops%` → use from JS.

// Re-export the canonical implementations from their homes.
pub use crate::builtins::object_static::to_property_key;
pub use crate::value::coerce::to_js_string as to_string; // ToString (§7.1.12)
pub use crate::value::coerce::to_number;
pub use crate::value::compare::same_value; // SameValue (§7.2.9)
pub use crate::value::primitive::to_object; // ToObject (§7.1.13)
pub use crate::value::primitive::to_primitive;
pub use crate::value::primitive::PrimitiveHint;

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{Object, Value};

/// PreferredType for ToPrimitive hint (ES spec §7.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferredType {
    Default,
    Number,
    String,
}

impl PreferredType {
    pub fn as_option_str(&self) -> Option<&'static str> {
        match self {
            PreferredType::Default => None,
            PreferredType::Number => Some("number"),
            PreferredType::String => Some("string"),
        }
    }
}

/// SameValueZero (§7.2.10): like SameValue but +0 and -0 compare equal.
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    if let (Value::Number(ai), Value::Number(bi)) = (a, b) {
        if *ai == 0.0 && *bi == 0.0 {
            return true;
        }
    }
    same_value(a, b)
}

/// IsCallable (§7.2.3).
pub fn is_callable(v: &Value) -> bool {
    v.is_callable()
}

/// IsConstructor (§7.2.4).
pub fn is_constructor(v: &Value) -> bool {
    crate::eval::class::helpers::is_constructor_value(v)
}

/// OrdinaryHasOwnProperty (§7.3.2): whether `o` has an own property `key`.
pub fn has_own(o: &Rc<RefCell<Object>>, key: &str) -> bool {
    o.borrow().has_own(key)
}

/// IsExtensible (§9.1.3): whether `v` is extensible. Functions/classes are
/// always extensible; primitives are never extensible.
pub fn is_extensible(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.borrow().extensible,
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => true,
        Value::Class(c) => c.is_extensible(),
        _ => false,
    }
}

/// Throw a TypeError from a JS builtin.
pub fn throw_type_error(msg: &str) -> crate::JsError {
    crate::value::error::create_js_error_with_type(msg, "TypeError").1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    // ── to_primitive tests ─────────────────────────────────────────────────────

    #[test]
    fn test_ops_to_primitive_primitives() {
        assert_eq!(
            to_primitive(&Value::Undefined, None).unwrap(),
            Value::Undefined
        );
        assert_eq!(to_primitive(&Value::Null, None).unwrap(), Value::Null);
        assert_eq!(
            to_primitive(&Value::Boolean(true), None).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            to_primitive(&Value::Number(42.0), None).unwrap(),
            Value::Number(42.0)
        );
        assert_eq!(
            to_primitive(&Value::String("hello".into()), None).unwrap(),
            Value::String("hello".into())
        );
    }

    #[test]
    fn test_ops_to_primitive_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Object with valueOf returning a primitive
        let result = ctx.eval("var o = { valueOf() { return 99 } }; o").unwrap();
        let prim = to_primitive(&result, Some("number")).unwrap();
        assert_eq!(prim, Value::Number(99.0));
    }

    #[test]
    fn test_ops_to_primitive_hint_order() {
        let mut ctx = crate::Context::new().unwrap();
        // With number hint: valueOf is tried first
        let result = ctx
            .eval("var o = { valueOf() { return 1 }, toString() { return 'a' } }; o")
            .unwrap();
        let prim = to_primitive(&result, Some("number")).unwrap();
        assert_eq!(prim, Value::Number(1.0));

        // With string hint: toString is tried first
        let prim = to_primitive(&result, Some("string")).unwrap();
        assert_eq!(prim, Value::String("a".to_string()));
    }

    #[test]
    fn test_ops_to_primitive_symbol_key() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("var o = { [Symbol.toPrimitive](hint) { return 'symPrim'; } }; o")
            .unwrap();
        let prim = to_primitive(&result, Some("string")).unwrap();
        assert_eq!(prim, Value::String("symPrim".to_string()));
    }

    // ── to_number tests ────────────────────────────────────────────────────────

    #[test]
    fn test_ops_to_number_primitives() {
        assert!(to_number(&Value::Undefined).is_nan());
        assert_eq!(to_number(&Value::Null), 0.0);
        assert_eq!(to_number(&Value::Boolean(false)), 0.0);
        assert_eq!(to_number(&Value::Boolean(true)), 1.0);
        assert_eq!(to_number(&Value::Number(42.5)), 42.5);
        assert!(to_number(&Value::Number(f64::NAN)).is_nan());
        assert_eq!(to_number(&Value::Number(f64::INFINITY)), f64::INFINITY);
        assert_eq!(
            to_number(&Value::Number(f64::NEG_INFINITY)),
            f64::NEG_INFINITY
        );
    }

    #[test]
    fn test_ops_to_number_strings() {
        assert_eq!(to_number(&Value::String("42".into())), 42.0);
        assert_eq!(to_number(&Value::String("-3".into())), -3.0);
        assert_eq!(to_number(&Value::String("".into())), 0.0);
        assert_eq!(to_number(&Value::String("   ".into())), 0.0);
        assert!(to_number(&Value::String("x".into())).is_nan());
        assert_eq!(to_number(&Value::String("Infinity".into())), f64::INFINITY);
        assert!(to_number(&Value::String("NaN".into())).is_nan());
    }

    #[test]
    fn test_ops_to_number_hex_octal_bin() {
        assert_eq!(to_number(&Value::String("0x10".into())), 16.0);
        assert_eq!(to_number(&Value::String("0XFF".into())), 255.0);
        assert_eq!(to_number(&Value::String("0b1010".into())), 10.0);
        assert_eq!(to_number(&Value::String("0B111".into())), 7.0);
        assert_eq!(to_number(&Value::String("0o77".into())), 63.0);
        assert_eq!(to_number(&Value::String("0O10".into())), 8.0);
    }

    #[test]
    fn test_ops_to_number_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Object converts via ToPrimitive
        let result = ctx.eval("var o = { valueOf() { return 17 } }; o").unwrap();
        assert_eq!(to_number(&result), 17.0);
    }

    // ── to_property_key tests ──────────────────────────────────────────────────

    #[test]
    fn test_ops_to_property_key_strings() {
        assert_eq!(
            to_property_key(&Value::String("foo".into())).unwrap(),
            "foo"
        );
        assert_eq!(to_property_key(&Value::Number(42.0)).unwrap(), "42");
        assert_eq!(to_property_key(&Value::Boolean(true)).unwrap(), "true");
    }

    #[test]
    fn test_ops_to_property_key_symbol() {
        use std::rc::Rc;
        let a = Value::Symbol(Rc::new(crate::value::Symbol::new(
            Some("myKey".into()),
            false,
        )));
        let b = Value::Symbol(Rc::new(crate::value::Symbol::new(
            Some("myKey".into()),
            false,
        )));
        let ka = to_property_key(&a).unwrap();
        let kb = to_property_key(&b).unwrap();
        assert!(ka.starts_with("myKey\0"));
        assert_ne!(ka, kb);
    }

    #[test]
    fn test_ops_to_property_key_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Objects convert via ToPrimitive(string)
        let result = ctx
            .eval("var o = { toString() { return 'propKey' } }; o")
            .unwrap();
        assert_eq!(to_property_key(&result).unwrap(), "propKey");
    }

    // ── PreferredType tests ────────────────────────────────────────────────────

    #[test]
    fn test_preferred_type_to_option() {
        assert_eq!(PreferredType::Default.as_option_str(), None);
        assert_eq!(PreferredType::Number.as_option_str(), Some("number"));
        assert_eq!(PreferredType::String.as_option_str(), Some("string"));
    }

    // ── same_value_zero (§7.2.10) ───────────────────────────────────────────────

    #[test]
    fn test_same_value_zero_plus_minus_zero() {
        // SameValueZero treats +0 and -0 as equal (unlike SameValue).
        assert!(same_value_zero(&Value::Number(0.0), &Value::Number(-0.0)));
        assert!(!same_value(&Value::Number(0.0), &Value::Number(-0.0)));
    }

    #[test]
    fn test_same_value_zero_nan() {
        assert!(same_value_zero(&Value::Number(f64::NAN), &Value::Number(f64::NAN)));
        assert!(same_value(&Value::Number(f64::NAN), &Value::Number(f64::NAN)));
    }

    #[test]
    fn test_same_value_zero_distinct() {
        assert!(!same_value_zero(&Value::Number(1.0), &Value::Number(2.0)));
        assert!(!same_value_zero(&Value::String("a".into()), &Value::String("b".into())));
    }

    // ── is_callable / is_constructor ────────────────────────────────────────────

    #[test]
    fn test_is_callable_and_constructor() {
        let mut ctx = crate::Context::new().unwrap();
        let fn_val = ctx.eval("(function () {})").unwrap();
        let arrow_val = ctx.eval("(() => {})").unwrap();
        let num_val = Value::Number(1.0);
        assert!(is_callable(&fn_val));
        assert!(is_callable(&arrow_val));
        assert!(!is_callable(&num_val));
        assert!(is_constructor(&fn_val));
        assert!(!is_constructor(&arrow_val));
    }

    // ── throw_type_error ────────────────────────────────────────────────────────

    #[test]
    fn test_throw_type_error_is_type_error() {
        let err = throw_type_error("boom");
        assert!(err.to_string().contains("TypeError"));
    }
}
