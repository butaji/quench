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
    fn async_generator_private_static_methods_all_resolve() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; class C { static async * #a(value) { yield * await value; } static async * #b(value) { yield * await value; } static get a() { return this.#a; } static get b() { return this.#b; } } Promise.all([C.a([1]).next(), C.b([1]).next()]).then(function(items) { result = items[0].value + items[1].value; });",
        )
        .unwrap();
        for _ in 0..8 {
            let _ = crate::builtins::promise::execute_pending_microtasks();
        }
        assert_eq!(ctx.get_global("result"), Some(Value::Number(2.0)));
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
    fn private_async_generator_return_value_is_preserved() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(
            "var ctorPromise; function check(value, expected) { if (value !== expected) throw new Error(); } class C { async * #m() { return 42; } get ref() { return this.#m; } \
             constructor() { check(typeof this.#m, 'function'); check(this.ref, this.#m); var p = this.#m().next(); ctorPromise = p.then(function(v) { check(v.value, 42); return v.value; }); } } \
             var c = new C(); var other = new C(); var result; ctorPromise.then(function() { return c.ref().next(); }).then(function(v) { result = v.value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::Number(42.0)));
    }

    #[test]
    fn async_generator_preserves_postfix_update_across_yields() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(
            "var seen = []; async function* values() { var current = 3; while (current > 0) yield current--; } var iterator = values(); iterator.next().then(function(v) { seen.push(v.value); iterator.next().then(function(v) { seen.push(v.value); iterator.next().then(function(v) { seen.push(v.value); iterator.next().then(function(v) { seen.push(v.done); }); }); }); });",
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        assert_eq!(
            ctx.eval("seen.join(',')").unwrap(),
            Value::String("3,2,1,true".into())
        );
    }

    #[test]
    fn computed_accessor_names_preserve_yield_resumes() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var assigned, obj; function* g() { obj = { get [yield]() { return 'get'; }, set [yield](value) { assigned = value; } }; } var iterator = g(); iterator.next(); iterator.next('first'); iterator.next('second');")
            .unwrap();
        assert_eq!(ctx.eval("obj.first").unwrap(), Value::String("get".into()));
        ctx.eval("obj.second = 'set'").unwrap();
        assert_eq!(ctx.eval("assigned").unwrap(), Value::String("set".into()));
    }

    #[test]
    fn template_assignment_waits_for_yield_resume() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("var value; function* g() { value = `1${yield}3${4}5`; } var iterator = g(); var first = iterator.next(); var before = value; var second = iterator.next(2); String(before) + '|' + value + '|' + second.done")
            .unwrap();
        assert_eq!(result, Value::String("undefined|12345|true".into()));
    }

    #[test]
    fn in_operand_assignment_waits_for_yield_resume() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("var object = { hit: true }; var value; function* g() { value = yield 'hit' in object; value = yield 'miss' in object; } var iterator = g(); var first = iterator.next(); var before = value; var second = iterator.next('second'); first.value === true && before === undefined && second.value === false && value === 'second'")
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn generator_return_does_not_replay_import_options_prefix() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("var before = 0; var after = 0; var iterator = function*() { before += 1, import('', yield), after += 1; }(); iterator.next(); var returned = iterator.return(595); String(returned.done) + ':' + returned.value + ':' + before + ':' + after")
            .unwrap();
        assert_eq!(result, Value::String("true:595:1:0".into()));
    }

    #[test]
    fn generator_for_loop_advances_between_yields() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("function* g() { for (var i = 0; i < 3; i++) yield i; } var iterator = g(); [iterator.next().value, iterator.next().value, iterator.next().value, iterator.next().done].join(',')")
            .unwrap();
        assert_eq!(result, Value::String("0,1,2,true".into()));
    }

    #[test]
    fn yield_delegate_does_not_read_value_until_done() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("var calls = 0; var step = Object.defineProperty({ done: false }, 'value', { get: function() { calls++; } }); var source = { [Symbol.iterator]: function() { return { next: function() { return step; } }; } }; function* g() { yield* source; } var iterator = g(); iterator.next(); calls")
            .unwrap();
        assert_eq!(result, Value::Number(0.0));
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

    #[test]
    fn async_generator_queues_concurrent_next_requests() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var values = []; async function* g() { yield Promise.resolve(42); yield Promise.resolve(39); } var iterator = g(); var a = iterator.next(); var b = iterator.next(); a.then(function(step) { values.push(step.value); }); b.then(function(step) { values.push(step.value); });")
            .unwrap();
        assert_eq!(
            ctx.eval("values.join(',')").unwrap(),
            Value::String("42,39".into())
        );
    }

    #[test]
    fn context_eval_clears_stale_generator_yield_state() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var log = []; async function* g() { log.push({ name: 'started' }); yield 1; } var iter = g();")
            .unwrap();
        crate::interpreter::set_generator_yield(Value::String("stale".into()));
        ctx.eval("iter.next();").unwrap();
        assert_eq!(
            ctx.eval("log[0].name").unwrap(),
            Value::String("started".into())
        );
    }
}
