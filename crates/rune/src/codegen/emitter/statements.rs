//! # Statement Emitter
//! Emits Rust statements from TypeScript AST.

use super::switch_match::emit_switch;
use super::variables::emit_var_decl;
use super::{emit_expr, CodeEmitter};
use swc_ecma_ast::{Decl, Stmt};

/// Emit a function body statement.
pub fn emit_body_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Block(block) => {
            for s in &block.stmts {
                emit_single_stmt(emitter, s);
            }
        }
        Stmt::Expr(expr_stmt) => {
            emitter.push_indent();
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Return(ret) => emit_return(emitter, ret),
        Stmt::If(if_stmt) => emit_if(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for(emitter, for_stmt),
        Stmt::Switch(switch_stmt) => emit_switch(emitter, switch_stmt),
        Stmt::Break(_) => {
            emitter.push_indent();
            emitter.push_str("break;\n");
        }
        Stmt::Continue(_) => {
            emitter.push_indent();
            emitter.push_str("continue;\n");
        }
        _ => {
            emitter.push_indent();
            emitter.push_str("// unsupported statement\n");
        }
    }
}

/// Emit a return statement with proper struct context handling.
fn emit_return(emitter: &mut CodeEmitter, ret: &swc_ecma_ast::ReturnStmt) {
    emitter.push_indent();
    if let Some(arg) = &ret.arg {
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
    } else {
        emitter.push_str("return ();\n");
    }
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
        Stmt::Expr(expr_stmt) => {
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
        Stmt::If(if_stmt) => emit_if(emitter, if_stmt),
        Stmt::While(while_stmt) => emit_while(emitter, while_stmt),
        Stmt::For(for_stmt) => emit_for(emitter, for_stmt),
        Stmt::Switch(switch_stmt) => emit_switch(emitter, switch_stmt),
        Stmt::Block(block) => emit_block(emitter, block),
        Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
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
            } else {
                emitter.push_str("return ();\n");
            }
        }
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str("// unsupported\n"),
    }
}

/// Emit a block statement.
fn emit_block(emitter: &mut CodeEmitter, block: &swc_ecma_ast::BlockStmt) {
    emitter.push_str("{\n");
    emitter.inc_indent();
    for s in &block.stmts {
        emit_single_stmt(emitter, s);
    }
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Restore struct context after a return.
fn restore_struct_context(emitter: &mut CodeEmitter, prev_struct: Option<String>) {
    if let Some(prev) = prev_struct {
        emitter.set_object_struct(Some(prev));
    } else {
        emitter.set_object_struct(None);
    }
}

/// Emit an if statement.
fn emit_if(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::IfStmt) {
    emitter.push_str("if ");
    emit_expr(emitter, &stmt.test);

    emitter.push_str(" {\n");
    emitter.inc_indent();
    if let Stmt::Block(block) = &*stmt.cons {
        for s in &block.stmts {
            emit_single_stmt(emitter, s);
        }
    } else {
        emitter.push_indent();
        emit_simple_stmt(emitter, &stmt.cons);
    }
    emitter.dec_indent();

    if let Some(alt) = &stmt.alt {
        emitter.push_indent();
        emitter.push_str("} else ");
        if matches!(&**alt, Stmt::If(_)) {
            if let Stmt::If(else_if) = &**alt {
                emit_if(emitter, else_if);
            }
        } else {
            emitter.push_str("{\n");
            emitter.inc_indent();
            if let Stmt::Block(block) = &**alt {
                for s in &block.stmts {
                    emit_single_stmt(emitter, s);
                }
            } else {
                emitter.push_indent();
                emit_simple_stmt(emitter, alt);
            }
            emitter.dec_indent();
        }
    }

    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a simple statement (no block, no extra newlines).
fn emit_simple_stmt(emitter: &mut CodeEmitter, stmt: &Stmt) {
    match stmt {
        Stmt::Expr(expr_stmt) => {
            emit_expr(emitter, &expr_stmt.expr);
            emitter.push_str(";\n");
        }
        Stmt::Decl(decl) => emit_var_decl(emitter, decl),
        Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
                emitter.push_str("return ");
                emit_expr(emitter, arg);
                emitter.push_str(";\n");
            } else {
                emitter.push_str("return ();\n");
            }
        }
        Stmt::Break(_) => emitter.push_str("break;\n"),
        Stmt::Continue(_) => emitter.push_str("continue;\n"),
        _ => emitter.push_str(";\n"),
    }
}

/// Emit a while statement.
fn emit_while(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::WhileStmt) {
    emitter.push_str("while ");
    emit_expr(emitter, &stmt.test);
    emitter.push_str(" {\n");
    emitter.inc_indent();
    emit_single_stmt(emitter, &stmt.body);
    emitter.dec_indent();
    emitter.push_indent();
    emitter.push_str("}\n");
}

/// Emit a for statement.
fn emit_for(emitter: &mut CodeEmitter, stmt: &swc_ecma_ast::ForStmt) {
    emitter.push_str("for ");
    if let Some(init) = &stmt.init {
        match init {
            swc_ecma_ast::VarDeclOrExpr::Expr(e) => emit_expr(emitter, e),
            swc_ecma_ast::VarDeclOrExpr::VarDecl(d) => {
                emit_var_decl(emitter, &Decl::Var(d.clone()));
            }
        }
    }
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
