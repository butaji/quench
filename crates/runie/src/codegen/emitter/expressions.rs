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
        Expr::JSXElement(elem) => emit_jsx_element(emitter, elem),
        Expr::JSXFragment(frag) => emit_jsx_fragment(emitter, frag),
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

    // Handle length comparison: usize == i32 needs cast on the integer side
    if is_usize_type(&left_type) && is_integer_type(&right_type) {
        emit_usize_comparison(emitter, &bin_expr.left, &bin_expr.right, bin_expr.op);
        return;
    }
    if is_integer_type(&left_type) && is_usize_type(&right_type) {
        emit_usize_comparison_rhs(emitter, &bin_expr.left, &bin_expr.right, bin_expr.op);
        return;
    }

    emit_binary_op(emitter, bin_expr);
}

/// Check if a type is usize (from .length, etc.).
#[must_use]
fn is_usize_type(ty: &str) -> bool {
    ty == "usize"
}

/// Emit comparison where left side is usize and right is integer.
fn emit_usize_comparison(
    emitter: &mut CodeEmitter,
    left: &swc_ecma_ast::Expr,
    right: &swc_ecma_ast::Expr,
    op: swc_ecma_ast::BinaryOp,
) {
    // left is usize, right is integer
    // Emit: left == right as usize
    emitter.push_str("(");
    emit_expr(emitter, left);
    emitter.push_str(&format!(") {} (", bin_op_str(op)));
    emit_expr(emitter, right);
    emitter.push_str(" as usize)");
}

/// Emit comparison where right side is usize and left is integer.
fn emit_usize_comparison_rhs(
    emitter: &mut CodeEmitter,
    left: &swc_ecma_ast::Expr,
    right: &swc_ecma_ast::Expr,
    op: swc_ecma_ast::BinaryOp,
) {
    // left is integer, right is usize
    // Emit: left as usize == right
    emitter.push_str("(");
    emit_expr(emitter, left);
    emitter.push_str(" as usize) ");
    emitter.push_str(bin_op_str(op));
    emitter.push_str(" ");
    emit_expr(emitter, right);
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

// ------------------------------------------------------------------
// Ratatui JSX emitter
// ------------------------------------------------------------------

/// Emit a JSX element as a ratatui widget builder chain.
fn emit_jsx_element(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    let tag = jsx_tag_name(&elem.opening.name);
    match tag.as_str() {
        "Paragraph" => emit_paragraph_element(emitter, elem),
        "Block" => emit_block_element(emitter, elem),
        "List" => emit_list_element(emitter, elem),
        "ListItem" => emit_list_item_element(emitter, elem),
        _ => emit_generic_widget_element(emitter, elem, &tag),
    }
}

/// Emit a JSX fragment as a `vec!` of its children.
fn emit_jsx_fragment(emitter: &mut CodeEmitter, frag: &swc_ecma_ast::JSXFragment) {
    let child_exprs: Vec<String> = frag
        .children
        .iter()
        .filter_map(jsx_child_to_string)
        .collect();
    emitter.push_str("vec![");
    for (i, child) in child_exprs.iter().enumerate() {
        if i > 0 {
            emitter.push_str(", ");
        }
        emitter.push_str(child);
    }
    emitter.push_str("]");
}

// -- Paragraph -----------------------------------------------------

fn emit_paragraph_element(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    emitter.push_str("Paragraph::new(");
    if let Some(text_expr) = get_jsx_text_attr(elem) {
        emit_expr(emitter, text_expr);
    } else {
        emit_jsx_text_content(emitter, elem);
    }
    emitter.push_str(")");
    emit_jsx_builder_chain(emitter, elem, &["text"]);
}

// -- Block ---------------------------------------------------------

fn emit_block_element(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    if let Some(child) = find_single_widget_child(elem) {
        // Block wraps a child widget: emit child + .block(...)
        emit_jsx_element(emitter, child);
        emitter.push_str(".block(");
        emit_block_builder(emitter, elem);
        emitter.push_str(")");
    } else {
        emit_block_builder(emitter, elem);
    }
}

fn emit_block_builder(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    let has_borders = has_jsx_attr(elem, "borders");
    if has_borders {
        emitter.push_str("Block::new()");
    } else {
        emitter.push_str("Block::bordered()");
    }
    emit_jsx_builder_chain(emitter, elem, &[]);
}

// -- List ----------------------------------------------------------

fn emit_list_element(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    emitter.push_str("List::new(");
    if let Some(expr_child) = find_expr_child(elem) {
        // e.g. {items.map(...)} → use the expression directly
        emit_expr(emitter, expr_child);
    } else {
        emitter.push_str("vec![");
        let mut first = true;
        for child in &elem.children {
            if let swc_ecma_ast::JSXElementChild::JSXElement(child_elem) = child {
                if !first {
                    emitter.push_str(", ");
                }
                first = false;
                emit_jsx_element(emitter, child_elem);
            }
        }
        emitter.push_str("]");
    }
    emitter.push_str(")");
    emit_jsx_builder_chain(emitter, elem, &["selected"]);
}

// -- ListItem ------------------------------------------------------

fn emit_list_item_element(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    emitter.push_str("ListItem::new(");
    emit_jsx_text_content(emitter, elem);
    emitter.push_str(")");
    emit_jsx_builder_chain(emitter, elem, &[]);
}

// -- Generic fallback ----------------------------------------------

fn emit_generic_widget_element(
    emitter: &mut CodeEmitter,
    elem: &swc_ecma_ast::JSXElement,
    tag: &str,
) {
    emitter.push_str(&format!("{tag}::new("));
    if let Some(text_expr) = get_jsx_text_attr(elem) {
        emit_expr(emitter, text_expr);
    } else {
        emit_jsx_text_content(emitter, elem);
    }
    emitter.push_str(")");
    emit_jsx_builder_chain(emitter, elem, &["text"]);
}

// -- Builder chain emission ----------------------------------------

/// Emit `.setter(value)` builder methods for every JSX attribute except those
/// listed in `skip`.
fn emit_jsx_builder_chain(
    emitter: &mut CodeEmitter,
    elem: &swc_ecma_ast::JSXElement,
    skip: &[&str],
) {
    for attr in &elem.opening.attrs {
        if let swc_ecma_ast::JSXAttrOrSpread::JSXAttr(attr) = attr {
            let key = jsx_attr_name(&attr.name);
            if skip.contains(&key.as_str()) {
                continue;
            }
            if let Some(setter) = build_ratatui_setter(&key) {
                emitter.push_str(&setter);
                if let Some(value) = &attr.value {
                    emit_ratatui_attr_value(emitter, &key, value);
                }
                emitter.push_str(")");
            }
        }
    }
}

fn build_ratatui_setter(key: &str) -> Option<String> {
    match key {
        "title" => Some(".title(".to_string()),
        "borders" => Some(".borders(".to_string()),
        "border_type" => Some(".border_type(".to_string()),
        "style" => Some(".style(".to_string()),
        "alignment" | "align" => Some(".alignment(".to_string()),
        "wrap" => Some(".wrap(".to_string()),
        "highlight_symbol" => Some(".highlight_symbol(".to_string()),
        "padding" => Some(".padding(".to_string()),
        _ => None,
    }
}

fn emit_ratatui_attr_value(
    emitter: &mut CodeEmitter,
    key: &str,
    value: &swc_ecma_ast::JSXAttrValue,
) {
    match value {
        swc_ecma_ast::JSXAttrValue::Str(s) => {
            let mapped = map_ratatui_str_value(key, s.value.as_str().unwrap_or(""));
            emitter.push_str(&mapped);
        }
        swc_ecma_ast::JSXAttrValue::JSXExprContainer(cont) => {
            if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                emit_expr(emitter, expr);
            }
        }
        swc_ecma_ast::JSXAttrValue::JSXElement(elem) => {
            emit_jsx_element(emitter, elem);
        }
        swc_ecma_ast::JSXAttrValue::JSXFragment(frag) => {
            emit_jsx_fragment(emitter, frag);
        }
    }
}

