use crate::{Context, Value};

fn eval(src: &str) -> Result<Value, crate::value::JsError> {
    Context::new().unwrap().eval(src)
}

mod return_statement {
    use super::*;

    #[test]
    fn return_with_value() {
        assert_eq!(eval("function f() { return 42; } f()").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn return_without_value() {
        assert_eq!(eval("function f() { return; } f()").unwrap(), Value::Undefined);
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
        assert_eq!(eval("let i = 0; while (true) { i++; if (i > 2) break; } i").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn continue_skips_iteration() {
        assert_eq!(eval("let i = 0, j = 0; while (i < 3) { i++; if (i === 2) continue; j++; } j").unwrap(), Value::Number(2.0));
    }
}

mod block_statement {
    use super::*;

    #[test]
    fn block_creates_scope() {
        assert_eq!(eval("{ let x = 1; } typeof x").unwrap(), Value::String("undefined".into()));
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
        assert_eq!(eval("let i = 0; while (i < 3) { i++; } i").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn while_with_break() {
        assert_eq!(eval("let i = 0; while (true) { i++; if (i >= 2) break; } i").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn while_with_continue() {
        assert_eq!(eval("let i = 0, c = 0; while (i < 3) { i++; if (i < 2) continue; c++; } c").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn while_never_executes() {
        assert_eq!(eval("let x = 5; while (false) { x = 10; } x").unwrap(), Value::Number(5.0));
    }
}

mod for_statement {
    use super::*;

    #[test]
    fn for_with_var_init() {
        assert_eq!(eval("for (var i = 0; i < 3; i++); i").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn for_with_let_init() {
        // Verify loop body executes
        assert_eq!(eval("let sum = 0; for (let j = 0; j < 3; j++) { sum++; } sum").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn for_with_expression_init() {
        assert_eq!(eval("let i = 0; for (i++; i < 3; i++); i").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn for_without_condition() {
        assert_eq!(eval("let i = 0; for (;;) { i++; if (i > 2) break; } i").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn for_with_break_continue() {
        assert_eq!(eval("let sum = 0; for (let i = 0; i < 5; i++) { if (i === 2) continue; sum++; } sum").unwrap(), Value::Number(4.0));
    }
}

mod try_catch_statement {
    use super::*;

    #[test]
    fn try_succeeds() {
        assert_eq!(eval("try { 1 } catch (e) { 2 }").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn catch_binds_error() {
        assert_eq!(eval("try { throw 42; } catch (e) { e }").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn catch_guards_body() {
        // Verify catch runs after throw and can modify outer scope
        assert_eq!(eval("let x = 1; try { throw 2; } catch (e) { x = e; } x").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn catch_param_shadows() {
        assert_eq!(eval("let x = 1; try { throw 2; } catch (x) { x }").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn catch_with_undefined() {
        assert_eq!(eval("try { throw undefined; } catch (e) { e }").unwrap(), Value::Undefined);
    }
}

mod for_in_statement {
    use super::*;

    #[test]
    fn for_in_iterates_keys() {
        assert_eq!(eval("let keys = []; for (let k in {a: 1, b: 2}) { keys.push(k); } keys.length").unwrap(), Value::Number(2.0));
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
        assert_eq!(eval("f(); function f() { return 42; }").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn hoisting_among_vars() {
        assert_eq!(eval("var x = f(); function f() { return 10; } x").unwrap(), Value::Number(10.0));
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
        assert_eq!(eval("{ function f() { return 7; } } f()").unwrap(), Value::Number(7.0));
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

    #[test]
    fn body_without_return_is_undefined() {
        assert_eq!(eval("function f() { 42; } f()").unwrap(), Value::Undefined);
    }

    #[test]
    fn body_with_empty_block_is_undefined() {
        assert_eq!(eval("function f() {} f()").unwrap(), Value::Undefined);
    }

    #[test]
    fn body_with_multiple_statements_returns_undefined() {
        assert_eq!(eval("function f() { var x = 1; x++; } f()").unwrap(), Value::Undefined);
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
        assert_eq!(eval("if (1 + 1 === 2) 10; else 20").unwrap(), Value::Number(10.0));
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
            eval("let i = 0; while (i < 3) { i++; let j = 0; while (j < 3) { j++; break; } } i").unwrap(),
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
        assert_eq!(eval("try { 99 } catch (e) { 0 }").unwrap(), Value::Number(99.0));
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
            eval("let keys = []; for (let k in ['a', 'b', 'c']) { keys.push(k); } keys.length").unwrap(),
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
            eval("let s = 0; for (let i = 0; i < 10; i++) { if (i === 5) break; s = i; } s").unwrap(),
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
        assert_eq!(
            eval("let a = 4, b = 5; a + b").unwrap(),
            Value::Number(9.0)
        );
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
        assert_eq!(eval("try { throw 99; } catch (e) { e }").unwrap(), Value::Number(99.0));
    }

    #[test]
    fn throw_boolean_caught() {
        assert_eq!(eval("try { throw true; } catch (e) { e }").unwrap(), Value::Boolean(true));
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
