use crate::{Context, Value};

fn eval(src: &str) -> Result<Value, crate::value::JsError> {
    Context::new().unwrap().eval(src)
}

mod return_statement {
    use super::*;

    #[test]
    fn return_with_value() {
        assert_eq!(
            eval("function f() { return 42; } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn return_without_value() {
        assert_eq!(
            eval("function f() { return; } f()").unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn static_getter_tail_return_super_not_deferred_without_trampoline() {
        assert_eq!(
            eval(
                "class B { static m() { return 1; } } \
                 class C extends B { static get x() { 0; return super.m(); } } \
                 C.x"
            )
            .unwrap(),
            Value::Number(1.0)
        );
    }
}

mod throw_statement {
    use super::*;

    #[test]
    fn throw_propagates() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("throw 42");
        assert!(result.is_err());
    }
}

mod break_continue {
    use super::*;

    #[test]
    fn break_exits_loop() {
        assert_eq!(
            eval("let i = 0; while (true) { i++; if (i > 2) break; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn continue_skips_iteration() {
        assert_eq!(
            eval("let i = 0, j = 0; while (i < 3) { i++; if (i === 2) continue; j++; } j").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn break_with_label_exits_labeled_loop() {
        assert_eq!(
            eval("let i = 0; LABEL: while (true) { i++; if (i > 2) break LABEL; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn break_label_in_eval_throws_when_label_is_outer() {
        // eval("break LABEL") where LABEL is defined OUTSIDE the eval should throw
        // SyntaxError per ES §13.12.1 (BreakStatement evaluation).
        let result =
            eval("var x = 0, y = 0; LABEL: do { x++; eval('break LABEL'); y++; } while(false); x");
        assert!(
            result.is_err(),
            "break LABEL in eval pointing to outer label should throw SyntaxError"
        );
    }

    #[test]
    fn continue_label_in_eval_throws_when_label_is_outer() {
        // eval("continue LABEL") where LABEL is defined OUTSIDE the eval should throw.
        let result = eval("var x = 0; LABEL: while (x < 3) { x++; eval('continue LABEL'); }");
        assert!(
            result.is_err(),
            "continue LABEL in eval pointing to outer label should throw SyntaxError"
        );
    }

    #[test]
    fn labeled_continue_to_outer_loop() {
        // Bug fix: continue LABEL should break out of inner loop, not continue it infinitely
        assert_eq!(
            eval(
                "let i = 0; OUTER: while (i < 3) {
                   i++;
                   INNER: while (true) { continue OUTER; }
                 } i"
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn labeled_continue_with_for_loop() {
        // continue LABEL targeting outer for loop should work
        assert_eq!(
            eval(
                "let result = 0; OUTER: for (let i = 0; i < 3; i++) {
                   INNER: for (let j = 0; j < 3; j++) {
                     if (j === 1) continue OUTER;
                     result++;
                   }
                 } result"
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn labeled_continue_in_do_while() {
        // continue LABEL targeting outer do-while should work
        assert_eq!(
            eval(
                "let i = 0; OUTER: do {
                   i++;
                   INNER: while (true) { continue OUTER; }
                 } while (i < 3); i"
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn unlabeled_continue_still_works() {
        // Regular (unlabeled) continue must still work
        assert_eq!(
            eval("let i = 0, j = 0; while (i < 3) { i++; if (i === 2) continue; j++; } j").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn break_label_in_eval_works_when_label_is_inner() {
        // break LABEL where LABEL is defined WITHIN the eval should work.
        assert_eq!(
            eval("eval('LABEL: while(true) { break LABEL; }')").unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn break_unknown_label_in_eval_throws() {
        // break to a label that doesn't exist anywhere should throw SyntaxError.
        let result = eval("eval('break NOSUCH')");
        assert!(
            result.is_err(),
            "break to undefined label should throw SyntaxError"
        );
    }

    #[test]
    fn continue_unknown_label_in_eval_throws() {
        // continue to a label that doesn't exist anywhere should throw SyntaxError.
        let result = eval("eval('continue NOSUCH')");
        assert!(
            result.is_err(),
            "continue to undefined label should throw SyntaxError"
        );
    }
}

mod block_statement {
    use super::*;

    #[test]
    fn block_creates_scope() {
        assert_eq!(
            eval("{ let x = 1; } typeof x").unwrap(),
            Value::String("undefined".into())
        );
    }

    #[test]
    fn var_hoisted_outside_block() {
        assert_eq!(eval("{ var x = 1; } x").unwrap(), Value::Number(1.0));
    }
}

mod empty_statement {
    use super::*;

    #[test]
    fn empty_returns_undefined() {
        assert_eq!(eval(";").unwrap(), Value::Undefined);
    }

    #[test]
    fn empty_does_not_override_previous_completion() {
        assert_eq!(eval("2;;").unwrap(), Value::Number(2.0));
        assert_eq!(eval("3;;;").unwrap(), Value::Number(3.0));
    }
}

mod var_declarations {
    use super::*;

    #[test]
    fn let_declaration() {
        assert_eq!(eval("let x = 5; x").unwrap(), Value::Number(5.0));
    }

    #[test]
    fn const_declaration() {
        assert_eq!(eval("const x = 7; x").unwrap(), Value::Number(7.0));
    }

    #[test]
    fn var_declaration() {
        assert_eq!(eval("var x = 3; x").unwrap(), Value::Number(3.0));
    }
}

mod if_statement {
    use super::*;

    #[test]
    fn if_branch_taken() {
        assert_eq!(eval("if (true) 1").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn else_branch_taken() {
        assert_eq!(eval("if (false) 0; else 2").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn if_without_else_returns_undefined() {
        assert_eq!(eval("if (false) 1").unwrap(), Value::Undefined);
    }
}

mod while_statement {
    use super::*;

    #[test]
    fn basic_while_loop() {
        assert_eq!(
            eval("let i = 0; while (i < 3) { i++; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn while_with_break() {
        assert_eq!(
            eval("let i = 0; while (true) { i++; if (i >= 2) break; } i").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn while_with_continue() {
        assert_eq!(
            eval("let i = 0, c = 0; while (i < 3) { i++; if (i < 2) continue; c++; } c").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn while_never_executes() {
        assert_eq!(
            eval("let x = 5; while (false) { x = 10; } x").unwrap(),
            Value::Number(5.0)
        );
    }
}

mod for_statement {
    use super::*;

    #[test]
    fn for_with_var_init() {
        assert_eq!(
            eval("for (var i = 0; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_let_init() {
        // Verify loop body executes
        assert_eq!(
            eval("let sum = 0; for (let j = 0; j < 3; j++) { sum++; } sum").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_expression_init() {
        assert_eq!(
            eval("let i = 0; for (i++; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_without_condition() {
        assert_eq!(
            eval("let i = 0; for (;;) { i++; if (i > 2) break; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_break_continue() {
        assert_eq!(
            eval("let sum = 0; for (let i = 0; i < 5; i++) { if (i === 2) continue; sum++; } sum")
                .unwrap(),
            Value::Number(4.0)
        );
    }
}

mod try_catch_statement {
    use super::*;

    #[test]
    fn try_succeeds() {
        assert_eq!(
            eval("try { 1 } catch (e) { 2 }").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn catch_binds_error() {
        assert_eq!(
            eval("try { throw 42; } catch (e) { e }").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn catch_guards_body() {
        // Verify catch runs after throw and can modify outer scope
        assert_eq!(
            eval("let x = 1; try { throw 2; } catch (e) { x = e; } x").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn catch_param_shadows() {
        assert_eq!(
            eval("let x = 1; try { throw 2; } catch (x) { x }").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn catch_with_undefined() {
        assert_eq!(
            eval("try { throw undefined; } catch (e) { e }").unwrap(),
            Value::Undefined
        );
    }
}

mod try_catch_finally_statement {
    use super::*;

    #[test]
    fn try_catch_works() {
        let r = eval("try { throw 42; } catch (e) { e }").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }
}

mod for_in_statement {
    use super::*;

    #[test]
    fn for_in_iterates_keys() {
        assert_eq!(
            eval("let keys = []; for (let k in {a: 1, b: 2}) { keys.push(k); } keys.length")
                .unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn for_in_with_break() {
        assert_eq!(eval("let keys = []; for (let k in {a: 1, b: 2, c: 3}) { keys.push(k); if (keys.length >= 2) break; } keys.length").unwrap(), Value::Number(2.0));
    }
}

// ─── Function declaration (eval_func_decl) ────────────────────────────────

mod function_declaration {
    use super::*;

    #[test]
    fn declaration_returns_undefined() {
        assert_eq!(eval("function f() {}").unwrap(), Value::Undefined);
    }

    #[test]
    fn hoisting_before_declaration() {
        assert_eq!(
            eval("f(); function f() { return 42; }").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn hoisting_among_vars() {
        assert_eq!(
            eval("var x = f(); function f() { return 10; } x").unwrap(),
            Value::Number(10.0)
        );
    }

    #[test]
    fn multiple_function_declarations() {
        assert_eq!(
            eval("function a() { return 1; } function b() { return 2; } a() + b()").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn function_inside_block() {
        assert_eq!(
            eval("{ function f() { return 7; } } f()").unwrap(),
            Value::Number(7.0)
        );
    }
}

// ─── Class declaration (eval_class_decl) ──────────────────────────────────

mod class_declaration {
    use super::*;

    #[test]
    fn declaration_returns_undefined() {
        assert_eq!(eval("class C {}").unwrap(), Value::Undefined);
    }

    #[test]
    fn direct_eval_class_decl_preserves_prior_completion() {
        assert_eq!(eval("eval('1; class C {}')").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn direct_eval_class_decl_after_prior_class_eval() {
        assert_eq!(
            eval("eval('class C {}'); eval('1; class C {}')").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn class_with_method() {
        assert_eq!(
            eval("class C { method() { return 42; } } new C().method()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn class_with_constructor() {
        assert_eq!(
            eval("class C { constructor(x) { this.x = x; } } new C(99).x").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn class_computed_getter_name_throws_on_abrupt() {
        // Computed property name in accessor should evaluate the expression.
        // If it throws, the class declaration should throw.
        let r = eval("var t = function() { throw new Error(); }; class C { get [t()]() {} }");
        assert!(
            r.is_err(),
            "computed getter name throwing should propagate to class decl, got {:?}",
            r
        );
    }

    #[test]
    fn class_computed_setter_name_throws_on_abrupt() {
        let r = eval("var t = function() { throw new Error(); }; class C { set [t()](_) {} }");
        assert!(
            r.is_err(),
            "computed setter name throwing should propagate to class decl, got {:?}",
            r
        );
    }
}

// ─── Var declaration without initializer (eval_var_decl) ──────────────────

mod var_without_init {
    use super::*;

    #[test]
    fn var_without_init_is_undefined() {
        assert_eq!(eval("var x; x").unwrap(), Value::Undefined);
    }

    #[test]
    fn let_without_init_is_undefined() {
        assert_eq!(eval("let x; x").unwrap(), Value::Undefined);
    }

    #[test]
    fn var_redeclaration_without_init_resets_to_undefined() {
        assert_eq!(eval("var x = 5; var x; x").unwrap(), Value::Undefined);
    }
}

// ─── Expression statement ─────────────────────────────────────────────────

mod expression_statement {
    use super::*;

    #[test]
    fn number_literal() {
        assert_eq!(eval("42").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn string_literal() {
        assert_eq!(eval("'hello'").unwrap(), Value::String("hello".into()));
    }

    #[test]
    fn boolean_literal() {
        assert_eq!(eval("true").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn null_literal() {
        assert_eq!(eval("null").unwrap(), Value::Null);
    }

    #[test]
    fn assignment_expression() {
        assert_eq!(eval("var x; x = 10").unwrap(), Value::Number(10.0));
    }

    #[test]
    fn call_expression() {
        assert_eq!(eval("Math.max(3, 7)").unwrap(), Value::Number(7.0));
    }
}

// ─── Multiple statements / eval_statements ────────────────────────────────

mod multiple_statements {
    use super::*;

    #[test]
    fn last_expression_is_completion_value() {
        assert_eq!(eval("1; 2; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn var_declaration_does_not_override_completion() {
        assert_eq!(eval("1; var x = 2; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn function_declaration_does_not_override_completion() {
        assert_eq!(eval("1; function f() {}; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn class_declaration_does_not_override_completion() {
        assert_eq!(eval("1; class C {}; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn sequence_of_expression_statements() {
        assert_eq!(eval("1 + 1; 2 + 2; 3 + 3").unwrap(), Value::Number(6.0));
    }
}

// ─── Return in more contexts (eval_function_body) ─────────────────────────

mod function_body {
    use super::*;

    /// Empty block: undefined per ES §13.2.1.
    #[test]
    fn return_async_call_yields_promise_not_tco() {
        assert_eq!(
            eval("function g() { return (async function() {})(); } typeof g()").unwrap(),
            Value::String("object".into())
        );
    }

    /// Empty block: undefined per ES §13.2.1.
    #[test]
    fn empty_block_is_undefined() {
        assert_eq!(eval("function f() {} f()").unwrap(), Value::Undefined);
    }

    /// Expression statement at end: its completion value is the return value.
    /// Per ES spec, the completion value of the last statement becomes the
    /// function's return value when no explicit return is present.
    #[test]
    fn expression_completion_is_return_value() {
        assert_eq!(
            eval("function f() { 42; } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    /// Postfix increment: x++ evaluates to the original value (1), then increments.
    #[test]
    fn postfix_increment_completion() {
        assert_eq!(
            eval("function f() { var x = 1; x++; } f()").unwrap(),
            Value::Number(1.0)
        );
    }

    /// Multiple statements: last statement's completion is the return value.
    #[test]
    fn last_statement_completion_is_return() {
        assert_eq!(
            eval("function f() { var x = 1; var y = 2; x + y; } f()").unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── If/else edge cases ───────────────────────────────────────────────────

mod if_edge_cases {
    use super::*;

    #[test]
    fn chained_else_if() {
        assert_eq!(
            eval("function f(n) { if (n > 5) return 'big'; else if (n > 2) return 'mid'; else return 'small'; } f(3)")
                .unwrap(),
            Value::String("mid".into())
        );
    }

    #[test]
    fn nested_if_else() {
        assert_eq!(
            eval("function f(a, b) { if (a) if (b) return 'ab'; else return 'a'; else return 'none'; } f(true, false)")
                .unwrap(),
            Value::String("a".into())
        );
    }

    #[test]
    fn if_with_block_body() {
        assert_eq!(
            eval("var x = 0; if (true) { x = 1; x++; } x").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn if_with_complex_condition() {
        assert_eq!(
            eval("if (1 + 1 === 2) 10; else 20").unwrap(),
            Value::Number(10.0)
        );
    }
}

// ─── While loop edge cases ────────────────────────────────────────────────

mod while_edge_cases {
    use super::*;

    #[test]
    fn while_with_return() {
        assert_eq!(
            eval("function f() { while (true) { return 42; } return 0; } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn while_break_in_inner_loop() {
        // break only exits the inner loop, outer continues
        assert_eq!(
            eval("let i = 0; while (i < 3) { i++; let j = 0; while (j < 3) { j++; break; } } i")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn while_with_complex_condition() {
        assert_eq!(
            eval("let i = 0; while ((i++) < 3) {} i").unwrap(),
            Value::Number(4.0)
        );
    }
}

// ─── For loop edge cases ──────────────────────────────────────────────────

mod for_edge_cases {
    use super::*;

    #[test]
    fn for_with_return() {
        assert_eq!(
            eval("function f() { for (var i = 0; i < 10; i++) { if (i === 3) return i; } return -1; } f()").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_no_init() {
        assert_eq!(
            eval("var i = 0; for (; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_no_update() {
        assert_eq!(
            eval("let i = 0; for (; i < 3;) { i++; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_empty_body() {
        assert_eq!(
            eval("let s = 0; for (let i = 0; i < 5; i++, s++); s").unwrap(),
            Value::Number(5.0)
        );
    }

    #[test]
    fn for_without_body_block() {
        assert_eq!(
            eval("let i = 0; for (; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Try/catch edge cases ─────────────────────────────────────────────────

mod try_catch_edge_cases {
    use super::*;

    #[test]
    fn return_in_try() {
        assert_eq!(
            eval("function f() { try { return 42; } catch (e) { return 0; } } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn return_in_catch() {
        assert_eq!(
            eval("function f() { try { throw 1; } catch (e) { return e + 10; } } f()").unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn nested_try_catch() {
        assert_eq!(
            eval("try { try { throw 1; } catch (e) { throw e + 1; } } catch (f) { f }").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn try_catch_with_var_in_body() {
        // var in try body is hoisted and visible in catch
        assert_eq!(
            eval("var x = 'outer'; try { throw 1; } catch (e) { x = 'caught'; } x").unwrap(),
            Value::String("caught".into())
        );
    }

    #[test]
    fn try_catch_no_throw_skips_catch() {
        assert_eq!(
            eval("try { 99 } catch (e) { 0 }").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn try_catch_with_object_error() {
        assert_eq!(
            eval("try { throw { code: 500 }; } catch (e) { e.code }").unwrap(),
            Value::Number(500.0)
        );
    }

    #[test]
    fn try_catch_with_string_error() {
        assert_eq!(
            eval("try { throw 'error'; } catch (e) { e }").unwrap(),
            Value::String("error".into())
        );
    }
}

// ─── For-in edge cases ────────────────────────────────────────────────────

mod for_in_edge_cases {
    use super::*;

    #[test]
    fn for_in_with_continue() {
        assert_eq!(
            eval("let keys = []; for (let k in {a:1, b:2, c:3}) { if (k === 'b') continue; keys.push(k); } keys.length")
                .unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn for_in_with_return() {
        assert_eq!(
            eval("function f() { for (let k in {a:1, b:2, c:3}) { if (k === 'b') return k; } return 'none'; } f()")
                .unwrap(),
            Value::String("b".into())
        );
    }

    #[test]
    fn for_in_on_array() {
        assert_eq!(
            eval("let keys = []; for (let k in ['a', 'b', 'c']) { keys.push(k); } keys.length")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_in_on_empty_object() {
        assert_eq!(
            eval("let count = 0; for (let k in {}) { count++; } count").unwrap(),
            Value::Number(0.0)
        );
    }

    #[test]
    fn for_in_with_var_declaration() {
        assert_eq!(
            eval("let keys = []; for (var k in {x:1, y:2}) { keys.push(k); } keys.length").unwrap(),
            Value::Number(2.0)
        );
    }
}

// ─── Break/continue edge cases ────────────────────────────────────────────

mod break_continue_edge_cases {
    use super::*;

    #[test]
    fn break_in_for() {
        assert_eq!(
            eval("let s = 0; for (let i = 0; i < 10; i++) { if (i === 5) break; s = i; } s")
                .unwrap(),
            Value::Number(4.0)
        );
    }

    #[test]
    fn continue_in_nested_loops() {
        assert_eq!(
            eval("let acc = 0; for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) continue; acc++; } } acc")
                .unwrap(),
            Value::Number(6.0)
        );
    }

    #[test]
    fn break_in_nested_loops() {
        assert_eq!(
            eval("let acc = 0; for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) break; acc++; } } acc")
                .unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Throw edge cases ─────────────────────────────────────────────────────

mod throw_edge_cases {
    use super::*;

    #[test]
    fn throw_type_error() {
        let result = eval("throw new TypeError('bad')");
        assert!(result.is_err());
    }

    #[test]
    fn throw_and_catch_string() {
        assert_eq!(
            eval("try { throw 'msg'; } catch (e) { e }").unwrap(),
            Value::String("msg".into())
        );
    }

    #[test]
    fn throw_prevents_subsequent_code() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("throw 1; 42");
        assert!(result.is_err());
    }
}

// ─── Sequence declarations (multiple var decls at once) ───────────────────

mod sequence_decls {
    use super::*;

    #[test]
    fn multiple_var_in_one_statement() {
        assert_eq!(
            eval("var a = 1, b = 2, c = 3; a + b + c").unwrap(),
            Value::Number(6.0)
        );
    }

    #[test]
    fn multiple_let_in_one_statement() {
        assert_eq!(eval("let a = 4, b = 5; a + b").unwrap(), Value::Number(9.0));
    }

    #[test]
    fn mixed_init_and_uninit() {
        assert_eq!(
            eval("var a = 1, b; a + (b === undefined ? 2 : 0)").unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Block statement edge cases ───────────────────────────────────────────

mod block_edge_cases {
    use super::*;

    #[test]
    fn block_returns_last_expression() {
        assert_eq!(eval("{ 1; 2; 3 }").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn nested_blocks_create_scopes() {
        assert_eq!(
            eval("{ let x = 1; { let x = 2; } x }").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn empty_block_returns_undefined() {
        assert_eq!(eval("{}").unwrap(), Value::Undefined);
    }

    #[test]
    fn block_with_var_hoists_out() {
        assert_eq!(eval("{ var x = 10; } x").unwrap(), Value::Number(10.0));
    }
}

// ─── Throw with different types ───────────────────────────────────────────

mod throw_types {
    use super::*;

    #[test]
    fn throw_number_caught() {
        assert_eq!(
            eval("try { throw 99; } catch (e) { e }").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn throw_boolean_caught() {
        assert_eq!(
            eval("try { throw true; } catch (e) { e }").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn throw_object_caught() {
        assert_eq!(
            eval("var o = { msg: 'err' }; try { throw o; } catch (e) { e.msg }").unwrap(),
            Value::String("err".into())
        );
    }

    #[test]
    fn throw_caught_does_not_propagate() {
        assert_eq!(
            eval("var x = 0; try { throw 1; } catch (e) { x = e; } x").unwrap(),
            Value::Number(1.0)
        );
    }
}

// ─── is_tail_expr ─────────────────────────────────────────────────────────

mod is_tail_expr {
    use super::super::is_tail_expr;
    use crate::ast::Expression;

    #[test]
    fn call_expression_is_tail() {
        let expr = Expression::Call {
            callee: Box::new(Expression::Identifier("f".into())),
            arguments: vec![],
        };
        assert!(is_tail_expr(&expr));
    }

    #[test]
    fn identifier_is_not_tail() {
        let expr = Expression::Identifier("x".into());
        assert!(!is_tail_expr(&expr));
    }

    #[test]
    fn binary_add_is_not_tail() {
        let expr = Expression::Binary {
            left: Box::new(Expression::Identifier("x".into())),
            op: crate::ast::BinaryOp::Add,
            right: Box::new(Expression::Number(1.0)),
        };
        assert!(!is_tail_expr(&expr));
    }

    #[test]
    fn direct_eval_call_is_not_tail() {
        let expr = Expression::Call {
            callee: Box::new(Expression::Identifier("eval".into())),
            arguments: vec![Expression::String("1".into())],
        };
        assert!(!is_tail_expr(&expr));
    }
}

// ─── acc_stack thread-local ────────────────────────────────────────────────

mod acc_stack {
    use super::super::{
        acc_stack_len, acc_stack_pop_to, acc_stack_push, acc_stack_top, acc_stack_update_last,
    };
    use crate::value::Value;

    fn sym(desc: &'static str) -> Value {
        crate::builtins::symbol::new_symbol(desc)
    }

    /// Drain the thread-local stack to a clean state.
    fn drain() {
        acc_stack_pop_to(0);
    }

    #[test]
    fn empty_stack_returns_none() {
        drain();
        assert_eq!(acc_stack_len(), 0);
        assert!(acc_stack_top().is_none());
    }

    #[test]
    fn push_increases_len() {
        drain();
        acc_stack_push(sym("A"));
        assert_eq!(acc_stack_len(), 1);
        assert!(acc_stack_top().is_some());
    }

    #[test]
    fn pop_to_restores_to_target() {
        drain();
        acc_stack_push(sym("X"));
        acc_stack_push(sym("Y"));
        acc_stack_push(sym("Z"));
        assert_eq!(acc_stack_len(), 3);
        // Pop back to depth 1: removes Y and Z.
        acc_stack_pop_to(1);
        assert_eq!(acc_stack_len(), 1);
        assert!(acc_stack_top().is_some_and(|v| v.is_symbol_with("X")));
    }

    #[test]
    fn pop_to_zero_clears() {
        drain();
        acc_stack_push(sym("A"));
        acc_stack_push(sym("B"));
        acc_stack_push(sym("C"));
        acc_stack_pop_to(0);
        assert_eq!(acc_stack_len(), 0);
        assert!(acc_stack_top().is_none());
    }

    #[test]
    fn update_last_replaces_top() {
        drain();
        acc_stack_push(sym("BOTTOM"));
        acc_stack_push(sym("OLD"));
        // update_last replaces the most-recently-pushed item (the top).
        acc_stack_update_last(sym("NEW"));
        assert!(acc_stack_top().is_some_and(|v| v.is_symbol_with("NEW")));
        acc_stack_pop_to(0);
    }

    #[test]
    fn update_last_only_affects_top() {
        drain();
        acc_stack_push(sym("A"));
        acc_stack_push(sym("B"));
        acc_stack_push(sym("C"));
        // Update top only.
        acc_stack_update_last(sym("X"));
        // Stack: A, B, X  (top replaced, A and B unchanged)
        let top = acc_stack_top();
        assert!(
            top.as_ref().is_some_and(|v| v.is_symbol_with("X")),
            "top should be X, got {:?}",
            top
        );
        drain();
    }

    #[test]
    fn nested_push_pop() {
        drain();
        acc_stack_push(sym("L1"));
        acc_stack_push(sym("L2"));
        acc_stack_pop_to(1);
        assert_eq!(acc_stack_len(), 1);
        acc_stack_push(sym("L2a"));
        assert_eq!(acc_stack_len(), 2);
        assert!(acc_stack_top().is_some_and(|v| v.is_symbol_with("L2a")));
        drain();
    }
}

// ─── tail_call_signal thread-local ─────────────────────────────────────────

mod tail_signal {
    use super::super::{set_tail_call_signal, take_tail_call_signal, TailCallSignal};
    use crate::env::Environment;
    use crate::value::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn make_fn() -> crate::value::ValueFunction {
        crate::value::ValueFunction::new(
            None,
            vec![],
            vec![],
            Rc::new(RefCell::new(Environment::new())),
            false,
            false,
        )
    }

    #[test]
    fn set_and_take() {
        assert!(take_tail_call_signal().is_none());

        let sig = TailCallSignal {
            function: make_fn(),
            arguments: vec![Value::Number(42.0)],
            this_val: Value::Undefined,
        };
        set_tail_call_signal(sig);
        let taken = take_tail_call_signal();
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().arguments, vec![Value::Number(42.0)]);
    }

    #[test]
    fn take_clears() {
        let sig = TailCallSignal {
            function: make_fn(),
            arguments: vec![],
            this_val: Value::Undefined,
        };
        set_tail_call_signal(sig);
        assert!(take_tail_call_signal().is_some());
        assert!(take_tail_call_signal().is_none());
    }
}

// ─── Non-tail call: caller executes remaining statements ─────────────────────

/// Regression test: var + return expression (no inner function call).
/// This tests the basic var scoping + return expression evaluation.
mod var_and_return {
    use super::*;

    #[test]
    fn var_then_return_expression() {
        // function f() { var x = 10; return x + 1; }
        // x is 10, return x+1 → 11
        assert_eq!(
            eval(
                r#""use strict";
                function f() { var x = 10; return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn var_from_expression_then_return() {
        // function f() { var x = 5 * 2; return x + 1; }
        assert_eq!(
            eval(
                r#""use strict";
                function f() { var x = 5 * 2; return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn multiple_var_then_return() {
        assert_eq!(
            eval(
                r#""use strict";
                function f() { var a = 1; var b = 2; return a + b + 3; }
                f()"#,
            )
            .unwrap(),
            Value::Number(6.0)
        );
    }

    /// Simple function call with return value used in expression.
    #[test]
    fn call_return_used_in_expression() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 10; }
                var x = g();
                x + 1"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    /// Same pattern but INSIDE a function body — this is the failing case.
    #[test]
    fn call_return_used_in_function_body() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 10; }
                function f() { var x = g(); return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }
}

mod non_tail_call {
    use super::*;

    /// The canonical non-tail call: var x = f(); return x + 1;
    /// f() returns 10, caller adds 1 → 11.
    #[test]
    fn caller_executes_remaining_after_non_tail_call() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 10; }
                function f() { var x = g(); return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    /// Multiple non-tail calls in sequence.
    #[test]
    fn multiple_non_tail_calls() {
        assert_eq!(
            eval(
                r#""use strict";
                function a() { return 1; }
                function b() { return a() + 10; }
                function c() { return b() + 100; }
                c()"#,
            )
            .unwrap(),
            Value::Number(111.0)
        );
    }

    /// Deep non-tail chain: f → g (tail) → h (tail), h returns, g adds, f adds.
    #[test]
    fn deep_non_tail_chain() {
        assert_eq!(
            eval(
                r#""use strict";
                function h() { return 100; }
                function g() { var y = h(); return y + 10; }
                function f() { var x = g(); return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(111.0)
        );
    }

    /// Non-tail call where caller discards the value.
    #[test]
    fn non_tail_call_result_discarded() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 99; }
                function f() { g(); return 42; }
                f()"#,
            )
            .unwrap(),
            Value::Number(42.0)
        );
    }

    /// Non-tail call with side-effect in expression.
    #[test]
    fn non_tail_call_with_side_effect() {
        assert_eq!(
            eval(
                r#""use strict";
                var calls = 0;
                function g() { calls += 1; return 10; }
                function f() { var x = g(); return calls; }
                f()"#,
            )
            .unwrap(),
            Value::Number(1.0)
        );
    }
}

// ─── try-finally ────────────────────────────────────────────────────────
// Note: try-finally (try without catch) is not yet implemented.
// These tests are ignored until the feature is added.

// ─── optional chaining ──────────────────────────────────────────────────
// Note: optional chaining on null/undefined is not fully implemented.
// These tests cover the supported cases.

mod optional_chaining {
    use super::*;

    #[test]
    fn optional_member_on_object() {
        let r = eval("var o = {a: 1}; o?.a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn optional_member_on_missing_property() {
        let r = eval("({})?.missing").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn optional_call_on_function() {
        let r = eval("var f = () => 42; f?.()").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn optional_chain_with_method() {
        let r = eval("var o = {m() { return 5; }}; o.m?.()").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn optional_chain_on_array() {
        let r = eval("[1,2,3]?.[1]").unwrap();
        assert_eq!(r, Value::Number(2.0));
    }
}

// ─── array spread in literals ──────────────────────────────────────────

mod array_spread {
    use super::*;

    #[test]
    fn spread_in_array_literal() {
        let r = eval("[1, ...[2, 3], 4]").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 4);
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::Number(2.0)));
                assert_eq!(arr.elements.get(2), Some(&Value::Number(3.0)));
                assert_eq!(arr.elements.get(3), Some(&Value::Number(4.0)));
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn spread_empty_array() {
        let r = eval("[1, ...[], 2]").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 2);
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::Number(2.0)));
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn spread_string() {
        let r = eval("[...'abc']").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 3);
                assert_eq!(arr.elements.first(), Some(&Value::String("a".to_string())));
                assert_eq!(arr.elements.get(1), Some(&Value::String("b".to_string())));
                assert_eq!(arr.elements.get(2), Some(&Value::String("c".to_string())));
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn spread_nested() {
        let r = eval("[1, ...['a', ...['b', 'c']], 2]").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 5);
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::String("a".to_string())));
                assert_eq!(arr.elements.get(2), Some(&Value::String("b".to_string())));
                assert_eq!(arr.elements.get(3), Some(&Value::String("c".to_string())));
                assert_eq!(arr.elements.get(4), Some(&Value::Number(2.0)));
            }
            _ => panic!("expected array"),
        }
    }
}

mod do_while_statement {
    use super::*;

    #[test]
    fn do_while_returns_body_completion_value() {
        // do-while body completes with a value when condition is false
        assert_eq!(eval("do { 1; } while (false)").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn do_while_returns_last_body_value() {
        // When condition becomes false, return the body completion value
        assert_eq!(
            eval("let x = 0; do { x++; } while (x < 3); x").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn do_while_returns_undefined_when_body_no_value() {
        // Body with no completion value returns undefined
        let result = eval("do ; while (false)").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn do_while_returns_expression_value() {
        assert_eq!(
            eval("do { 42; } while (false)").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn do_while_break_exits_loop() {
        assert_eq!(
            eval("let i = 0; do { i++; if (i > 2) break; } while (true); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn do_while_break_returns_undefined() {
        // break does not provide a value; loop returns undefined
        let result = eval("let i = 0; do { i++; if (i > 2) break; } while (true)").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn do_while_return_interrupts() {
        assert_eq!(
            eval("function f() { do { return 99; } while (true); } f()").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn do_while_continue_restarts() {
        // continue in do-while jumps back to condition check, skipping j++.
        // i=1,2: continue skips j++. i=3,4: j++ runs. i=5: exit.
        // j ends at 3 (j=1 at i=3, j=2 at i=4, j=3 at i=5).
        assert_eq!(
            eval("let i = 0, j = 0; do { i++; if (i < 3) continue; j++; } while (i < 5); j")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn do_while_with_nested_labeled_break() {
        // break inside a nested labeled do-while should exit only that do-while,
        // not the outer one. Variables declared after break inside the inner
        // do-while should still be accessible (var hoisting).
        let mut ctx = Context::new().unwrap();
        ctx.eval("var result = ''").unwrap();
        assert_eq!(
            ctx.eval(
                r#"
                var result = "";
                do_out: do {
                    result += "A";
                    do_in: do {
                        result += "B";
                        break do_in;
                        result += "FAIL";
                    } while (0);
                    result += "C";
                } while (2==1);
                result;
            "#
            )
            .unwrap(),
            Value::String("ABC".to_string())
        );
    }

    #[test]
    fn do_while_body_completion_from_expression() {
        // The test S12.6.1_A3: do __in__do=1; while(false) should return 1
        assert_eq!(
            eval("var x = 0; do x = 1; while (false); x").unwrap(),
            Value::Number(1.0)
        );
    }
}

mod labeled_statement {
    use super::*;

    #[test]
    fn eval_block_block_with_labels() {
        // eval('{}{x: 42};') should return 42, not the object {x: 42}.
        // {x: 42} in statement context is a Block with a labeled statement,
        // not an object literal. The completion value should be 42.
        assert_eq!(eval("eval('{}{x: 42};')").unwrap(), Value::Number(42.0));
    }
}

#[test]
fn test_block_const_scope() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx.eval("function fn() { const u = 3; { const v = 6; return u + v; } } fn()");
    eprintln!("block const scope: {:?}", r);
}

mod class_name_in_const {
    use super::*;

    #[test]
    fn const_class_gets_binding_name() {
        // Bug fix: const cls = class {}; should set cls.name = "cls"
        assert_eq!(
            eval("const cls = class {}; cls.name").unwrap(),
            Value::String("cls".to_string())
        );
    }

    #[test]
    fn let_class_gets_binding_name() {
        // Same for let declarations
        assert_eq!(
            eval("let cls = class {}; cls.name").unwrap(),
            Value::String("cls".to_string())
        );
    }

    #[test]
    fn var_class_gets_binding_name() {
        // Same for var declarations
        assert_eq!(
            eval("var cls = class {}; cls.name").unwrap(),
            Value::String("cls".to_string())
        );
    }

    #[test]
    fn named_class_not_overridden() {
        // Named class expressions should keep their own name
        assert_eq!(
            eval("var cls = class MyClass {}; cls.name").unwrap(),
            Value::String("MyClass".to_string())
        );
    }
}