fn map_ratatui_str_value(key: &str, value: &str) -> String {
    match key {
        "borders" => match value {
            "ALL" => "Borders::ALL".to_string(),
            "NONE" => "Borders::NONE".to_string(),
            "LEFT" => "Borders::LEFT".to_string(),
            "RIGHT" => "Borders::RIGHT".to_string(),
            "TOP" => "Borders::TOP".to_string(),
            "BOTTOM" => "Borders::BOTTOM".to_string(),
            _ => format!("Borders::{value}"),
        },
        "border_type" => match value {
            "plain" | "single" => "BorderType::Plain".to_string(),
            "rounded" => "BorderType::Rounded".to_string(),
            "double" => "BorderType::Double".to_string(),
            "thick" => "BorderType::Thick".to_string(),
            _ => format!("BorderType::{value}"),
        },
        "alignment" | "align" => match value {
            "left" => "Alignment::Left".to_string(),
            "center" => "Alignment::Center".to_string(),
            "right" => "Alignment::Right".to_string(),
            _ => format!("Alignment::{value}"),
        },
        _ => format!("{:?}", value),
    }
}

// -- Helpers -------------------------------------------------------

/// Extract the tag name from a JSX element name.
fn jsx_tag_name(name: &swc_ecma_ast::JSXElementName) -> String {
    match name {
        swc_ecma_ast::JSXElementName::Ident(ident) => ident.sym.to_string(),
        _ => "Unknown".to_string(),
    }
}

