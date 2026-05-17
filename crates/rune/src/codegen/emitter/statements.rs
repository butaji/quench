//! # Statement Emitter
//!
//! Emits Rust statements from TypeScript AST.

use super::switch_match::emit_switch;
use super::variables::emit_var_decl;
use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::{Decl, Stmt};

/// Emit a function body statement.
#[allow(clippy::too_many_lines)]
pub fn emit_body_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Block(block) => emit_block_stmts(emitter, block),
        Stmt::Expr(expr_stmt) => emit_expr_stmt(emitter, expr_stmt),
        Stmt::Return(ret) => emit_return_stmt(emitter, ret),
        Stmt::If(if_stmt) => emit_if_stmt(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while_stmt(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for_stmt(emitter, for_stmt),
        Stmt::ForOf(for_of_stmt) => emit_for_of_stmt(emitter, for_of_stmt),
        Stmt::Switch(switch_stmt) => emit_switch(emitter, switch_stmt),
        Stmt::Break(_) => emit_break(emitter),
        Stmt::Continue(_) => emit_continue(emitter),
        _ => emit_unsupported(emitter),
    }
}

fn emit_block_stmts(emitter: &mut CodeEmitter, block: &swc_ecma_ast::BlockStmt) {
    for s in &block.stmts {
        emit_single_stmt(emitter, s);
    }
}

fn emit_expr_stmt(emitter: &mut CodeEmitter, expr_stmt: &swc_ecma_ast::ExprStmt) {
    emitter.push_indent();
    emit_expr(emitter, &expr_stmt.expr);
    emitter.push_str(";\n");
}

fn emit_return_stmt(emitter: &mut CodeEmitter, ret: &swc_ecma_ast::ReturnStmt) {
    emitter.push_indent();
    if let Some(arg) = &ret.arg {
        emit_return_with_value(emitter, arg);
    } else {
        emitter.push_str("return ();\n");
    }
}

fn emit_return_with_value(emitter: &mut CodeEmitter, arg: &swc_ecma_ast::Expr) {
    if let Some(expected) = emitter.expected_return() {
        if is_custom_struct_type(expected) {
            let prev_struct = emitter.object_struct_name().cloned();
            emitter.set_object_struct(Some(expected.clone()));
            emitter.push_str("return ");
            emit_expr(emitter, arg);
            emitter.push_str(";\n");
            restore_struct_context(emitter, prev_struct);
            return;
        }
    }
    emitter.push_str("return ");
    emit_expr(emitter, arg);
    emitter.push_str(";\n");
}

/// Check if a type is a custom struct (not a built-in type).
fn is_custom_struct_type(ty: &str) -> bool {
    (ty.starts_with(|c: char| c.is_uppercase()) || ty.starts_with("__"))
        && !ty.starts_with("Vec")
        && !ty.starts_with("Option")
        && !ty.starts_with("Result")
        && ty != "String"
        && ty != "bool"
        && ty != "f64"
        && ty != "i32"
        && ty != "()"
}

/// Emit a single statement.
pub fn emit_single_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    emitter.push_indent();
    match stmt {
        Stmt::Expr(expr_stmt) => emit_single_expr_stmt(emitter, expr_stmt),
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
        Stmt::If(if_stmt) => emit_if_stmt(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while_stmt(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for_stmt(emitter, for_stmt),
        Stmt::ForOf(for_of_stmt) => emit_for_of_stmt(emitter, for_of_stmt),
        Stmt::Switch(switch_stmt) => emit_switch(emitter, switch_stmt),
        Stmt::Block(block) => emit_block_with_indent(emitter, block),
        Stmt::Return(ret) => emit_return_stmt(emitter, ret),
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str("// unsupported\n"),
    }
}

fn emit_single_expr_stmt(emitter: &mut CodeEmitter, expr_stmt: &swc_ecma_ast::ExprStmt) {
    emit_expr(emitter, &expr_stmt.expr);
    emitter.push_str(";\n");
}

fn emit_block_with_indent(emitter: &mut CodeEmitter, block: &swc_ecma_ast::BlockStmt) {
    emitter.push_str("{\n");
    emitter.inc_indent();
    for s in &block.stmts {
        emit_single_stmt(emitter, s);
    }
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

fn restore_struct_context(emitter: &mut CodeEmitter, prev_struct: Option<String>) {
    if let Some(prev) = prev_struct {
        emitter.set_object_struct(Some(prev));
    } else {
        emitter.set_object_struct(None);
    }
}

fn emit_if_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::IfStmt) {
    emitter.push_str("if ");
    emit_expr(emitter, &stmt.test);
    emit_if_body(emitter, &stmt.cons);
    if let Some(alt) = &stmt.alt {
        emit_if_else(emitter, alt);
    }
    emitter.push_indent();
    emitter.push_str("}\n");
}

fn emit_if_body(emitter: &mut CodeEmitter, cons: &Stmt) {
    emitter.push_str(" {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = cons {
        emit_block_stmts(emitter, block);
    } else {
        emitter.push_indent();
        emit_simple_stmt(emitter, cons);
    }
    emitter.dec_indent();
}

fn emit_if_else(emitter: &mut CodeEmitter, alt_stmt: &Stmt) {
    emitter.push_indent();
    emitter.push_str("} else ");
    if let Stmt::If(else_if) = alt_stmt {
        emit_if_stmt(emitter, else_if);
    } else {
        emitter.push_str("{\n");
        emitter.inc_indent();
        if let Stmt::Block(block) = alt_stmt {
            emit_block_stmts(emitter, block);
        } else {
            emitter.push_indent();
            emit_simple_stmt(emitter, alt_stmt);
        }
        emitter.dec_indent();
    }
}

fn emit_simple_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Expr(expr_stmt) => {
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
        Stmt::Return(ret) => emit_return_simple(emitter, ret),
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str(";\n"),
    }
}

