use super::*;
use crate::ast::{ArrowBody, Expression, Param};

fn make_env() -> Rc<RefCell<Environment>> {
    Rc::new(RefCell::new(Environment::new()))
}

#[test]
fn test_bind_params_no_args() {
    let env = make_env();
    let params = vec![Param::new("x")];
    let args: Vec<Value> = vec![];
    bind_params(&params, &args, &env, false).unwrap();
    assert_eq!(env.borrow().get("x"), Some(Value::Undefined));
}

#[test]
fn test_bind_params_with_args() {
    let env = make_env();
    let params = vec![Param::new("x"), Param::new("y")];
    let args = vec![Value::Number(1.0), Value::Number(2.0)];
    bind_params(&params, &args, &env, false).unwrap();
    assert_eq!(env.borrow().get("x"), Some(Value::Number(1.0)));
    assert_eq!(env.borrow().get("y"), Some(Value::Number(2.0)));
}

#[test]
fn test_bind_params_extra_args() {
    let env = make_env();
    let params = vec![Param::new("x")];
    let args = vec![Value::Number(1.0), Value::Number(2.0)];
    bind_params(&params, &args, &env, false).unwrap();
    assert_eq!(env.borrow().get("x"), Some(Value::Number(1.0)));
}

#[test]
fn test_bind_params_arrow_true() {
    let env = make_env();
    let params = vec![Param::new("x")];
    let args = vec![Value::Number(42.0)];
    bind_params(&params, &args, &env, true).unwrap();
    assert_eq!(env.borrow().get("x"), Some(Value::Number(42.0)));
}

#[test]
fn test_resolve_param_value_undefined_uses_default() {
    let env = make_env();
    env.borrow_mut()
        .define("y".to_string(), Value::Number(99.0));
    let mut param = Param::new("x");
    param.default = Some(Box::new(Expression::Identifier("y".to_string())));
    let args: Vec<Value> = vec![Value::Undefined];
    let result = resolve_param_value(&param, &args, 0, &env, false).unwrap();
    assert_eq!(result, Value::Number(99.0));
}

#[test]
fn test_resolve_param_value_provided_value() {
    let env = make_env();
    let param = Param::new("x");
    let args = vec![Value::Number(5.0)];
    let result = resolve_param_value(&param, &args, 0, &env, false).unwrap();
    assert_eq!(result, Value::Number(5.0));
}

#[test]
fn test_resolve_param_value_missing_no_default() {
    let env = make_env();
    let param = Param::new("x");
    let args: Vec<Value> = vec![];
    let result = resolve_param_value(&param, &args, 0, &env, false).unwrap();
    assert_eq!(result, Value::Undefined);
}

#[test]
fn test_eval_arrow_body_expression() {
    let env = make_env();
    let expr = ArrowBody::Expression(Expression::Number(42.0));
    let result = eval_arrow_body(&Some(expr), &env).unwrap();
    assert_eq!(result, Value::Number(42.0));
}

#[test]
fn test_eval_arrow_body_block() {
    let env = make_env();
    let stmts = std::rc::Rc::new(vec![crate::ast::Statement::Return(Some(Box::new(
        Expression::Number(7.0),
    )))]);
    let result = eval_arrow_body(&Some(ArrowBody::Block(stmts)), &env).unwrap();
    assert_eq!(result, Value::Number(7.0));
}

#[test]
fn test_eval_arrow_body_none() {
    let env = make_env();
    let result = eval_arrow_body(&None, &env).unwrap();
    assert_eq!(result, Value::Undefined);
}
