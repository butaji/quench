//! Expression conversion

use crate::transpile::hir;
use oxc_ast::ast::*;

pub fn convert_expr(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        Expression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        Expression::NullLiteral(_) => Some(hir::Expr::Null),
        Expression::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        Expression::ArrayExpression(_) => Some(hir::Expr::Array { elems: vec![] }),
        Expression::BinaryExpression(b) => convert_binary(b),
        _ => None,
    }
}

fn convert_binary(b: &BinaryExpression) -> Option<hir::Expr> {
    let op = match b.operator {
        BinaryOperator::Addition => hir::BinaryOp::Add,
        BinaryOperator::Subtraction => hir::BinaryOp::Sub,
        BinaryOperator::Multiplication => hir::BinaryOp::Mul,
        BinaryOperator::Division => hir::BinaryOp::Div,
        _ => return None,
    };
    Some(hir::Expr::Bin {
        op,
        left: Box::new(convert_expr(&b.left)?),
        right: Box::new(convert_expr(&b.right)?),
    })
}
