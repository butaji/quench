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
