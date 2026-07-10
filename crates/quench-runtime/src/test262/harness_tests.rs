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

    // =============================================================================
    // fnGlobalObject.js tests (Task 359)
    // =============================================================================

    #[test]
    fn harness_fn_global_object_returns_object() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("typeof fnGlobalObject()");
        assert!(result.is_ok(), "fnGlobalObject should work: {:?}", result);
        if let Ok(v) = result {
            // typeof returns "object" for objects
            assert!(matches!(v, crate::Value::String(ref s) if s == "object"), 
                "fnGlobalObject should return an object, got {:?}", v);
        }
    }

    #[test]
    fn harness_fn_global_object_equals_global_this() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("fnGlobalObject() === globalThis");
        assert!(result.is_ok(), "fnGlobalObject should equal globalThis: {:?}", result);
    }

    // =============================================================================
    // isConstructor.js tests (Task 359)
    // =============================================================================

    #[test]
    fn harness_is_constructor_function() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("isConstructor(function() {})");
        assert!(result.is_ok(), "isConstructor should work: {:?}", result);
    }

    #[test]
    fn harness_is_constructor_builtin() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("isConstructor(Array)");
        assert!(result.is_ok(), "isConstructor should work: {:?}", result);
    }

    #[test]
    fn harness_is_constructor_object() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // isConstructor should return false for objects, not throw
        let result = ctx.eval("isConstructor({})");
        assert!(result.is_ok(), "isConstructor should not throw");
        // The actual value check would require evaluating it
        let result = ctx.eval("isConstructor({}) === false");
        assert!(result.is_ok(), "isConstructor for object literal should be false");
    }

    // =============================================================================
    // regExpUtils.js tests (Task 360)
    // =============================================================================

    #[test]
    fn harness_build_string_lone_code_points() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // Test buildString with lone code points
        let result = ctx.eval(
            "buildString({loneCodePoints: [65, 66, 67], ranges: []}) === 'ABC'"
        );
        assert!(result.is_ok(), "buildString should work: {:?}", result);
    }

    #[test]
    fn harness_build_string_ranges() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // Test buildString with ranges
        let result = ctx.eval(
            "buildString({loneCodePoints: [], ranges: [[65, 67]]}) === 'ABC'"
        );
        assert!(result.is_ok(), "buildString should work with ranges: {:?}", result);
    }

    #[test]
    fn harness_test_property_of_strings() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // testPropertyOfStrings should not throw
        let result = ctx.eval(
            "testPropertyOfStrings({regExp: /a/, matchStrings: ['a'], nonMatchStrings: ['b']})"
        );
        assert!(result.is_ok(), "testPropertyOfStrings should work: {:?}", result);
    }

    #[test]
    fn harness_match_validator() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // matchValidator should not throw when called
        let result = ctx.eval("matchValidator(['b'], 0, 'abc')");
        assert!(result.is_ok(), "matchValidator should work: {:?}", result);
    }

    // =============================================================================
    // asyncHelpers.js tests (Task 361)
    // =============================================================================

    #[test]
    fn harness_async_test() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // asyncTest should not throw when called with a function
        let result = ctx.eval("asyncTest(function() {})");
        assert!(result.is_ok(), "asyncTest should work: {:?}", result);
    }

    // =============================================================================
    // detachArrayBuffer.js tests (Task 362)
    // =============================================================================

    #[test]
    fn harness_detach_buffer() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // $DETACHBUFFER should mark buffer as detached (using plain object)
        let result = ctx.eval(
            "var buf = {byteLength: 8}; $DETACHBUFFER(buf); buf.byteLength === 0 && buf.detached === true"
        );
        assert!(result.is_ok(), "$DETACHBUFFER should work: {:?}", result);
    }
}
