//! Comprehensive tests for assert.deepEqual and structural equality

#[cfg(test)]
mod tests {
    use crate::test262::{QuenchHost, Test262Host};

    // =============================================================================
    // Circular reference tests
    // =============================================================================

    #[test]
    fn test_deep_equal_circular_self_reference() {
        let mut host = QuenchHost::new();
        let result = host.run_script("var a = []; a.push(a); assert.deepEqual(a, a);");
        assert!(
            result.is_ok(),
            "self-referencing array should equal itself: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_circular_mutual_reference() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "var a = [], b = []; a.push(b); b.push(a); assert.deepEqual(a, a); assert.deepEqual(b, b);",
        );
        assert!(result.is_ok(), "mutual refs should work: {:?}", result);
    }

    #[test]
    fn test_deep_equal_circular_objects() {
        let mut host = QuenchHost::new();
        let result =
            host.run_script("var a = {}, b = {}; a.self = a; b.self = b; assert.deepEqual(a, b);");
        assert!(
            result.is_ok(),
            "circular objects should deep equal: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_circular_different_structures_fails() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "var a = {x: 1}, b = {x: 2}; a.self = a; b.self = b; assert.throws(Test262Error, function() { assert.deepEqual(a, b) });",
        );
        assert!(
            result.is_ok(),
            "different circular objects should fail: {:?}",
            result
        );
    }

    // =============================================================================
    // NaN handling
    // =============================================================================

    #[test]
    fn test_deep_equal_nan_equals_nan() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual(NaN, NaN)");
        assert!(result.is_ok(), "NaN should equal NaN: {:?}", result);
    }

    #[test]
    fn test_deep_equal_nan_in_array() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual([NaN, 1, NaN], [NaN, 1, NaN])");
        assert!(
            result.is_ok(),
            "arrays with NaN should be equal: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_nan_in_object() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual({a: NaN, b: 1}, {a: NaN, b: 1})");
        assert!(
            result.is_ok(),
            "objects with NaN should be equal: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_nan_not_equal_to_number() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.throws(Test262Error, function() { assert.deepEqual(NaN, 0) }); assert.throws(Test262Error, function() { assert.deepEqual(0, NaN) });",
        );
        assert!(result.is_ok(), "NaN should not equal numbers: {:?}", result);
    }

    // =============================================================================
    // -0 handling
    // =============================================================================

    #[test]
    fn test_deep_equal_negative_zero_equals_negative_zero() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual(-0, -0)");
        assert!(result.is_ok(), "-0 should equal -0: {:?}", result);
    }

    #[test]
    fn test_deep_equal_negative_zero_not_equals_zero() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.throws(Test262Error, function() { assert.deepEqual(0, -0) }); assert.throws(Test262Error, function() { assert.deepEqual(-0, 0) });",
        );
        assert!(result.is_ok(), "+0 should not equal -0: {:?}", result);
    }

    #[test]
    fn test_deep_equal_negative_zero_in_array() {
        let mut host = QuenchHost::new();
        let result = host
            .run_script("assert.throws(Test262Error, function() { assert.deepEqual([0], [-0]) });");
        assert!(
            result.is_ok(),
            "arrays with +0/-0 should differ: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_negative_zero_in_object() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.throws(Test262Error, function() { assert.deepEqual({x: 0}, {x: -0}) });",
        );
        assert!(
            result.is_ok(),
            "objects with +0/-0 should differ: {:?}",
            result
        );
    }

    // =============================================================================
    // Boxed primitives
    // =============================================================================

    #[test]
    fn test_deep_equal_boxed_number() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual(new Number(42), new Number(42))");
        assert!(result.is_ok(), "boxed numbers should equal: {:?}", result);
    }

    #[test]
    fn test_deep_equal_boxed_string() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual(new String('hi'), new String('hi'))");
        assert!(result.is_ok(), "boxed strings should equal: {:?}", result);
    }

    #[test]
    fn test_deep_equal_boxed_boolean() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual(new Boolean(true), new Boolean(true))");
        assert!(result.is_ok(), "boxed booleans should equal: {:?}", result);
    }

    #[test]
    fn test_deep_equal_boxed_different_values() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.throws(Test262Error, function() { assert.deepEqual(new Number(1), new Number(2)) }); assert.throws(Test262Error, function() { assert.deepEqual(new String('a'), new String('b')) });",
        );
        assert!(
            result.is_ok(),
            "different boxed primitives should not equal: {:?}",
            result
        );
    }

    // =============================================================================
    // Object property order independence
    // =============================================================================

    #[test]
    fn test_deep_equal_property_order_independent() {
        let mut host = QuenchHost::new();
        let result = host.run_script("assert.deepEqual({a: 1, b: 2}, {b: 2, a: 1})");
        assert!(
            result.is_ok(),
            "property order should not matter: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_nested_property_order_independent() {
        let mut host = QuenchHost::new();
        let result =
            host.run_script("assert.deepEqual({a: {x: 1}, b: {y: 2}}, {b: {y: 2}, a: {x: 1}})");
        assert!(
            result.is_ok(),
            "nested property order should not matter: {:?}",
            result
        );
    }

    // =============================================================================
    // Primitive edge cases
    // =============================================================================

    #[test]
    fn test_deep_equal_null_and_undefined() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.throws(Test262Error, function() { assert.deepEqual(null, undefined) }); assert.throws(Test262Error, function() { assert.deepEqual(undefined, null) });",
        );
        assert!(
            result.is_ok(),
            "null should not equal undefined: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_same_primitives() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.deepEqual(null, null); assert.deepEqual(undefined, undefined); assert.deepEqual(true, true); assert.deepEqual(false, false); assert.deepEqual('hello', 'hello'); assert.deepEqual(42, 42);",
        );
        assert!(result.is_ok(), "same primitives should equal: {:?}", result);
    }

    #[test]
    fn test_deep_equal_infinity() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "assert.deepEqual(Infinity, Infinity); assert.deepEqual(-Infinity, -Infinity); assert.throws(Test262Error, function() { assert.deepEqual(Infinity, -Infinity) });",
        );
        assert!(result.is_ok(), "infinity edge cases: {:?}", result);
    }

    // =============================================================================
    // Deeply nested
    // =============================================================================

    #[test]
    fn test_deep_equal_deeply_nested() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "var a = {a: {a: {a: {a: {x: 1}}}}}; var b = {a: {a: {a: {a: {x: 1}}}}}; assert.deepEqual(a, b);",
        );
        assert!(
            result.is_ok(),
            "deeply nested objects should equal: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_deeply_nested_mismatch() {
        let mut host = QuenchHost::new();
        let result = host.run_script(
            "var a = {a: {a: {a: {x: 1}}}}; var b = {a: {a: {a: {x: 2}}}}; assert.throws(Test262Error, function() { assert.deepEqual(a, b) });",
        );
        assert!(
            result.is_ok(),
            "deeply nested mismatch should fail: {:?}",
            result
        );
    }
}
