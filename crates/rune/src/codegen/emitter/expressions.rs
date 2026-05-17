//! # Expression Emitter
//!
//! Emits Rust expressions from TypeScript AST.

use super::literals::emit_template_literal;
use super::utils::infer_struct_from_object;
use super::{emit_call, emit_lit, emit_member, emit_object, infer_type, CodeEmitter};
use swc_ecma_ast::Expr;

/// Emit an expression.
pub fn emit_expr(emitter: &mut CodeEmitter, expr: &Expr) {
    match expr {
        Expr::Lit(lit) => emit_lit(emitter, lit),
        Expr::Ident(ident) => emit_ident(emitter, ident),
        Expr::Bin(bin_expr) => emit_bin_expr(emitter, bin_expr),
        Expr::Unary(unary_expr) => emit_unary_expr(emitter, unary_expr),
        Expr::Call(call_expr) => emit_call(emitter, call_expr),
        Expr::Member(member_expr) => emit_member(emitter, member_expr),
        Expr::Cond(cond_expr) => emit_conditional_expr(emitter, cond_expr),
        Expr::Array(arr) => emit_array_expr(emitter, arr),
        Expr::Object(obj) => emit_object_expr(emitter, obj),
        Expr::Arrow(arrow) => emit_arrow(emitter, arrow),
        Expr::Paren(paren) => emit_paren_expr(emitter, paren),
        Expr::New(n) => emit_new_expr(emitter, n),
        Expr::Tpl(tpl) => emit_template_literal(emitter, tpl),
        Expr::TaggedTpl(_) => emitter.push_str("String::new()"),
        Expr::JSXElement(_) | Expr::JSXFragment(_) => emit_jsx_placeholder(emitter),
        Expr::Await(await_expr) => emit_await_expr(emitter, await_expr),
        Expr::Yield(_) => emitter.push_str("()"),
        Expr::Update(update_expr) => emit_update_expr(emitter, update_expr),
        Expr::Assign(_) => emit_assign_expr(emitter, expr),
        Expr::Seq(_) => emitter.push_str("()"),
        _ => emitter.push_str("()"),
    }
}

/// Emit an identifier.
fn emit_ident(emitter: &mut CodeEmitter, ident: &swc_ecma_ast::Ident) {
    emitter.push_str(&super::to_snake_case(ident.sym.as_ref()));
}

/// Emit binary expression with proper type coercion.
fn emit_bin_expr(emitter: &mut CodeEmitter, bin_expr: &swc_ecma_ast::BinExpr) {
    let left_type = infer_type(&bin_expr.left);
    let right_type = infer_type(&bin_expr.right);

    if bin_expr.op == swc_ecma_ast::BinaryOp::Add
        && (left_type == "String" || right_type == "String")
    {
        emit_string_concat(emitter, bin_expr);
        return;
    }

    // Check for integer division warning
    if bin_expr.op == swc_ecma_ast::BinaryOp::Div
        && is_integer_type(&left_type)
        && is_integer_type(&right_type)
    {
        emitter.emit_warning(
            "integer-division",
            "Integer division produces i32 result (5 / 2 == 2), not f64. \
             Use 5.0 or explicit cast for float division.",
        );
    }

    emit_binary_op(emitter, bin_expr);
}

/// Check if a type string represents an integer.
#[must_use]
fn is_integer_type(ty: &str) -> bool {
    matches!(ty, "i8" | "i16" | "i32" | "i64" | "isize" | "usize")
}

fn emit_string_concat(emitter: &mut CodeEmitter, bin_expr: &swc_ecma_ast::BinExpr) {
    emitter.push_str("format!(\"{}{}\", ");
    emit_expr(emitter, &bin_expr.left);
    emitter.push_str(", ");
    emit_expr(emitter, &bin_expr.right);
    emitter.push_str(")");
}

