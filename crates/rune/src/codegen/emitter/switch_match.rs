//! # Switch Statement Emitter
//! Emits Rust match expressions from TypeScript switch statements.
use super::{emit_expr, CodeEmitter};
use super::types::{is_enum_type, to_rust_name};
use swc_ecma_ast::{Stmt, SwitchCase};

/// Emit a switch statement as Rust match.
pub fn emit_switch(emitter: &mut CodeEmitter, switch_stmt: &swc_ecma_ast::SwitchStmt) {
    emitter.push_str("match ");
    emit_expr(emitter, &switch_stmt.discriminant);
    emitter.push_str(" {\n");
    emitter.inc_indent();

    for case in &switch_stmt.cases {
        emit_switch_case(emitter, case);
    }

    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a single switch case.
pub fn emit_switch_case(emitter: &mut CodeEmitter, case: &SwitchCase) {
    emitter.push_indent();
    if let Some(test) = &case.test {
        emit_case_pattern_for_test(emitter, test);
    } else {
        emitter.push_str("_");
    }
    emitter.push_str(" {\n");

    emitter.inc_indent();
    for stmt in &case.cons {
        if !matches!(stmt, Stmt::Break(_)) {
            super::emit_single_stmt(emitter, stmt);
        }
    }
    emitter.dec_indent();

    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit case pattern for a test expression.
fn emit_case_pattern_for_test(emitter: &mut CodeEmitter, test: &swc_ecma_ast::Expr) {
    match test {
        swc_ecma_ast::Expr::Member(member) => {
            emit_tagged_variant_pattern(emitter, member);
        }
        swc_ecma_ast::Expr::Ident(ident) => {
            // For enum types, preserve PascalCase; for values, convert to snake_case
            let name = ident.sym.as_ref();
            if is_enum_type(name) {
                emitter.push_str(name);
            } else {
                emitter.push_str(&to_rust_name(name));
            }
        }
        swc_ecma_ast::Expr::Lit(lit) => {
            if let swc_ecma_ast::Lit::Str(s) = lit {
                emitter.push_str(&format!("{:?}", s.value));
            } else {
                emit_expr(emitter, test);
            }
        }
        _ => emit_expr(emitter, test),
    }
    emitter.push_str(" => ");
}

/// Emit a tagged variant pattern from member access.
fn emit_tagged_variant_pattern(emitter: &mut CodeEmitter, member: &swc_ecma_ast::MemberExpr) {
    if let swc_ecma_ast::MemberProp::Ident(prop) = &member.prop {
        let prop_name = prop.sym.as_ref();

        if let swc_ecma_ast::Expr::Ident(type_ident) = &*member.obj {
            let type_name = type_ident.sym.as_ref();
            if is_enum_type(type_name) {
                emitter.push_str(type_name);
            } else {
                emitter.push_str(&to_rust_name(type_name));
            }
            emitter.push_str("::");
            emitter.push_str(prop_name);
            return;
        }

        emit_expr(emitter, &member.obj);
        if prop_name == "tag" {
            emitter.push_str("::");
        } else {
            emitter.push_str(".");
            emitter.push_str(prop_name);
        }
    } else {
        emit_expr(emitter, &swc_ecma_ast::Expr::Member(member.clone()));
    }
}
