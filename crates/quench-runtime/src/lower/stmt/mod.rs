//! Statement lowering module - convert SWC statements to runtime AST statements

mod declarations;
mod destructuring;
mod exports;

pub use declarations::*;
pub use destructuring::*;
pub use exports::*;

// Re-export for use by other modules
use crate::ast::Statement;
use crate::lower::control_flow::{
    lower_do_while_stmt, lower_for_in_stmt, lower_for_of_stmt, lower_for_stmt, lower_if_stmt,
    lower_switch, lower_try_stmt, lower_while_stmt,
};
use crate::lower::expr::lower_expr;
use crate::lower::helpers::LowerError;
use swc_ecma_ast as swc;

/// Lower a swc Module to our runtime Program
pub fn lower_module(module: &swc::Module) -> Result<crate::ast::Program, LowerError> {
    let mut statements: Vec<Statement> = Vec::new();
    let mut export_stmts: Vec<Statement> = Vec::new();

    for item in &module.body {
        match lower_module_item(item)? {
            Some(Statement::Export(stmt)) => export_stmts.push(*stmt),
            Some(stmt) => statements.push(stmt),
            None => {}
        }
    }

    // If we have export statements, add them at the end
    statements.extend(export_stmts);

    Ok(crate::ast::Program::Script(statements))
}

/// Lower a swc Script to our runtime Program
pub fn lower_script(script: &swc::Script) -> Result<crate::ast::Program, LowerError> {
    let mut statements: Vec<Statement> = Vec::new();
    for stmt in &script.body {
        if let Some(s) = lower_stmt_checked(stmt)? {
            statements.push(s);
        }
    }
    Ok(crate::ast::Program::Script(statements))
}

/// Lower a statement, propagating an error for truly unsupported statements
fn lower_stmt_checked(stmt: &swc::Stmt) -> Result<Option<Statement>, LowerError> {
    match stmt {
        swc::Stmt::With(_) => Err(LowerError::new("`with` statements are not supported")),
        _ => Ok(lower_stmt(stmt)),
    }
}

/// Lower a swc ModuleItem to a Statement
fn lower_module_item(item: &swc::ModuleItem) -> Result<Option<Statement>, LowerError> {
    match item {
        swc::ModuleItem::Stmt(stmt) => lower_stmt_checked(stmt),
        swc::ModuleItem::ModuleDecl(decl) => Ok(lower_module_decl(decl)),
    }
}

/// Lower a swc Stmt to our Statement
#[allow(unreachable_patterns, clippy::complexity)]
pub fn lower_stmt(stmt: &swc::Stmt) -> Option<Statement> {
    match stmt {
        swc::Stmt::Empty(_) => Some(Statement::Empty),
        swc::Stmt::Block(block) => {
            let stmts: Vec<Statement> = block.stmts.iter().filter_map(lower_stmt).collect();
            Some(Statement::Block(stmts))
        }
        swc::Stmt::Break(_) => Some(Statement::Break(None)),
        swc::Stmt::Continue(_) => Some(Statement::Continue(None)),
        swc::Stmt::Debugger(_) => Some(Statement::Empty),
        swc::Stmt::With(_) => {
            // Nested `with` (where LowerError propagation is not possible):
            // lower to a statement that throws at eval time instead of vanishing
            Some(Statement::Throw(Box::new(crate::ast::Expression::String(
                "SyntaxError: `with` statements are not supported".to_string(),
            ))))
        }
        swc::Stmt::Decl(decl) => lower_decl(decl),
        swc::Stmt::Return(ret) => {
            let expr = ret.arg.as_ref().and_then(|e| lower_expr(e).ok());
            Some(Statement::Return(expr.map(Box::new)))
        }
        swc::Stmt::Labeled(labeled) => lower_stmt(&labeled.body),
        swc::Stmt::If(if_stmt) => lower_if_stmt(if_stmt),
        swc::Stmt::Switch(switch) => lower_switch(switch),
        swc::Stmt::Throw(throw) => {
            let expr = lower_expr(&throw.arg).ok()?;
            Some(Statement::Throw(Box::new(expr)))
        }
        swc::Stmt::Try(try_stmt) => lower_try_stmt(try_stmt),
        swc::Stmt::While(while_stmt) => lower_while_stmt(while_stmt),
        swc::Stmt::DoWhile(do_while) => lower_do_while_stmt(do_while),
        swc::Stmt::For(for_stmt) => lower_for_stmt(for_stmt),
        swc::Stmt::ForIn(for_in_stmt) => lower_for_in_stmt(for_in_stmt),
        swc::Stmt::ForOf(for_of_stmt) => lower_for_of_stmt(for_of_stmt),
        swc::Stmt::Expr(expr_stmt) => {
            let expr = lower_expr(&expr_stmt.expr).ok()?;
            Some(Statement::Expression(Box::new(expr)))
        }
        _ => None,
    }
}