fn emit_binary_op(emitter: &mut CodeEmitter, bin_expr: &swc_ecma_ast::BinExpr) {
    let needs_parens = matches!(
        bin_expr.op,
        swc_ecma_ast::BinaryOp::Lt
            | swc_ecma_ast::BinaryOp::LtEq
            | swc_ecma_ast::BinaryOp::Gt
            | swc_ecma_ast::BinaryOp::GtEq
    );

    if needs_parens {
        emitter.push_str("(");
    }
    emit_expr(emitter, &bin_expr.left);
    emitter.push_str(&format!(" {} ", bin_op_str(bin_expr.op)));
    emit_expr(emitter, &bin_expr.right);
    if needs_parens {
        emitter.push_str(")");
    }
}

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
        swc_ecma_ast::BinaryOp::BitXor => "^",
        swc_ecma_ast::BinaryOp::LShift => "<<",
        swc_ecma_ast::BinaryOp::RShift => ">>",
        swc_ecma_ast::BinaryOp::EqEq | swc_ecma_ast::BinaryOp::NotEq => "==",
        _ => "??",
    }
}

/// Emit unary expression with proper handling.
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
fn emit_conditional_expr(emitter: &mut CodeEmitter, cond: &swc_ecma_ast::CondExpr) {
    let cons_type = infer_type(&cond.cons);
    let alt_type = infer_type(&cond.alt);
    let result_type = resolve_result_type(&cons_type, &alt_type);

    if needs_temp_var(&result_type) {
        emit_conditional_block(emitter, cond);
    } else {
        emit_conditional_expr_simple(emitter, cond);
    }
}

fn resolve_result_type(cons_type: &str, alt_type: &str) -> String {
    if cons_type == alt_type {
        cons_type.to_string()
    } else if cons_type == "()" {
        alt_type.to_string()
    } else if alt_type == "()" {
        cons_type.to_string()
    } else {
        "()".to_string()
    }
}

fn needs_temp_var(result_type: &str) -> bool {
    !result_type.is_empty()
        && result_type != "()"
        && result_type != "f64"
        && result_type != "bool"
        && result_type != "String"
        && result_type != "i32"
        && result_type != "usize"
}

fn emit_conditional_block(emitter: &mut CodeEmitter, cond: &swc_ecma_ast::CondExpr) {
    emitter.push_str("{ if ");
    emit_expr(emitter, &cond.test);
    emitter.push_str(" { ");
    emit_expr(emitter, &cond.cons);
    emitter.push_str(" } else { ");
    emit_expr(emitter, &cond.alt);
    emitter.push_str(" } }");
}

fn emit_conditional_expr_simple(emitter: &mut CodeEmitter, cond: &swc_ecma_ast::CondExpr) {
    emitter.push_str("if ");
    emit_expr(emitter, &cond.test);
    emitter.push_str(" { ");
    emit_expr(emitter, &cond.cons);
    emitter.push_str(" } else { ");
    emit_expr(emitter, &cond.alt);
    emitter.push_str(" }");
}

/// Emit an array literal expression.
fn emit_array_expr(emitter: &mut CodeEmitter, arr: &swc_ecma_ast::ArrayLit) {
    let elem_type = infer_array_element_type(arr);
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
    if !elem_type.is_empty() && elem_type != "()" {
        emitter.push_str(&format!(" as Vec<{}>", elem_type));
    }
}

fn infer_array_element_type(arr: &swc_ecma_ast::ArrayLit) -> String {
    if arr.elems.is_empty() {
        return "()".to_string();
    }
    if let Some(Some(elem)) = arr.elems.first() {
        return infer_type(&elem.expr);
    }
    "()".to_string()
}

/// Emit an object literal expression.
fn emit_object_expr(emitter: &mut CodeEmitter, obj: &swc_ecma_ast::ObjectLit) {
    let existing_struct = emitter.object_struct_name().cloned();
    let struct_name = existing_struct.or_else(|| infer_struct_from_object(obj));

    if let Some(name) = struct_name {
        let prev_struct = emitter.object_struct_name().cloned();
        emitter.set_object_struct(Some(name));
        emit_object(emitter, obj);
        if let Some(prev) = prev_struct {
            emitter.set_object_struct(Some(prev));
        } else {
            emitter.set_object_struct(None);
        }
    } else {
        emit_object(emitter, obj);
    }
}

/// Emit a parenthesized expression.
fn emit_paren_expr(emitter: &mut CodeEmitter, paren: &swc_ecma_ast::ParenExpr) {
    emitter.push_str("(");
    emit_expr(emitter, &paren.expr);
    emitter.push_str(")");
}

/// Emit a JSX placeholder widget.
fn emit_jsx_placeholder(emitter: &mut CodeEmitter) {
    emitter.push_str("Box::new(ratatui::widgets::Block::default()) as Box<dyn Widget>");
}

