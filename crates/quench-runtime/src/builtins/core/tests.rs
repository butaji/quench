//! Tests for the canonical ops used by builtins.
//!
//! These are regression tests ensuring the ops used in builtins produce correct results.
//! The canonical implementations live in `crate::eval::ops`.

#[cfg(test)]
mod tests {
    use crate::eval::ops::to_primitive;
    use crate::Value;

    // ── string.rs uses to_primitive(number hint) ────────────────────────────────

    #[test]
    fn test_to_primitive_number_hint_primitives() {
        // Primitives should return themselves regardless of hint.
        assert_eq!(to_primitive(&Value::Number(42.0), Some("number")).unwrap(), Value::Number(42.0));
        assert_eq!(to_primitive(&Value::Null, Some("number")).unwrap(), Value::Null);
        assert_eq!(to_primitive(&Value::Boolean(true), Some("number")).unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_to_primitive_number_hint_object() {
        let mut ctx = crate::Context::new().unwrap();
        let obj = ctx.eval("({ valueOf() { return 17 } })").unwrap();
        let prim = to_primitive(&obj, Some("number")).unwrap();
        assert_eq!(prim, Value::Number(17.0));
    }

    // ── descriptors.rs uses to_primitive(string hint) ──────────────────────────

    #[test]
    fn test_to_primitive_string_hint_primitives() {
        assert_eq!(to_primitive(&Value::String("foo".into()), Some("string")).unwrap(), Value::String("foo".into()));
    }

    #[test]
    fn test_to_primitive_string_hint_object() {
        let mut ctx = crate::Context::new().unwrap();
        let obj = ctx.eval("({ toString() { return 'bar' } })").unwrap();
        let prim = to_primitive(&obj, Some("string")).unwrap();
        assert_eq!(prim, Value::String("bar".into()));
    }

    // ── bigint.rs uses to_primitive(number hint) ────────────────────────────────

    #[test]
    fn test_to_primitive_bigint_coercion() {
        let mut ctx = crate::Context::new().unwrap();
        let obj = ctx.eval("({ valueOf() { return 99n } })").unwrap();
        let prim = to_primitive(&obj, Some("number")).unwrap();
        // valueOf returns BigInt; toPrimitive for BigInt returns it as-is
        use std::rc::Rc;
        use num_bigint::BigInt;
        assert!(matches!(prim, Value::BigInt(_)));
        let Value::BigInt(bi) = prim else { unreachable!() };
        assert_eq!(*bi, BigInt::from(99));
    }
}
