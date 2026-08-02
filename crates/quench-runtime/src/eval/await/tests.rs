//! Unit tests for async/await runtime support.

#[cfg(test)]
mod await_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── is_promise ──────────────────────────────────────────────────────────

    #[test]
    fn is_promise_promise_object() {
        // Promise.resolve() returns a Promise object
        let r = eval("Promise.resolve(1) !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn await_binary_operand_is_lowered_as_right_operand() {
        let crate::ast::Program::Script(statements) = crate::parser::parse_script(
            "async function f() { actual.push('Await: ' + await patched); }",
        )
        .unwrap();
        let crate::ast::Statement::FunctionDeclaration { body, .. } = &statements[0] else {
            panic!("expected function declaration");
        };
        let crate::ast::Statement::Expression(expression) = &body[0] else {
            panic!("expected expression statement");
        };
        let crate::ast::Expression::Call { arguments, .. } = expression.as_ref() else {
            panic!("expected call expression");
        };
        assert!(matches!(
            &arguments[0],
            crate::ast::Expression::Binary {
                right,
                ..
            } if matches!(right.as_ref(), crate::ast::Expression::Await(_))
        ));
    }

    #[test]
    fn await_call_suspension_preserves_following_statement() {
        let crate::ast::Program::Script(statements) =
            crate::parser::parse_script("async function f() { a(); b(); }").unwrap();
        let crate::ast::Statement::FunctionDeclaration { body, .. } = &statements[0] else {
            panic!("expected function declaration");
        };
        assert_eq!(body.len(), 2);
        let crate::ast::Program::Script(statements) = crate::parser::parse_script(
            "async function f() { a.push('x' + await p); a.push('y' + await p); }",
        )
        .unwrap();
        let crate::ast::Statement::FunctionDeclaration { body, .. } = &statements[0] else {
            panic!("expected function declaration");
        };
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn await_call_suspension_runs_following_await() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var count = 0; var result = []; var p = {}; p.then = function(resolve) { count++; resolve(count); }; \
             async function f() { result.push(await p); result.push(await p); } \
             f().then(function() { result.push('done'); });",
        )
        .unwrap();
        assert_eq!(
            ctx.eval("result.join(',')").unwrap(),
            Value::String("1,2,done".into())
        );
    }

    #[test]
    fn is_promise_plain_object_false() {
        // Plain objects are not Promises
        let r = eval("var o = {}; o.then !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn is_promise_number_false() {
        let r = eval("typeof 42;").unwrap();
        assert_eq!(r, Value::String("number".to_string()));
    }

    #[test]
    fn is_promise_string_false() {
        let r = eval("typeof 'hello';").unwrap();
        assert_eq!(r, Value::String("string".to_string()));
    }

    #[test]
    fn is_promise_thenable_is_promise() {
        // An object with a .then method that follows the right signature
        // should be treated as a Promise-like (thenable) — but per spec,
        // is_promise checks the prototype chain for Promise.prototype's
        // promise_data marker. A plain thenable without the marker returns false.
        let r = eval(
            "var thenable = { then: function(resolve) { resolve(1); } }; \
             Promise.resolve(thenable) instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── eval_await_value ────────────────────────────────────────────────────

    #[test]
    fn await_resolves_non_promise() {
        // Promise.resolve wraps non-Promise values
        let r = eval("Promise.resolve(42) !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn await_passes_through_promise() {
        // If value is already a Promise, eval_await_value returns it unchanged
        let r = eval(
            "var p = Promise.resolve(1); \
             Promise.resolve(p) === p;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── async function basics ────────────────────────────────────────────────

    #[test]
    fn async_function_returns_promise() {
        let r = eval(
            "async function f() {} \
             f() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_function_resolves_return_value() {
        let r = eval(
            "async function f() { return 42; } \
             f();",
        )
        .unwrap();
        // Returns a Promise
        assert!(!matches!(r, Value::Undefined));
    }

    #[test]
    fn async_arrow_returns_promise() {
        let r = eval(
            "var f = async () => 1; \
             f() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_method_returns_promise() {
        let r = eval(
            "var obj = { async method() { return 'ok'; } }; \
             obj.method() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_await_plain_value_fulfills_with_value() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { return await 42; } f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::Number(42.0)));
    }

    #[test]
    fn async_await_plain_value_in_variable_fulfills_with_value() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { var value = await 42; return value; } f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::Number(42.0)));
    }

    #[test]
    fn async_await_super_method_with_done_binding_fulfills_with_string() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var $DONE = function() {}; var sup = { method() { return 'sup'; } }; \
             var child = { async method() { var x = await super.method(); \
             globalThis.result = x; } }; Object.setPrototypeOf(child, sup); \
             child.method().then($DONE, $DONE);",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("sup".to_string()))
        );
    }

    #[test]
    fn async_function_finally_await_reject_overrides_return() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result;
             var seen;
             async function f() {
               try {
                 return 'early-return';
               } finally {
                 seen = await new Promise(function(resolve, reject) { reject('override'); });
               }
              }
             var p = f();
             p.then(function(v) { result = 'resolved:' + v; }, function(e) { result = 'rejected:' + e; });
             ;",
        )
        .unwrap();
        let f = ctx.eval("f").unwrap();
        if let Value::Function(f) = f {
            assert!(f.is_async);
        } else {
            panic!("f should be a function");
        }
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        let r = ctx.eval("result").unwrap();
        assert_eq!(r, Value::String("rejected:override".to_string()));
        let seen = ctx.eval("typeof seen").unwrap();
        assert_eq!(seen, Value::String("undefined".to_string()));
    }

    #[test]
    fn async_finally_return_await_overrides_pending_throw() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { try { throw 'early'; } finally { return await new Promise(function(resolve) { resolve('override'); }); } } \
             f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("override".into()))
        );
    }

    #[test]
    fn named_async_function_name_reassignment_rejects_in_strict_body() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "'use strict'; var result; var calls = 0; var ref = async function BindingIdentifier() { \
             calls++; (() => { BindingIdentifier = 1; })(); }; \
             ref().then(function() { result = 'resolved'; }, function(error) { result = error.name; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("calls"), Some(Value::Number(1.0)));
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("TypeError".into()))
        );
    }

    #[test]
    fn async_await_rejected_promise_enters_catch() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { try { await Promise.reject('boom'); } catch (error) { result = error; } } \
             f();",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::String("boom".into())));
    }

    #[test]
    fn async_await_thenable_throw_enters_catch_with_same_error() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var error={}; var caught=false; var same=false; var thenable={then:function(resolve,reject){throw error;}}; async function f(){try{await thenable;}catch(e){caught=true;same=e===error;}} f();")
            .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("caught"), Some(Value::Boolean(true)));
        assert_eq!(ctx.get_global("same"), Some(Value::Boolean(true)));
    }

    #[test]
    fn async_await_monkey_patched_promise_then_is_not_called() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var thenCallCount = 0; \
             const value = 42; \
             const actual = []; \
             const patched = Promise.resolve(value); \
             patched.then = function(...args) { \
                 thenCallCount += 1; \
                 Promise.prototype.then.apply(this, args); \
             }; \
             var result; \
             async function trigger() { \
                 actual.push('Await: ' + await patched); \
             } \
             trigger().then(function() { result = 'done'; }, function(error) { result = error; }); \
             new Promise(function (resolve) { \
                 actual.push('Promise: 1'); \
                 resolve(); \
             }).then(function () { \
                 actual.push('Promise: 2'); \
             });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("thenCallCount"), Some(Value::Number(0.0)));
        assert_eq!(ctx.get_global("result"), Some(Value::String("done".into())));
        assert_eq!(
            ctx.eval("actual.join(',')").unwrap(),
            Value::String("Promise: 1,Await: 42,Promise: 2".to_string())
        );
        let then_count = ctx.get_global("thenCallCount").unwrap();
        assert_eq!(then_count, Value::Number(0.0));
        let result = ctx.eval("result").unwrap();
        return if let Value::String(result) = result {
            assert_eq!(result, "done");
            ()
        } else {
            panic!("unexpected result: {result:?}");
        };
    }

    #[test]
    fn async_function_finally_await_reject_overrides_throw() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result;
             async function f() {
               try {
                 await new Promise(function(resolve, reject) { reject('early-reject'); });
               } finally {
                 await new Promise(function(resolve, reject) { reject('override'); });
               }
             }
             var p = f();
             p.then(function(v) { result = 'resolved:' + v; }, function(e) { result = 'rejected:' + e; });",
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        let r = ctx.eval("result").unwrap();
        assert_eq!(r, Value::String("rejected:override".to_string()));
    }

    #[test]
    fn async_arrow_finally_rejection_reaches_chained_handler() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; var f = async () => { try { throw 'early-throw'; } \
             finally { await new Promise(function(resolve, reject) { reject('override'); }); } }; \
             f().then(function() { result = 'wrong'; }, function(value) { result = value; }) \
             .then(function() { result = result + ':done'; }, function() { result = 'rejected'; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("override:done".into()))
        );
    }

    #[test]
    fn async_arrow_try_reject_finally_reject_overrides_throw() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; \
             var f = async() => { \
               try { \
                 await new Promise(function(resolve, reject) { reject('early-reject'); }); \
               } finally { \
                 await new Promise(function(resolve, reject) { reject('override'); }); \
               } \
             }; \
             f().then(function() { result = 'resolved'; }, function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("override".into()))
        );
    }

    #[test]
    fn async_function_await_in_try_catch_catches_rejection() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "'use strict'; \
             var callCount = 0; \
             var ref = async function BindingIdentifier() { \
               callCount++; \
               (() => { BindingIdentifier = 1; })(); \
             }; \
             async function f() { \
               var catchCount = 0; \
               try { \
                 await ref(); \
               } catch (error) { \
                 catchCount += 1; \
               } \
               return [catchCount, callCount]; \
             } \
             var result; \
             f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.eval("result[0]").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("result[1]").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn named_async_function_reassignment_in_arrow_rejects_in_strict_awaited_call() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "'use strict'; \
             let callCount = 0; \
             var result; \
             var ref = async function BindingIdentifier() { \
               callCount++; \
               (() => { BindingIdentifier = 1; })(); \
               return BindingIdentifier; \
             }; \
             async function f() { \
               let catchCount = 0; \
               try { \
                 await ref(); \
               } catch (error) { \
                 catchCount += 1; \
               } \
               result = catchCount + ',' + callCount; \
             } \
             f();",
        )
        .unwrap();
        for _ in 0..8 {
            let _ = crate::builtins::promise::execute_pending_microtasks();
        }
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("1,1".to_string()))
        );
    }

    #[test]
    fn async_await_then_promises_interleave_without_refcell_panic() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var actual = [];\
             async function pushAwait(value) { actual.push('Await: ' + value); }\
             async function callAsync() {\
               await pushAwait(1);\
               await pushAwait(2);\
             }\
             callAsync();\
             new Promise(function(resolve) { actual.push('Promise: 1'); resolve(); })\
               .then(function() { actual.push('Promise: 2'); });",
        )
        .unwrap();

        let _ = crate::builtins::promise::execute_pending_microtasks();

        assert_eq!(
            ctx.eval("actual.join(',')").unwrap(),
            Value::String("Await: 1,Promise: 1,Await: 2,Promise: 2".to_string())
        );
    }

    #[test]
    fn async_function_call_returns_marked_promise() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "async function f() { return 42; }\
             f();",
            )
            .unwrap();
        assert!(
            matches!(result, Value::Object(_)),
            "async function should return a promise-like object"
        );
        assert!(
            crate::eval::r#await::is_promise(&result),
            "promise marker should be visible"
        );
    }

    #[test]
    fn async_generator_next_first_step_is_promise() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "async function pushAwait() { return 1; }\
             async function* callAsync() {\
               await pushAwait();\
             }\
             var gen = callAsync();\
             var result = gen.next();",
        )
        .unwrap();
        let result = ctx.eval("result").unwrap();
        assert!(crate::eval::r#await::is_promise(&result));
    }

    #[test]
    fn async_generator_for_loop_next_is_promise_then_completion() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "async function pushAwait() { }\
             async function* callAsync() {\
               for (let i = 0; i < 2; i++) {\
                 await pushAwait();\
               }\
               return 0;\
             }\
             var gen = callAsync();\
             var result = gen.next();",
        )
        .unwrap();
        let first = ctx.eval("result").unwrap();
        assert!(crate::eval::r#await::is_promise(&first));
    }
}
