//! # Expression Emitter
//!
//! Emits Rust expressions from TypeScript AST.

use super::{CodeEmitter, emit_call, emit_lit, emit_member, emit_object, infer_type};
use super::literals::emit_template_literal;
use swc_ecma_ast::Expr;

/// Emit an expression.
pub fn emit_expr(emitter: &mut CodeEmitter, expr: &Expr) {
    match expr {
        Expr::Lit(lit) => emit_lit(emitter, lit),
        Expr::Ident(ident) => {
            emitter.push_str(&super::to_snake_case(ident.sym.as_ref()));
        }
        Expr::Bin(bin_expr) => emit_bin_expr(emitter, bin_expr),
        Expr::Unary(unary_expr) => emit_unary_expr(emitter, unary_expr),
        Expr::Call(call_expr) => emit_call(emitter, call_expr),
        Expr::Member(member_expr) => emit_member(emitter, member_expr),
        Expr::Cond(cond_expr) => emit_conditional_expr(emitter, cond_expr),
        Expr::Array(arr) => {
            emitter.push_str("vec![");
            for (i, elem) in arr.elems.iter().enumerate() {
                if i > 0 {
                    emitter.push_str(", ");
                }
                if let Some(elem) = elem {
                    emit_expr(emitter, &elem.expr);
                }
            }
            emitter.push_str("]");
        }
        Expr::Object(obj) => emit_object(emitter, obj),
        Expr::Arrow(arrow) => emit_arrow(emitter, arrow),
        Expr::Paren(paren) => {
            emitter.push_str("(");
            emit_expr(emitter, &paren.expr);
            emitter.push_str(")");
        }
        Expr::New(_n) => emitter.push_str("/* new */ ()"),
        Expr::Tpl(tpl) => emit_template_literal(emitter, tpl),
        Expr::TaggedTpl(_) => emitter.push_str("String::new()"),
        Expr::JSXElement(_) | Expr::JSXFragment(_) => emitter.push_str("()"),
        Expr::Await(await_expr) => {
            emitter.push_str("tokio::spawn(async move { ");
            emit_expr(emitter, &await_expr.arg);
            emitter.push_str(" }).await");
        }
        Expr::Yield(_) => emitter.push_str("()"),
        Expr::Update(_) => emitter.push_str("()"),
        Expr::Assign(_) => emitter.push_str("()"),
        Expr::Seq(_) => emitter.push_str("()"),
        _ => emitter.push_str("()"),
    }
}

/// Emit a binary expression with proper type coercion.
fn emit_bin_expr(emitter: &mut CodeEmitter, bin_expr: &swc_ecma_ast::BinExpr) {
    let left_type = infer_type(&bin_expr.left);
    let right_type = infer_type(&bin_expr.right);

    if bin_expr.op == swc_ecma_ast::BinaryOp::Add
        && (left_type == "String" || right_type == "String")
    {
        emitter.push_str("format!(\"{}{}\", ");
        emit_expr(emitter, &bin_expr.left);
        emitter.push_str(", ");
        emit_expr(emitter, &bin_expr.right);
        emitter.push_str(")");
        return;
    }

    emit_expr(emitter, &bin_expr.left);
    emitter.push_str(&format!(" {} ", bin_op_str(bin_expr.op)));
    emit_expr(emitter, &bin_expr.right);
}

/// Emit a unary expression with proper handling.
fn emit_unary_expr(emitter: &mut CodeEmitter, unary_expr: &swc_ecma_ast::UnaryExpr) {
    match unary_expr.op {
        swc_ecma_ast::UnaryOp::Minus => {
            emitter.push_str("-");
            emit_expr(emitter, &unary_expr.arg);
        }
        swc_ecma_ast::UnaryOp::Plus => emit_expr(emitter, &unary_expr.arg),
        swc_ecma_ast::UnaryOp::Bang => {
            emitter.push_str("!");
            emit_expr(emitter, &unary_expr.arg);
        }
        swc_ecma_ast::UnaryOp::TypeOf => emitter.push_str("\"unknown\""),
        swc_ecma_ast::UnaryOp::Void => emitter.push_str("()"),
        swc_ecma_ast::UnaryOp::Delete => emitter.push_str("/* delete */ ()"),
        swc_ecma_ast::UnaryOp::Tilde => {
            emitter.push_str("!");
            emit_expr(emitter, &unary_expr.arg);
        }
    }
}

/// Emit a conditional (ternary) expression properly.
/// TypeScript: `a ? b : c` -> Rust: `if a { b } else { c }`
fn emit_conditional_expr(emitter: &mut CodeEmitter, cond: &swc_ecma_ast::CondExpr) {
    emitter.push_str("if ");
    emit_expr(emitter, &cond.test);
    emitter.push_str(" { ");
    emit_expr(emitter, &cond.cons);
    emitter.push_str(" } else { ");
    emit_expr(emitter, &cond.alt);
    emitter.push_str(" }");
}

/// Emit an arrow function with proper closure syntax.
fn emit_arrow(emitter: &mut CodeEmitter, arrow: &swc_ecma_ast::ArrowExpr) {
    let params: Vec<_> = arrow
        .params
        .iter()
        .filter_map(|p| {
            if let swc_ecma_ast::Pat::Ident(ident) = p {
                Some(super::to_snake_case(ident.id.sym.as_ref()))
            } else {
                None
            }
        })
        .collect();

    if params.is_empty() {
        emitter.push_str("|| ");
    } else {
        emitter.push_str(&format!("|{}| ", params.join(", ")));
    }

    match &*arrow.body {
        swc_ecma_ast::BlockStmtOrExpr::Expr(e) => emit_expr(emitter, e),
        swc_ecma_ast::BlockStmtOrExpr::BlockStmt(block) => {
            emitter.push_str("{ ");
            for s in &block.stmts {
                super::emit_single_stmt(emitter, s);
            }
            emitter.push_str(" }");
        }
    }
}

/// Get binary operator string.
fn bin_op_str(op: swc_ecma_ast::BinaryOp) -> &'static str {
    match op {
        swc_ecma_ast::BinaryOp::Add => "+",
        swc_ecma_ast::BinaryOp::Sub => "-",
        swc_ecma_ast::BinaryOp::Mul => "*",
        swc_ecma_ast::BinaryOp::Div => "/",
        swc_ecma_ast::BinaryOp::Mod => "%",
        swc_ecma_ast::BinaryOp::EqEqEq => "==",
        swc_ecma_ast::BinaryOp::NotEqEq => "!=",
        swc_ecma_ast::BinaryOp::Lt => "<",
        swc_ecma_ast::BinaryOp::LtEq => "<=",
        swc_ecma_ast::BinaryOp::Gt => ">",
        swc_ecma_ast::BinaryOp::GtEq => ">=",
        swc_ecma_ast::BinaryOp::LogicalAnd => "&&",
        swc_ecma_ast::BinaryOp::LogicalOr => "||",
        swc_ecma_ast::BinaryOp::BitAnd => "&",
        swc_ecma_ast::BinaryOp::BitOr => "|",
        swc_ecma_ast::BinaryOp::BitXor => "^",
        swc_ecma_ast::BinaryOp::LShift => "<<",
        swc_ecma_ast::BinaryOp::RShift => ">>",
        swc_ecma_ast::BinaryOp::In => "in",
        swc_ecma_ast::BinaryOp::InstanceOf => "is",
        _ => "??",
    }
}