fn jsx_attr_name(name: &swc_ecma_ast::JSXAttrName) -> String {
    match name {
        swc_ecma_ast::JSXAttrName::Ident(ident) => ident.sym.to_string(),
        swc_ecma_ast::JSXAttrName::JSXNamespacedName(ns) => {
            format!("{}_{}", ns.ns.sym, ns.name.sym)
        }
    }
}

fn has_jsx_attr(elem: &swc_ecma_ast::JSXElement, key: &str) -> bool {
    elem.opening.attrs.iter().any(|attr| {
        if let swc_ecma_ast::JSXAttrOrSpread::JSXAttr(attr) = attr {
            jsx_attr_name(&attr.name) == key
        } else {
            false
        }
    })
}

fn get_jsx_text_attr(elem: &swc_ecma_ast::JSXElement) -> Option<&swc_ecma_ast::Expr> {
    elem.opening.attrs.iter().find_map(|attr| {
        if let swc_ecma_ast::JSXAttrOrSpread::JSXAttr(attr) = attr {
            if jsx_attr_name(&attr.name) == "text" {
                if let Some(swc_ecma_ast::JSXAttrValue::JSXExprContainer(cont)) = &attr.value {
                    if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                        return Some(expr.as_ref());
                    }
                }
            }
        }
        None
    })
}

fn find_single_widget_child(elem: &swc_ecma_ast::JSXElement) -> Option<&swc_ecma_ast::JSXElement> {
    let widgets: Vec<&swc_ecma_ast::JSXElement> = elem
        .children
        .iter()
        .filter_map(|child| match child {
            swc_ecma_ast::JSXElementChild::JSXElement(e) => Some(e.as_ref()),
            _ => None,
        })
        .collect();
    if widgets.len() == 1 {
        widgets.first().copied()
    } else {
        None
    }
}

fn find_expr_child(elem: &swc_ecma_ast::JSXElement) -> Option<&swc_ecma_ast::Expr> {
    elem.children.iter().find_map(|child| match child {
        swc_ecma_ast::JSXElementChild::JSXExprContainer(cont) => {
            if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                Some(expr.as_ref())
            } else {
                None
            }
        }
        _ => None,
    })
}

/// Emit text/expression children as a single Rust expression.
fn emit_jsx_text_content(emitter: &mut CodeEmitter, elem: &swc_ecma_ast::JSXElement) {
    let parts: Vec<String> = elem
        .children
        .iter()
        .filter_map(|child| match child {
            swc_ecma_ast::JSXElementChild::JSXText(text) => {
                let trimmed = text.value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(format!("{:?}", trimmed))
                }
            }
            swc_ecma_ast::JSXElementChild::JSXExprContainer(cont) => {
                if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                    Some(expr_to_string(expr))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    match parts.len() {
        0 => emitter.push_str("\"\""),
        1 => emitter.push_str(&parts[0]),
        _ => {
            let fmt = parts.iter().map(|_| "{}").collect::<String>();
            emitter.push_str(&format!("format!(\"{fmt}\", "));
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    emitter.push_str(", ");
                }
                emitter.push_str(part);
            }
            emitter.push_str(")");
        }
    }
}

/// Convert a JSX child to a Rust expression string.
fn jsx_child_to_string(child: &swc_ecma_ast::JSXElementChild) -> Option<String> {
    match child {
        swc_ecma_ast::JSXElementChild::JSXText(text) => {
            let trimmed = text.value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("{:?}", trimmed))
            }
        }
        swc_ecma_ast::JSXElementChild::JSXExprContainer(cont) => {
            if let swc_ecma_ast::JSXExpr::Expr(expr) = &cont.expr {
                Some(expr_to_string(expr))
            } else {
                None
            }
        }
        swc_ecma_ast::JSXElementChild::JSXElement(elem) => {
            Some(jsx_element_to_string(elem))
        }
        swc_ecma_ast::JSXElementChild::JSXFragment(frag) => {
            Some(jsx_fragment_to_string(frag))
        }
        swc_ecma_ast::JSXElementChild::JSXSpreadChild(_) => None,
    }
}

fn jsx_element_to_string(elem: &swc_ecma_ast::JSXElement) -> String {
    let mut e = CodeEmitter::new();
    emit_jsx_element(&mut e, elem);
    e.into_output()
}

fn jsx_fragment_to_string(frag: &swc_ecma_ast::JSXFragment) -> String {
    let mut e = CodeEmitter::new();
    emit_jsx_fragment(&mut e, frag);
    e.into_output()
}

fn expr_to_string(expr: &swc_ecma_ast::Expr) -> String {
    let mut e = CodeEmitter::new();
    emit_expr(&mut e, expr);
    e.into_output()
}

/// Emit an await expression.
fn emit_await_expr(emitter: &mut CodeEmitter, await_expr: &swc_ecma_ast::AwaitExpr) {
    emit_expr(emitter, &await_expr.arg);
    emitter.push_str(".await");
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
