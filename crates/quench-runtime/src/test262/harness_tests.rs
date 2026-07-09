//! Tests for test262 harness helpers

#[cfg(test)]
mod tests {
    use crate::test262::harness::inject_harness;
    use crate::Context;

    #[test]
    fn harness_assert_same_value_passes() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.sameValue(1 + 1, 2, 'addition');");
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    fn harness_assert_same_value_fails() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.sameValue(1 + 1, 3, 'addition');");
        assert!(result.is_err(), "Expected failure but got {:?}", result);
    }

    #[test]
    fn harness_assert_same_value() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.sameValue(1, 1, 'should pass');");
        assert!(result.is_ok());
    }

    #[test]
    fn harness_assert_not_same_value() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.notSameValue(1, 2, 'should pass');");
        assert!(result.is_ok());
    }

    #[test]
    fn harness_print() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("print('hello');");
        assert!(result.is_ok());
    }

    #[test]
    fn harness_compare_array_passes() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray([1, 2, 3], [1, 2, 3]);");
        assert!(result.is_ok(), "compareArray should pass: {:?}", result);
    }

    #[test]
    fn harness_compare_array_fails_length() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray([1, 2], [1, 2, 3]);");
        assert!(result.is_err(), "compareArray should fail on different lengths");
    }

    #[test]
    fn harness_compare_array_fails_elements() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray([1, 2, 3], [1, 2, 4]);");
        assert!(result.is_err(), "compareArray should fail on different elements");
    }

    #[test]
    fn harness_compare_array_with_nan() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray([NaN], [NaN]);");
        assert!(result.is_ok(), "compareArray should pass with NaN: {:?}", result);
    }

    #[test]
    fn harness_compare_array_with_zeros() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray([+0], [-0]);");
        assert!(result.is_err(), "compareArray should fail on +0 vs -0");
    }

    #[test]
    fn harness_compare_array_primitive_actual() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray(42, [42]);");
        assert!(result.is_err(), "compareArray should fail on primitive actual");
    }

    #[test]
    fn harness_compare_array_primitive_expected() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.compareArray([42], 42);");
        assert!(result.is_err(), "compareArray should fail on primitive expected");
    }

    #[test]
    fn harness_array_contains_passes() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.arrayContains([1, 2, 3, 4, 5], [2, 4]);");
        assert!(result.is_ok(), "arrayContains should pass: {:?}", result);
    }

    #[test]
    fn harness_array_contains_fails() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.arrayContains([1, 2, 3], [2, 4]);");
        assert!(result.is_err(), "arrayContains should fail when element not found");
    }

    #[test]
    fn harness_array_contains_with_nan() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.arrayContains([1, NaN, 3], [NaN]);");
        assert!(result.is_ok(), "arrayContains should pass with NaN: {:?}", result);
    }

    // =============================================================================
    // propertyHelper.js tests (Task 358)
    // =============================================================================

    #[test]
    fn harness_verify_property_passes() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("var obj = {x: 42}; verifyProperty(obj, 'x', {value: 42});");
        assert!(result.is_ok(), "verifyProperty should pass: {:?}", result);
    }

    #[test]
    fn harness_verify_property_fails_wrong_value() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("var obj = {x: 42}; verifyProperty(obj, 'x', {value: 100});");
        assert!(result.is_err(), "verifyProperty should fail on wrong value");
    }

    #[test]
    fn harness_verify_accessor_property() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval(
            "var obj = {}; Object.defineProperty(obj, 'x', {get: function() { return 42; }}); \
             verifyAccessorProperty(obj, 'x', {get: function() { return 42; }});"
        );
        assert!(result.is_ok(), "verifyAccessorProperty should pass: {:?}", result);
    }

    // =============================================================================
    // deepEqual.js tests (Task 358)
    // =============================================================================

    #[test]
    fn harness_deep_equal_passes() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.deepEqual({a: 1, b: 2}, {a: 1, b: 2});");
        assert!(result.is_ok(), "deepEqual should pass: {:?}", result);
    }

    #[test]
    fn harness_deep_equal_fails() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.deepEqual({a: 1}, {a: 2});");
        assert!(result.is_err(), "deepEqual should fail on different values");
    }

    #[test]
    fn harness_deep_equal_arrays() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.deepEqual([1, 2, 3], [1, 2, 3]);");
        assert!(result.is_ok(), "deepEqual should pass for arrays: {:?}", result);
    }
}
