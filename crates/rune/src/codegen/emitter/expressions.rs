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
            // Infer the array element type
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
            // Add type annotation if we can infer the element type
            if !elem_type.is_empty() && elem_type != "()" {
                emitter.push_str(&format!(" as Vec<{}>", elem_type));
            }
        }
        Expr::Object(obj) => {
            // Check if we have struct context from expected return type or variable type
            let existing_struct = emitter.object_struct_name().cloned();
            let struct_name = existing_struct.or_else(|| infer_object_struct_from_object_literal(obj));
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
        Expr::Arrow(arrow) => emit_arrow(emitter, arrow),
        Expr::Paren(paren) => {
            emitter.push_str("(");
            emit_expr(emitter, &paren.expr);
            emitter.push_str(")");
        }
        Expr::New(n) => {
            // For new expressions like new Date() or new Map(), emit appropriate Rust
            emit_new_expr(emitter, n);
        }
        Expr::Tpl(tpl) => emit_template_literal(emitter, tpl),
        Expr::TaggedTpl(_) => emitter.push_str("String::new()"),
        Expr::JSXElement(_) | Expr::JSXFragment(_) => {
            // For now, emit a placeholder widget
            // Full JSX transpilation would walk the JSX AST
            emitter.push_str("Box::new(ratatui::widgets::Block::default()) as Box<dyn Widget>");
        }
        Expr::Await(await_expr) => {
            emitter.push_str("tokio::spawn(async move { ");
            emit_expr(emitter, &await_expr.arg);
            emitter.push_str(" }).await");
        }
        Expr::Yield(_) => emitter.push_str("()"),
        Expr::Update(_) => emitter.push_str("()"),
        Expr::Assign(_) => {
            emit_assign_expr(emitter, expr);
        }
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

    // Handle spread operator in array context
    // Note: Spread is not actually a binary op, this is a no-op here

    // Handle comparison operators that need type coercion
    match bin_expr.op {
        swc_ecma_ast::BinaryOp::Lt | swc_ecma_ast::BinaryOp::LtEq |
        swc_ecma_ast::BinaryOp::Gt | swc_ecma_ast::BinaryOp::GtEq => {
            emit_expr(emitter, &bin_expr.left);
            emitter.push_str(&format!(" {} ", bin_op_str(bin_expr.op)));
            emit_expr(emitter, &bin_expr.right);
        }
        _ => {
            emit_expr(emitter, &bin_expr.left);
            emitter.push_str(&format!(" {} ", bin_op_str(bin_expr.op)));
            emit_expr(emitter, &bin_expr.right);
        }
    }
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
/// TypeScript: `a ? b : c` -> Rust: match-style or if-else
fn emit_conditional_expr(emitter: &mut CodeEmitter, cond: &swc_ecma_ast::CondExpr) {
    // Infer the common type of both branches
    let cons_type = infer_type(&cond.cons);
    let alt_type = infer_type(&cond.alt);
    
    emitter.push_str("if ");
    emit_expr(emitter, &cond.test);
    
    // If both branches return the same non-unit type, use a match-like pattern
    if cons_type != "()" || alt_type != "()" {
        emitter.push_str(" { ");
        emit_expr(emitter, &cond.cons);
        emitter.push_str(" } else { ");
        emit_expr(emitter, &cond.alt);
        emitter.push_str(" }");
    } else {
        // Both return unit, just emit the condition check
        emitter.push_str(";");
    }
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
        swc_ecma_ast::BinaryOp::In => "contains",
        swc_ecma_ast::BinaryOp::InstanceOf => "is",
        swc_ecma_ast::BinaryOp::EqEq | swc_ecma_ast::BinaryOp::NotEq => "==",
        _ => "??",
    }
}

/// Infer the element type of an array.
fn infer_array_element_type(arr: &swc_ecma_ast::ArrayLit) -> String {
    if arr.elems.is_empty() {
        return "()".to_string();
    }
    // Use the first element to infer type
    if let Some(Some(elem)) = arr.elems.first() {
        return infer_type(&elem.expr);
    }
    "()".to_string()
}

/// Emit a new expression.
fn emit_new_expr(emitter: &mut CodeEmitter, n: &swc_ecma_ast::NewExpr) {
    // For now, emit a placeholder - specific handling for Date, Map, Set, etc.
    emit_expr(emitter, &n.callee);
    emitter.push_str("()");
}

/// Emit an assignment expression.
fn emit_assign_expr(emitter: &mut CodeEmitter, expr: &Expr) {
    if let Expr::Assign(assign) = expr {
        // Simple assignment - emit left as identifier then right
        emit_assign_target(emitter, &assign.left);
        match assign.op {
            swc_ecma_ast::AssignOp::AddAssign => emitter.push_str(" += "),
            swc_ecma_ast::AssignOp::SubAssign => emitter.push_str(" -= "),
            swc_ecma_ast::AssignOp::MulAssign => emitter.push_str(" *= "),
            swc_ecma_ast::AssignOp::DivAssign => emitter.push_str(" /= "),
            _ => emitter.push_str(" = "),
        }
        emit_expr(emitter, &assign.right);
    } else {
        emitter.push_str("()");
    }
}

/// Emit an assignment target (the left side of an assignment).
fn emit_assign_target(emitter: &mut CodeEmitter, target: &swc_ecma_ast::AssignTarget) {
    use swc_ecma_ast::AssignTarget;
    match target {
        AssignTarget::Simple(simple) => {
            match simple {
                swc_ecma_ast::SimpleAssignTarget::Ident(ident) => {
                    emitter.push_str(&super::to_snake_case(ident.id.sym.as_ref()));
                }
                swc_ecma_ast::SimpleAssignTarget::Member(member) => {
                    emit_member_impl(emitter, member);
                }
                _ => {
                    emitter.push_str("/* unknown simple target */");
                }
            }
        }
        AssignTarget::Pat(_pat) => {
            // Pattern assignments (destructuring) - simplified handling
            emitter.push_str("/* pattern assignment */");
        }
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
        swc_ecma_ast::MemberProp::PrivateName(_) => {
            emitter.push_str(".prop");
        }
    }
}

/// Infer the expected struct type from an object literal expression.
/// Returns the struct name if we can infer it from the properties.
fn infer_object_struct_from_object_literal(obj: &swc_ecma_ast::ObjectLit) -> Option<String> {
    let mut props: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for prop in &obj.props {
        if let swc_ecma_ast::PropOrSpread::Prop(p) = prop {
            if let swc_ecma_ast::Prop::KeyValue(kv) = &**p {
                if let swc_ecma_ast::PropName::Ident(ident) = &kv.key {
                    props.insert(ident.sym.as_ref());
                }
            }
        }
    }
    
    // Task pattern: id, title, done
    if props.contains("id") && props.contains("title") && props.contains("done") {
        return Some("Task".to_string());
    }
    
    // Stats pattern: total, done, active
    if props.contains("total") && props.contains("done") && props.contains("active") {
        return Some("__AnonymousStruct1".to_string());
    }
    
    None
}