/// Emit an await expression.
fn emit_await_expr(emitter: &mut CodeEmitter, await_expr: &swc_ecma_ast::AwaitExpr) {
    emitter.push_str("tokio::spawn(async move { ");
    emit_expr(emitter, &await_expr.arg);
    emitter.push_str(" }).await");
}

/// Emit an arrow function with proper closure syntax.
fn emit_arrow(emitter: &mut CodeEmitter, arrow: &swc_ecma_ast::ArrowExpr) {
    let params = extract_arrow_params(arrow);

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

/// Extract parameters from an arrow function.
fn extract_arrow_params(arrow: &swc_ecma_ast::ArrowExpr) -> Vec<String> {
    arrow
        .params
        .iter()
        .filter_map(|p| {
            if let swc_ecma_ast::Pat::Ident(ident) = p {
                Some(super::to_snake_case(ident.id.sym.as_ref()))
            } else {
                None
            }
        })
        .collect()
}

/// Emit a new expression.
fn emit_new_expr(emitter: &mut CodeEmitter, n: &swc_ecma_ast::NewExpr) {
    emit_expr(emitter, &n.callee);
    emitter.push_str("()");
}

/// Emit an assignment expression.
fn emit_assign_expr(emitter: &mut CodeEmitter, expr: &Expr) {
    if let Expr::Assign(assign) = expr {
        emit_assign_target(emitter, &assign.left);
        emit_assign_op(emitter, assign.op);
        emit_expr(emitter, &assign.right);
    } else {
        emitter.push_str("()");
    }
}

/// Emit an update expression (i++, i--, ++i, --i).
fn emit_update_expr(emitter: &mut CodeEmitter, update: &swc_ecma_ast::UpdateExpr) {
    // For Rust, convert i++ to i += 1
    emit_expr(emitter, &update.arg);
    match update.op {
        swc_ecma_ast::UpdateOp::PlusPlus => emitter.push_str(" += 1"),
        swc_ecma_ast::UpdateOp::MinusMinus => emitter.push_str(" -= 1"),
    }
}

/// Emit assignment operator.
fn emit_assign_op(emitter: &mut CodeEmitter, op: swc_ecma_ast::AssignOp) {
    match op {
        swc_ecma_ast::AssignOp::AddAssign => emitter.push_str(" += "),
        swc_ecma_ast::AssignOp::SubAssign => emitter.push_str(" -= "),
        swc_ecma_ast::AssignOp::MulAssign => emitter.push_str(" *= "),
        swc_ecma_ast::AssignOp::DivAssign => emitter.push_str(" /= "),
        _ => emitter.push_str(" = "),
    }
}

/// Emit an assignment target (the left side of an assignment).
fn emit_assign_target(emitter: &mut CodeEmitter, target: &swc_ecma_ast::AssignTarget) {
    match target {
        swc_ecma_ast::AssignTarget::Simple(simple) => emit_simple_target(emitter, simple),
        swc_ecma_ast::AssignTarget::Pat(_pat) => {
            emitter.push_str("/* pattern assignment */");
        }
    }
}

/// Emit a simple assignment target.
fn emit_simple_target(emitter: &mut CodeEmitter, simple: &swc_ecma_ast::SimpleAssignTarget) {
    match simple {
        swc_ecma_ast::SimpleAssignTarget::Ident(ident) => {
            emitter.push_str(&super::to_snake_case(ident.id.sym.as_ref()));
        }
        swc_ecma_ast::SimpleAssignTarget::Member(member) => emit_member_impl(emitter, member),
        _ => emitter.push_str("/* unknown simple target */"),
    }
}

/// Emit a member expression (for assignment targets).
fn emit_member_impl(emitter: &mut CodeEmitter, member: &swc_ecma_ast::MemberExpr) {
    emit_expr(emitter, &member.obj);
    match &member.prop {
        swc_ecma_ast::MemberProp::Ident(ident) => {
            emitter.push_str(".");
            emitter.push_str(ident.sym.as_ref());
        }
        swc_ecma_ast::MemberProp::Computed(comp) => {
            emitter.push_str("[");
            emit_expr(emitter, &comp.expr);
            emitter.push_str("]");
        }
        swc_ecma_ast::MemberProp::PrivateName(_) => emitter.push_str(".prop"),
    }
}
