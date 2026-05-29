//! Expression conversion
// allow:complexity

use crate::transpile::hir;
use oxc_ast::ast::*;

pub fn convert_expr(expr: &Expression) -> Option<hir::Expr> {
    match expr {
        Expression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        Expression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        Expression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        Expression::NullLiteral(_) => Some(hir::Expr::Null),
        Expression::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        Expression::ArrayExpression(a) => Some(hir::Expr::Array { elems: arr_elems(&a) }),
        Expression::BinaryExpression(b) => conv_bin(b),
        Expression::LogicalExpression(l) => conv_log(l),
        Expression::ConditionalExpression(c) => conv_cond(c),
        Expression::AssignmentExpression(a) => conv_assign(a),
        Expression::ArrowFunctionExpression(a) => conv_arrow(a),
        Expression::CallExpression(c) => conv_call(c),
        Expression::UpdateExpression(u) => conv_update(u),
        Expression::UnaryExpression(u) => conv_unary(u),
        Expression::ParenthesizedExpression(p) => convert_expr(&p.expression),
        _ => None,
    }
}

fn arr_elems(a: &ArrayExpression) -> Vec<Option<hir::Expr>> {
    a.elements.iter().map(|e| e.as_expression().and_then(convert_expr)).collect()
}

fn conv_bin(b: &BinaryExpression) -> Option<hir::Expr> {
    let left = Box::new(convert_expr(&b.left)?);
    let right = Box::new(convert_expr(&b.right)?);
    let op = match b.operator {
        BinaryOperator::Addition => hir::BinaryOp::Add,
        BinaryOperator::Subtraction => hir::BinaryOp::Sub,
        BinaryOperator::Multiplication => hir::BinaryOp::Mul,
        BinaryOperator::Division => hir::BinaryOp::Div,
        BinaryOperator::Remainder => hir::BinaryOp::Mod,
        _ => return None,
    };
    Some(hir::Expr::Bin { op, left, right })
}

fn conv_log(l: &LogicalExpression) -> Option<hir::Expr> {
    let op = match l.operator {
        LogicalOperator::And => hir::LogicalOp::And,
        LogicalOperator::Or => hir::LogicalOp::Or,
        _ => return None,
    };
    Some(hir::Expr::Logical { op, left: Box::new(convert_expr(&l.left)?), right: Box::new(convert_expr(&l.right)?) })
}

fn conv_cond(c: &ConditionalExpression) -> Option<hir::Expr> {
    Some(hir::Expr::Cond { test: Box::new(convert_expr(&c.test)?), consequent: Box::new(convert_expr(&c.consequent)?), alternate: Box::new(convert_expr(&c.alternate)?) })
}

fn conv_assign(_a: &AssignmentExpression) -> Option<hir::Expr> {
    None // TODO: assignment operators not yet supported
}

fn conv_arrow(a: &ArrowFunctionExpression) -> Option<hir::Expr> {
    let params: Vec<hir::Param> = a.params.items.iter().filter_map(|p| {
        if let BindingPattern::BindingIdentifier(i) = &p.pattern {
            Some(hir::Param { name: i.name.to_string(), type_: None, default: None, optional: p.optional, pattern: None })
        } else { None }
    }).collect();
    let body = if a.expression {
        if let Some(stmt) = a.body.statements.first() {
            if let Statement::ExpressionStatement(e) = stmt {
                convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined)
            } else { hir::Expr::Undefined }
        } else { hir::Expr::Undefined }
    } else { hir::Expr::Undefined };
    Some(hir::Expr::ArrowFunction { params, body: Box::new(body), is_async: a.r#async })
}

fn conv_call(c: &CallExpression) -> Option<hir::Expr> {
    let callee = Box::new(convert_expr(&c.callee)?);
    let args: Vec<hir::Expr> = c.arguments.iter().filter_map(|a| a.as_expression().and_then(convert_expr)).collect();
    Some(hir::Expr::Call { callee, arguments: args })
}

fn conv_update(_u: &UpdateExpression) -> Option<hir::Expr> {
    None // TODO: update expressions not fully supported
}

fn conv_unary(u: &UnaryExpression) -> Option<hir::Expr> {
    let op = match u.operator {
        UnaryOperator::LogicalNot => hir::UnaryOp::Not,
        UnaryOperator::BitwiseNot => hir::UnaryOp::BitNot,
        UnaryOperator::Typeof => hir::UnaryOp::Typeof,
        UnaryOperator::Void => hir::UnaryOp::Void,
        UnaryOperator::Delete => hir::UnaryOp::Delete,
        _ => return None,
    };
    Some(hir::Expr::Unary { op, arg: Box::new(convert_expr(&u.argument)?), prefix: true })
}