fn emit_return_simple(emitter: &mut CodeEmitter, ret: &swc_ecma_ast::ReturnStmt) {
    if let Some(arg) = &ret.arg {
        emitter.push_str("return ");
        emit_expr(emitter, arg);
        emitter.push_str(";\n");
    } else {
        emitter.push_str("return ();\n");
    }
}

fn emit_while_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::WhileStmt) {
    emitter.push_str("while ");
    emit_expr(emitter, &stmt.test);
    emitter.push_str(" {\n");
    emitter.inc_indent();
    emit_single_stmt(emitter, &stmt.body);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

fn emit_for_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::ForStmt) {
    emitter.push_str("for ");
    emit_for_init(emitter, stmt.init.as_ref());
    emitter.push_str("; ");
    if let Some(test) = &stmt.test {
        emit_expr(emitter, test);
    }
    emitter.push_str("; ");
    if let Some(update) = &stmt.update {
        emit_expr(emitter, update);
    }
    emitter.push_str(" {\n");
    emitter.inc_indent();
    emit_single_stmt(emitter, &stmt.body);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

fn emit_for_init(emitter: &mut CodeEmitter, init: Option<&swc_ecma_ast::VarDeclOrExpr>) {
    if let Some(init) = init {
        match init {
            swc_ecma_ast::VarDeclOrExpr::Expr(e) => emit_expr(emitter, e),
            swc_ecma_ast::VarDeclOrExpr::VarDecl(d) => emit_var_decl(emitter, &Decl::Var(d.clone())),
        }
    }
}

fn emit_for_of_stmt(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::ForOfStmt) {
    match &stmt.left {
        swc_ecma_ast::ForHead::VarDecl(var_decl) => {
            let var_name = extract_for_of_var_name(var_decl);
            if let Some(name) = &var_name {
                let is_const = var_decl.kind == swc_ecma_ast::VarDeclKind::Const;
                emit_for_of_loop(emitter, name, is_const, &stmt.right, &stmt.body);
            } else {
                emitter.push_str("// unsupported for-of pattern\n");
            }
        }
        swc_ecma_ast::ForHead::Pat(_pat) => emit_for_of_pattern(emitter, &stmt.right),
        swc_ecma_ast::ForHead::UsingDecl(_) => emitter.push_str("// using not supported\n"),
    }
}

fn extract_for_of_var_name(var_decl: &swc_ecma_ast::VarDecl) -> Option<String> {
    var_decl.decls.iter().find_map(|decl| extract_var_name(&decl.name))
}

fn emit_for_of_loop(
    emitter: &mut CodeEmitter,
    var_name: &str,
    _is_const: bool,
    right: &swc_ecma_ast::Expr,
    body: &swc_ecma_ast::Stmt,
) {
    // Save context for nested expressions
    let prev_struct = emitter.object_struct_name().cloned();
    emitter.set_object_struct(None);

    emitter.push_str(&format!("for {var_name} in "));
    emit_expr(emitter, right);
    emitter.push_str(".iter().cloned() {\n");
    emitter.inc_indent();
    emit_single_stmt(emitter, body);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");

    // Restore context
    restore_struct_context(emitter, prev_struct);
}

fn emit_for_of_pattern(emitter: &mut CodeEmitter, right: &swc_ecma_ast::Expr) {
    emitter.push_str("// pattern: ");
    emit_expr(emitter, right);
    emitter.push_str(";\n");
}

fn extract_var_name(name: &swc_ecma_ast::Pat) -> Option<String> {
    if let swc_ecma_ast::Pat::Ident(ident) = name {
        Some(super::to_snake_case(ident.id.sym.as_ref()))
    } else {
        None
    }
}

fn emit_break(emitter: &mut CodeEmitter) {
    emitter.push_str("break;\n");
}

fn emit_continue(emitter: &mut CodeEmitter) {
    emitter.push_str("continue;\n");
}

fn emit_unsupported(emitter: &mut CodeEmitter) {
    emitter.push_str("// unsupported statement\n");
}
