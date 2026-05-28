//! Expression evaluation for interpreter

use super::*;
use crate::transpile::hir::PropKey;
use std::collections::HashMap;

pub fn evaluate_expr(expr: &Expr, ctx: &EvalContext) -> Value {
    match expr {
        Expr::Null => Value::Null,
        Expr::Undefined => Value::Undefined,
        Expr::Boolean(b) => Value::Bool(*b),
        Expr::Number(n) => Value::Number(*n),
        Expr::String(s) => Value::String(s.clone()),
        Expr::Ident { name } => ctx.scope.get(name).cloned().unwrap_or(Value::Undefined),
        Expr::Array { elems } => Value::Array(elems.iter().filter_map(|e| e.as_ref()).map(|e| evaluate_expr(e, ctx)).collect()),
        Expr::Object { props } => {
            let mut map = HashMap::new();
            for prop in props {
                if let ObjectProp::Init { key, value } = prop {
                    let key_str = match key {
                        PropKey::Ident(s) => s.clone(),
                        PropKey::String(s) => s.clone(),
                        PropKey::Number(n) => n.to_string(),
                        PropKey::Computed(_) => String::new(),
                    };
                    map.insert(key_str, evaluate_expr(value, ctx));
                }
            }
            Value::Object(map)
        }
        _ => Value::Undefined,
    }
}

pub fn apply_binary_op(op: &BinaryOp, left: Value, right: Value) -> Value {
    match op {
        BinaryOp::Add => add_values(left, right),
        BinaryOp::Sub => num_op(left, right, |a, b| a - b),
        BinaryOp::Mul => num_op(left, right, |a, b| a * b),
        BinaryOp::Div => num_op(left, right, |a, b| if b != 0.0 { a / b } else { 0.0 }),
        BinaryOp::Eq | BinaryOp::EqStrict => Value::Bool(left == right),
        BinaryOp::Ne | BinaryOp::NeStrict => Value::Bool(left != right),
        BinaryOp::Lt => num_bool(left, right, |a, b| a < b),
        BinaryOp::Le => num_bool(left, right, |a, b| a <= b),
        BinaryOp::Gt => num_bool(left, right, |a, b| a > b),
        BinaryOp::Ge => num_bool(left, right, |a, b| a >= b),
        _ => Value::Undefined,
    }
}

fn add_values(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => Value::Number(a + b),
        (Value::String(mut a), b) => { a.push_str(&format!("{:?}", b)); Value::String(a) }
        (a, Value::String(b)) => Value::String(format!("{:?}{}", a, b)),
        _ => Value::Undefined,
    }
}

fn num_op<F: Fn(f64, f64) -> f64>(left: Value, right: Value, f: F) -> Value {
    if let (Value::Number(a), Value::Number(b)) = (left, right) { Value::Number(f(a, b)) } else { Value::Undefined }
}

fn num_bool<F: Fn(f64, f64) -> bool>(left: Value, right: Value, f: F) -> Value {
    if let (Value::Number(a), Value::Number(b)) = (left, right) { Value::Bool(f(a, b)) } else { Value::Undefined }
}

pub fn apply_logical_op(op: &LogicalOp, left: Value, right: Value) -> Value {
    match op {
        LogicalOp::And => if let Value::Bool(true) = left { right } else { left },
        LogicalOp::Or => if let Value::Bool(true) = left { left } else { right },
        LogicalOp::NullishCoalesce => match left { Value::Null | Value::Undefined => right, _ => left },
    }
}
