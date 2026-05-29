//! Middleware runtime

use crate::transpile::hir::*;

pub fn execute_stmt(_stmt: &Stmt) -> Value {
    Value::Null
}
pub fn expr_to_value(_expr: &Expr) -> Value {
    Value::Null
}
pub fn apply_binary_op(_op: &BinaryOp, _left: Value, _right: Value) -> Value {
    Value::Null
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

pub struct MiddlewareExecutor;
pub struct MiddlewareOutcome;
pub struct MiddlewareDef;
