//! Hook detection for JS generation

use super::super::hir::*;

pub fn stmts_have_call(stmts: &[Stmt], name: &str) -> bool { stmts.iter().any(|s| stmt_has_call(s, name)) }
pub fn stmt_has_call(stmt: &Stmt, name: &str) -> bool { match stmt { Stmt::Expr { expr } => expr_has_call(expr, name), Stmt::Return { arg } => arg.as_ref().map_or(false, |e| expr_has_call(e, name)), Stmt::If { consequent, alternate, .. } => stmts_have_call(consequent, name) || alternate.as_ref().map_or(false, |a| stmts_have_call(a, name)), Stmt::Block { stmts } => stmts_have_call(stmts, name), Stmt::While { body, .. } => stmts_have_call(body, name), _ => false } }
pub fn expr_has_call(expr: &Expr, name: &str) -> bool { if let Expr::Call { callee, .. } = expr { if let Expr::Ident { name: n } = callee.as_ref() { return n.name == name; } } false }
pub fn has_hook_calls(stmts: &[Stmt]) -> bool { stmts_have_call(stmts, "useState") }
