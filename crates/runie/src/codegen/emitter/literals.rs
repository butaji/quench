//! # Literal Emitter
//!
//! Emits Rust literals from TypeScript.

use super::CodeEmitter;
use super::emit_expr;
use swc_ecma_ast::Lit;

/// Emit a literal.
pub fn emit_lit(emitter: &mut CodeEmitter, lit: &Lit) {
    match lit {
        Lit::Str(s) => emitter.push_str(&format!("{:?}", s.value)),
        Lit::Num(n) => emit_number_literal(emitter, n),
        Lit::Bool(b) => emitter.push_str(if b.value { "true" } else { "false" }),
        Lit::Null(_) => emitter.push_str("None"),
        Lit::BigInt(b) => {
            emitter.push_str(&b.value.to_string());
            emitter.push_str("i64");
        }
        Lit::JSXText(text) => emitter.push_str(&format!("{:?}", text.value)),
        Lit::Regex(_) => emitter.push_str("String::new()"),
    }
}

fn emit_number_literal(emitter: &mut CodeEmitter, n: &swc_ecma_ast::Number) {
    if n.value.fract() == 0.0 && n.value.abs() < f64::from(i32::MAX) {
        emitter.push_str(&format!("{}i32", n.value as i32));
    } else {
        emitter.push_str(&format!("{}_f64", n.value));
    }
}

/// Emit a template literal.
pub fn emit_template_literal(emitter: &mut CodeEmitter, tpl: &swc_ecma_ast::Tpl) {
    if tpl.quasis.is_empty() {
        emitter.push_str("String::new()");
        return;
    }

    if tpl.exprs.is_empty() {
        emitter.push_str(&format!("{:?}", tpl.quasis[0].raw));
        return;
    }

    let mut fmt = String::new();

    for (i, quasi) in tpl.quasis.iter().enumerate() {
        fmt.push_str(&quasi.raw.replace('{', "{{").replace('}', "}}"));
        if i < tpl.exprs.len() {
            fmt.push_str("{}");
        }
    }

    emitter.push_str(&format!("format!(\"{}\", ", fmt));
    emit_template_args(emitter, tpl);
    emitter.push_str(")");
}

fn emit_template_args(emitter: &mut CodeEmitter, tpl: &swc_ecma_ast::Tpl) {
    for (i, expr) in tpl.exprs.iter().enumerate() {
        if i > 0 {
            emitter.push_str(", ");
        }
        emit_expr(emitter, expr);
    }
}
