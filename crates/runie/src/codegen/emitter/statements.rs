//! # Statement Emitter
//!
//! Emits Rust statements from TypeScript AST.
//! Delegates to specialized modules for control flow.

use super::switch_match::emit_switch;
use super::stmt_control::{
    emit_for_of_stmt, emit_for_stmt, emit_if_stmt, emit_return_stmt, emit_single_stmt,
    emit_while_stmt,
};
use super::variables::emit_var_decl;
use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::Stmt;

/// Emit a function body statement.
pub fn emit_body_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Block(block) => emit_block_stmts(emitter, block),
        Stmt::Expr(expr_stmt) => emit_expr_stmt(emitter, expr_stmt),
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
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

fn emit_break(emitter: &mut CodeEmitter) {
    emitter.push_str("break;\n");
}

fn emit_continue(emitter: &mut CodeEmitter) {
    emitter.push_str("continue;\n");
}

fn emit_unsupported(emitter: &mut CodeEmitter) {
    emitter.push_str("// unsupported statement\n");
}
