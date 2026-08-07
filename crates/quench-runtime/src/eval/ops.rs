//! Canonical ECMAScript abstract operations (spec ops).
//!
//! This module is the single source of truth for spec abstract operations.
//! All `eval/` nodes and JS builtins must use these, not local copies.
//!
//! Ops are exposed on `%ops%` (the JS-Rust bridge) so JS builtins can call them.
//! New op: add here → expose on `%ops%` → use from JS.

// Re-export the canonical implementations from their homes.
pub use crate::builtins::object_static::to_property_key;
pub use crate::value::coerce::to_number;
pub use crate::value::primitive::to_primitive;
pub use crate::value::primitive::PrimitiveHint;

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
}
