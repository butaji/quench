use super::*;

#[test]
fn test_function_constructor_compiles_real_function() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var f = Function('a', 'return a'); f(3)").unwrap();
    assert_eq!(result, Value::Number(3.0));
}

#[test]
fn test_function_constructor_multiple_params() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval("Function('a', 'b', 'return a + b')(2, 5)")
        .unwrap();
    assert_eq!(result, Value::Number(7.0));
}

#[test]
fn test_function_constructor_uses_global_scope() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var g = 41; Function('return g + 1')()").unwrap();
    assert_eq!(result, Value::Number(42.0));
}

#[test]
fn test_function_constructor_invalid_body_throws() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Function('a', 'return a @ b')");
    assert!(result.is_err(), "invalid body must throw SyntaxError");
}

#[test]
fn test_function_constructor_immediate_call() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Function('a', 'return a')(3)").unwrap();
    assert_eq!(result, Value::Number(3.0));
}

#[test]
fn test_bind_sets_length_and_name() {
    let mut ctx = Context::new().unwrap();
    let len = ctx
        .eval("Function.prototype.bind.call(function foo(a, b) {}, null, 1).length")
        .unwrap();
    assert_eq!(len, Value::Number(1.0));
    let name = ctx
        .eval("Function.prototype.bind.call(function foo(a, b) {}, null).name")
        .unwrap();
    assert_eq!(name, Value::String("bound foo".to_string()));
}
