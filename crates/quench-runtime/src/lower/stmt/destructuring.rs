//! Destructuring pattern lowering functions

use crate::ast::{Expression, Statement, VarKind};
use oxc::ast::ast;

use crate::lower::pattern::{lower_array_binding, lower_object_binding};

/// Lower array destructuring pattern via runtime iterator protocol.
pub fn lower_array_destructuring(
    kind: VarKind,
    arr: &ast::ArrayPattern,
    init_expr: Option<Expression>,
    idx: usize,
) -> Vec<Statement> {
    let _ = idx;
    let pattern = lower_array_binding(arr).expect("valid array destructuring pattern");
    vec![Statement::PatternDeclaration {
        kind,
        pattern,
        init: init_expr,
    }]
}

/// Lower object destructuring pattern via runtime binding initialization.
pub fn lower_object_destructuring(
    kind: VarKind,
    obj: &ast::ObjectPattern,
    init_expr: Option<Expression>,
    idx: usize,
) -> Vec<Statement> {
    let _ = idx;
    let pattern = lower_object_binding(obj).expect("valid object destructuring pattern");
    vec![Statement::PatternDeclaration {
        kind,
        pattern,
        init: init_expr,
    }]
}

/// Wrap declarations in appropriate statement(s).
/// Always use SequenceDecls to avoid creating spurious block scopes.
/// Block scope is only created by explicit `{ ... }` in the source.
pub fn wrap_decls(decls: Vec<Statement>) -> Option<Statement> {
    if decls.is_empty() {
        return Some(Statement::Empty);
    }
    if decls.len() == 1 {
        return Some(decls.into_iter().next().unwrap());
    }
    // SequenceDecls evaluates statements without introducing a new lexical scope,
    // which is correct for declarations at any level (var, let, const).
    Some(Statement::SequenceDecls(decls))
}
