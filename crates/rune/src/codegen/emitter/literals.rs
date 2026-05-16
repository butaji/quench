//! # Literal Emitter
//!
//! Emits Rust literals from TypeScript.

use super::CodeEmitter;
use swc_ecma_ast::Lit;

/// Emit a literal.
pub fn emit_lit(emitter: &mut CodeEmitter, lit: &Lit) {
    match lit {
        Lit::Str(s) => emitter.push_str(&format!("{:?}", s.value)),
        Lit::Num(n) => {
            if n.value.fract() == 0.0 && n.value.abs() < f64::from(i32::MAX) {
                emitter.push_str(&format!("{}i32", n.value as i32));
            } else {
                emitter.push_str(&n.value.to_string());
            }
        }
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
    let mut args: Vec<String> = Vec::new();

    for (i, quasi) in tpl.quasis.iter().enumerate() {
        fmt.push_str(&quasi.raw.replace('{', "{{").replace('}', "}}"));
        if i < tpl.exprs.len() {
            fmt.push_str("{}");
            let arg_str = expr_to_string(&tpl.exprs[i]);
            args.push(arg_str);
        }
    }

    emitter.push_str(&format!("format!(\"{}\", {})", fmt, args.join(", ")));
}

fn expr_to_string(expr: &swc_ecma_ast::Expr) -> String {
    use swc_ecma_ast::Expr;
    match expr {
        Expr::Ident(ident) => super::to_snake_case(ident.sym.as_ref()),
        Expr::Lit(Lit::Str(s)) => format!("{:?}", s.value),
        Expr::Lit(Lit::Num(n)) => n.value.to_string(),
        _ => String::from("()"),
    }
}
