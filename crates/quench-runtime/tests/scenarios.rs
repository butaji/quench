//! JS/TS scenario tests for quench-runtime

use quench_runtime::{Context, Value};

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Basic Array scenarios
    // =========================================================================

    #[test]
    fn scenario_array_literal_basic() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("[1, 2, 3]").unwrap();
        match &result {
            Value::Object(obj) => {
                let arr = obj.borrow();
                assert_eq!(arr.elements.len(), 3);
            }
            _ => panic!("Expected array, got {:?}", result),
        }
    }

    #[test]
    fn scenario_array_index_access() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("[1, 2, 3][1]").unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    // =========================================================================
    // Basic Object scenarios
    // =========================================================================

    #[test]
    fn scenario_object_literal_basic() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("({ x: 1, y: 2 })").unwrap();
        match &result {
            Value::Object(obj) => {
                let obj = obj.borrow();
                assert_eq!(obj.get("x"), Some(Value::Number(1.0)));
                assert_eq!(obj.get("y"), Some(Value::Number(2.0)));
            }
            _ => panic!("Expected object, got {:?}", result),
        }
    }

    // =========================================================================
    // Error scenarios
    // =========================================================================

    #[test]
    fn scenario_throw_number() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("throw 42");
        assert!(result.is_err());
    }

    #[test]
    fn scenario_throw_error_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("throw new Error('test')");
        assert!(result.is_err());
    }

    #[test]
    fn scenario_new_error_constructor() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Error('test') instanceof Error").unwrap();
        assert_eq!(result, Value::Boolean(true));
        
        let result = ctx.eval("new Error('test').message").unwrap();
        assert_eq!(result, Value::String("test".to_string()));
    }

    // =========================================================================
    // Optional chaining scenarios
    // =========================================================================

    #[test]
    fn scenario_optional_chaining_basic() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = { a: { b: 42 } }; obj?.a?.b").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn scenario_optional_chaining_null_shortcircuit() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("const obj = null; obj?.a?.b").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    // =========================================================================
    // Nullish coalescing scenarios
    // =========================================================================

    #[test]
    fn scenario_nullish_coalescing_returns_default_for_null() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("null ?? 'default'").unwrap();
        assert_eq!(result, Value::String("default".to_string()));
    }

    // =========================================================================
    // Template literal scenarios
    // =========================================================================

    #[test]
    fn scenario_template_literal_simple() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("`hello world`").unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }

    #[test]
    fn scenario_template_literal_with_expression() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"`a${1 + 2}b`"#).unwrap();
        assert_eq!(result, Value::String("a3b".to_string()));
    }

    // =========================================================================
    // String prototype scenarios
    // =========================================================================

    #[test]
    fn scenario_string_length() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("\"hello\".length").unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn scenario_string_to_upper_case() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("\"hello\".toUpperCase()").unwrap();
        assert_eq!(result, Value::String("HELLO".to_string()));
    }

    // =========================================================================
    // typeof scenarios
    // =========================================================================

    #[test]
    fn scenario_typeof_undeclared_returns_undefined() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof nonExistentVariable").unwrap();
        assert_eq!(result, Value::String("undefined".to_string()));
    }

    // =========================================================================
    // for-of scenarios
    // =========================================================================

    #[test]
    fn scenario_for_of_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let sum = 0; for (let x of [1, 2, 3]) { sum += x; } sum").unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn scenario_for_of_string() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let chars = []; for (let c of 'ab') { chars.push(c); } chars.join(',')").unwrap();
        assert_eq!(result, Value::String("a,b".to_string()));
    }

    #[test]
    fn scenario_for_of_var() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let result = ''; for (var x of [1, 2]) { result += x; } result").unwrap();
        assert_eq!(result, Value::String("12".to_string()));
    }

    // =========================================================================
    // for-in scenarios
    // =========================================================================

    #[test]
    fn scenario_for_in_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let keys = []; for (let k in {x: 1, y: 2}) { keys.push(k); } keys.join(',')").unwrap();
        // Check that both keys are present (order is implementation-defined)
        let s = match result {
            Value::String(s) => s,
            _ => panic!("Expected string, got {:?}", result),
        };
        assert!(s.contains("x") && s.contains("y"), "Expected keys x and y, got {}", s);
    }

    #[test]
    fn scenario_for_in_string() {
        let mut ctx = Context::new().unwrap();
        // for-in on string should iterate over indices
        let result = ctx.eval("let chars = []; for (let c in 'ab') { chars.push(c); } chars.join(',')").unwrap();
        // String iteration gives indices
        let s = match result {
            Value::String(s) => s,
            _ => panic!("Expected string, got {:?}", result),
        };
        // Check that we get 2 items
        assert_eq!(s.matches(',').count(), 1, "Expected 2 chars separated by comma, got {}", s);
    }

    #[test]
    fn scenario_for_in_empty_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let count = 0; for (let k in {}) { count++; } count").unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn scenario_for_in_with_break() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let keys = []; for (let k in {a:1, b:2, c:3}) { if (k === 'b') break; keys.push(k); } keys.join(',')").unwrap();
        assert_eq!(result, Value::String("a".to_string()));
    }

    #[test]
    fn scenario_for_in_with_continue() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let keys = []; for (let k in {a:1, b:2, c:3}) { if (k === 'b') continue; keys.push(k); } keys.join(',')").unwrap();
        assert_eq!(result, Value::String("a,c".to_string()));
    }

    // =========================================================================
    // instanceof scenarios
    // =========================================================================

    #[test]
    fn scenario_instanceof_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("[] instanceof Array").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn scenario_instanceof_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("({}) instanceof Object").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn scenario_instanceof_function() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("(function(){}) instanceof Function").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn scenario_instanceof_not() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("({}) instanceof Array").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn scenario_instanceof_with_custom_constructor() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            function MyClass() {}
            function NotMyClass() {}
            let obj = new MyClass();
            let r1 = obj instanceof MyClass;
            let r2 = obj instanceof NotMyClass;
            [r1, r2];
        "#).unwrap();
        match result {
            Value::Object(o) => {
                let o = o.borrow();
                assert_eq!(o.elements.first(), Some(&Value::Boolean(true)), "should be instanceof MyClass");
                assert_eq!(o.elements.get(1), Some(&Value::Boolean(false)), "should not be instanceof NotMyClass");
            }
            _ => panic!("Expected array, got {:?}", result),
        }
    }

    #[test]
    fn scenario_instanceof_with_inheritance() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            function Parent() {}
            function Child() {}
            Child.prototype = Object.create(Parent.prototype);
            let child = new Child();
            let r1 = child instanceof Child;
            let r2 = child instanceof Parent;
            let r3 = child instanceof Object;
            [r1, r2, r3];
        "#).unwrap();
        match result {
            Value::Object(o) => {
                let o = o.borrow();
                assert_eq!(o.elements.first(), Some(&Value::Boolean(true)), "child instanceof Child");
                assert_eq!(o.elements.get(1), Some(&Value::Boolean(true)), "child instanceof Parent");
                assert_eq!(o.elements.get(2), Some(&Value::Boolean(true)), "child instanceof Object");
            }
            _ => panic!("Expected array, got {:?}", result),
        }
    }

    #[test]
    fn scenario_instanceof_primitive_returns_false() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("42 instanceof Object").unwrap();
        assert_eq!(result, Value::Boolean(false));
        let result = ctx.eval("'string' instanceof Object").unwrap();
        assert_eq!(result, Value::Boolean(false));
        let result = ctx.eval("true instanceof Object").unwrap();
        assert_eq!(result, Value::Boolean(false));
        let result = ctx.eval("null instanceof Object").unwrap();
        assert_eq!(result, Value::Boolean(false));
        let result = ctx.eval("undefined instanceof Object").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    // =========================================================================
    // in operator scenarios
    // =========================================================================

    #[test]
    fn scenario_in_operator_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'a' in {a: 1, b: 2}").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn scenario_in_operator_not_found() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'c' in {a: 1, b: 2}").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn scenario_in_operator_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("'0' in [1, 2, 3]").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    // =========================================================================
    // getter scenarios
    // =========================================================================

    #[test]
    fn scenario_getter_basic() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let obj = { get x() { return 42; } }; obj.x").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn scenario_getter_with_args() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let obj = { get val() { return this._val * 2; } }; obj._val = 5; obj.val").unwrap();
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn scenario_getter_returns_undefined() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let obj = { get x() {} }; obj.x").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn scenario_getter_in_closure() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            let captured = 100;
            let obj = { 
                get value() { return captured; },
                set value(v) { captured = v * 2; }
            };
            let r1 = obj.value;
            obj.value = 50;
            let r2 = obj.value;
            [r1, r2, captured];
        "#).unwrap();
        // After setting value to 50, captured becomes 100 (50 * 2)
        // So r1 should be 100, r2 should be 100, captured should be 100
        match result {
            Value::Object(o) => {
                let o = o.borrow();
                assert_eq!(o.elements.first(), Some(&Value::Number(100.0)), "r1 should be 100");
                assert_eq!(o.elements.get(1), Some(&Value::Number(100.0)), "r2 should be 100");
                assert_eq!(o.elements.get(2), Some(&Value::Number(100.0)), "captured should be 100");
            }
            _ => panic!("Expected array, got {:?}", result),
        }
    }

    #[test]
    fn scenario_setter_basic() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("let stored = null; let obj = { set x(v) { stored = v; } }; obj.x = 100; stored").unwrap();
        assert_eq!(result, Value::Number(100.0));
    }

    // =========================================================================
    // Symbol scenarios
    // =========================================================================

    #[test]
    fn scenario_symbol_typeof() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Symbol").unwrap();
        assert_eq!(result, Value::String("function".to_string()));
    }

    #[test]
    fn scenario_symbol_creation() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Symbol('test')").unwrap();
        assert_eq!(result, Value::String("symbol".to_string()));
    }

    #[test]
    fn scenario_symbol_unique() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Symbol('a') !== Symbol('a')").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn scenario_symbol_no_args() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Symbol()").unwrap();
        assert_eq!(result, Value::String("symbol".to_string()));
    }

    #[test]
    fn scenario_symbol_as_property_key() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            const sym = Symbol('test');
            const obj = {};
            obj[sym] = 'value';
            obj[sym];
        "#).unwrap();
        assert_eq!(result, Value::String("value".to_string()));
    }

    #[test]
    fn scenario_symbol_strict_equality() {
        let mut ctx = Context::new().unwrap();
        // Symbol should not be loosely equal to anything
        let result = ctx.eval("Symbol() == Symbol()").unwrap();
        assert_eq!(result, Value::Boolean(false));
        let result = ctx.eval("Symbol() === Symbol()").unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    // =========================================================================
    // Global scenarios
    // =========================================================================

    #[test]
    fn scenario_global_read_write() {
        let mut ctx = Context::new().unwrap();
        ctx.set_global("myGlobal".to_string(), Value::Number(42.0));
        assert_eq!(ctx.get_global("myGlobal"), Some(Value::Number(42.0)));

        let result = ctx.eval("myGlobal").unwrap();
        assert_eq!(result, Value::Number(42.0));

        ctx.eval("myGlobal = 99").unwrap();
        assert_eq!(ctx.get_global("myGlobal"), Some(Value::Number(99.0)));
    }
}

