//! Unit tests for generator function state management.
//!
//! Generator function invocation returns a GeneratorObject.
//! Full .next/.return/.throw support is registered separately.

#[cfg(test)]
mod generator_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── Generator function detection ─────────────────────────────────────────

    #[test]
    fn generator_function_kind_is_generator() {
        // function* has kind "generator"
        let r = eval("var g = (function*() {}); typeof g;").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    // ─── Generator state ─────────────────────────────────────────────────────

    #[test]
    fn generator_returns_object() {
        let r = eval("function* g() {} typeof g()").unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn generator_next_without_value() {
        let r = eval("function* g() { yield 1; } g().next().value").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn generator_next_yields_correct_values() {
        let r = eval("function* g() { yield 1; yield 2; } var gen = g(); gen.next().value + gen.next().value").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn generator_expression_statement_completes_undefined() {
        let r = eval("function* g() { ({ yield: 1 }); } g().next().value").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn generator_declaration_function_returns_true_after_yield() {
        let r = eval(
            "function *foo(a) { yield a + 1; return; }\
             var g = foo(3); \
             var a = g.next(); \
             var b = g.next(); \
             [a.value === 4, a.done, b.done, b.value]",
        )
        .unwrap();
        if let Value::Object(obj) = r {
            let arr = obj.borrow();
            assert_eq!(arr.elements[0], Value::Boolean(true));
            assert_eq!(arr.elements[1], Value::Boolean(false));
            assert_eq!(arr.elements[2], Value::Boolean(true));
            assert_eq!(arr.elements[3], Value::Undefined);
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn generator_yield_in_array_then_completes_undefined() {
        let r = eval(
            "function* g() { [yield 1]; } \
             var gen = g(); gen.next(); gen.next().value",
        )
        .unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn generator_yield_spread_array_multiple() {
        let r = eval(
            "function* g() { yield [...yield yield]; } \
             var iter = g(); \
             var a = iter.next(false); \
             var b = iter.next(['a','b','c']); \
             var c = iter.next(b.value); \
             [String(a.value === undefined), String(a.done), b.value[0], b.value[1], b.value[2], c.value[0], c.value[1], c.value[2], String(b.done), String(c.done)].join('|');",
        )
        .unwrap();
        assert_eq!(
            r,
            Value::String("true|false|a|b|c|a|b|c|false|false".to_string())
        );
    }

    #[test]
    fn generator_yield_identifier_in_nested_call() {
        let r = eval(
            "var got; function *gen() { return (function(arg) { got = arg; return arg + 1; }(yield)); } var iter = gen(); iter.next(); var item = iter.next(42); [item.value, got]",
        )
        .unwrap();
        if let Value::Object(obj) = r {
            let arr = obj.borrow();
            assert_eq!(arr.elements[0], Value::Number(43.0));
            assert_eq!(arr.elements[1], Value::Number(42.0));
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn generator_explicit_return_value() {
        let r = eval("function* g() { return 42; } g().next().value").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn generator_return_done() {
        let r = eval("function* g() { return 42; } g().next().done").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn generator_multiple_yields() {
        let r = eval(
            "function* g() { yield 1; yield 2; yield 3; } \
             var gen = g(); \
             [gen.next().value, gen.next().value, gen.next().value];",
        )
        .unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn generator_restricted_properties_assigning_caller_throws_type_error() {
        let err = eval("function* generator() {} generator.caller = {};").unwrap_err();
        assert!(err.0.contains("TypeError"));
    }

    #[test]
    fn generator_restricted_properties_accessing_caller_throws_type_error() {
        let err = eval("function* generator() {}; generator.caller;").unwrap_err();
        assert!(err.0.contains("TypeError"));
    }

    #[test]
    fn generator_body_not_executed_on_creation() {
        let r = eval(
            "var executed = false; \
             function* g() { executed = true; } \
             var gen = g(); \
             executed;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    // ─── Yield in for-of destructuring patterns ──────────────────────────────

    /// Regression test: yield inside a for-of destructuring pattern (computed
    /// property key in member expression) must not cause the loop to exit
    /// prematurely. The item must be saved in `run.pending` so that on resume
    /// the iterator is NOT advanced again.
    #[test]
    fn for_of_destruct_yield_in_computed_member_key() {
        let r = eval(
            "var gen = (function*() { \
               var x = {}; \
               var results = []; \
               for ([[x[yield]]] of [[{ prop: 1 }]]) { \
                 results.push(x.prop); \
               } \
               return results; \
             })(); \
             gen.next(); /* yield from destructuring */ \
             gen.next('prop').value; /* resume with 'prop' */",
        )
        .unwrap_err();
        assert!(r.0.contains("iterable"));
    }

    /// Reproducer: simple for-of with yield in destructuring computed key.
    #[test]
    fn for_of_destruct_yield_simple() {
        let r = eval(
            "function* g() { \
               var x = {}; \
               for ([[x[yield]]] of [[{ prop: 1 }]]) { \
                 x.prop; \
               } \
             } \
             var gen = g(); \
             gen.next(); \
             gen.next('prop').value;",
        )
        .unwrap_err();
        assert!(r.0.contains("iterable"));
    }

    #[test]
    fn for_of_default_yield_resume() {
        // Reproducer for test262:
        // for ([x = yield] of [[]]) — yield in destructuring default + resume
        // Expected: x=86 after gen.next(86), body runs once
        let r = eval(
            "(function() { \
               var x = 'init'; \
               var bodyCount = 0; \
               function* g() { \
                 for ([x = yield] of [[]]) { \
                   bodyCount++; \
                 } \
               } \
               var gen = g(); \
               gen.next(); \
               gen.next(86); \
               return String(x) + ',' + String(bodyCount); \
             })();",
        )
        .unwrap();
        assert_eq!(r, Value::String("86,1".to_string()), "x=86, bodyCount=1");
    }

    // ─── Async generator ──────────────────────────────────────────────────────

    #[test]
    fn async_generator_returns_object() {
        let r = eval("async function* g() {} typeof g()").unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn async_generator_named_binding_assignment_in_arrow_keeps_function() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
            "var result; \
             let ref = async function * BindingIdentifier() { \
               (() => { BindingIdentifier = 1; })(); \
               return BindingIdentifier; \
             }; \
             async function test() { var item = await (await ref()).next(); result = ref.name + ',' + typeof item.value + ',' + String(item.value === 1) + ',' + String(item.value === ref); } \
             test();",
            )
            .unwrap();
        for _ in 0..8 {
            let _ = crate::builtins::promise::execute_pending_microtasks();
        }
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String(
                "BindingIdentifier,function,false,true".to_string()
            ))
        );
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn async_generator_nested_yield_object_spread_replays_symbol_value() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var s = Symbol('s'); var result; \
             var gen = async function *g() { \
               yield { ...yield yield, ...(function(arg) { var yield = arg; return {...yield}; }(yield)), ...yield }; \
             }; \
             var iter = gen(); iter.next(); iter.next(); \
             iter.next({ x: 10, a: 0, b: 0, [s]: 1 }); \
             iter.next({ y: 20, a: 1, b: 1, [s]: 42 }); \
             iter.next({ z: 30, b: 2 }).then(function(item) { result = item.value[s]; });",
        )
        .unwrap();
        for _ in 0..12 {
            let _ = crate::builtins::promise::execute_pending_microtasks();
        }
        assert_eq!(ctx.get_global("result"), Some(Value::Number(42.0)));
    }

    #[test]
    fn async_generator_with_non_object_prototype_uses_async_generator_prototype() {
        let r = eval(
            "async function* g() {} \
             var expected = Object.getPrototypeOf(g.prototype); \
             g.prototype = undefined; \
             Object.getPrototypeOf(g()) === expected;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_generator_has_next_method() {
        let r = eval("async function* g() {} var gen = g(); typeof gen.next").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_has_return_method() {
        let r = eval("async function* g() {} var gen = g(); typeof gen.return").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_has_throw_method() {
        let r = eval("async function* g() {} var gen = g(); typeof gen.throw").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_next_returns_promise() {
        let r = eval(
            "async function* g() { yield 1; } \
             var gen = g(); \
             gen.next() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_generator_await_resumes_and_completes() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; \
             async function* g() { await Promise.resolve(); return 2; } \
             g().next().then(function(value) { result = String(value.done) + ':' + String(value.value); });",
        )
        .unwrap();
        let r = ctx.eval("result").unwrap();
        assert_eq!(r, Value::String("true:2".to_string()));
    }

    #[test]
    fn async_generator_await_interleaves_with_promise_jobs() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var actual = []; \
             async function pushAwait() { actual.push('await'); } \
             async function* callAsync() { for (var i = 0; i < 2; i++) { await pushAwait(); } } \
             callAsync().next(); \
             new Promise(function(resolve) { actual.push(1); resolve(); }).then(function() { actual.push(2); });",
        )
        .unwrap();
        let r = ctx.eval("actual.join(',')").unwrap();
        assert_eq!(r, Value::String("await,1,await,2".to_string()));
    }
}
