//! Control flow statement lowering
//!
//! Handles lowering of if, while, for, try-catch, switch statements.

use swc_ecma_ast as swc;
use crate::ast::{BinaryOp, Expression, ForInit, Statement, VarKind};
use super::expr::lower_expr;
use super::helpers::atom_to_string;
use super::stmt::lower_stmt;

/// Lower an if statement
pub fn lower_if_stmt(if_stmt: &swc::IfStmt) -> Option<Statement> {
    let condition = lower_expr(&if_stmt.test).ok()?;
    let consequent = Box::new(lower_stmt(&if_stmt.cons).unwrap_or(Statement::Empty));
    let alternate = if_stmt.alt.as_ref().map(|a| {
        Box::new(lower_stmt(a).unwrap_or(Statement::Empty))
    });
    Some(Statement::If {
        condition: Box::new(condition),
        consequent,
        alternate,
    })
}

/// Lower a while statement
pub fn lower_while_stmt(while_stmt: &swc::WhileStmt) -> Option<Statement> {
    let condition = lower_expr(&while_stmt.test).ok()?;
    let body = Box::new(lower_stmt(&while_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::While { condition: Box::new(condition), body })
}

/// Lower a for statement
pub fn lower_for_stmt(for_stmt: &swc::ForStmt) -> Option<Statement> {
    let init = for_stmt.init.as_ref().and_then(lower_for_init);
    let condition = for_stmt.test.as_ref().and_then(|e| lower_expr(e).ok()).map(Box::new);
    let update = for_stmt.update.as_ref().and_then(|e| lower_expr(e).ok()).map(Box::new);
    let body = Box::new(lower_stmt(&for_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::For { init, condition, update, body })
}

/// Lower a for-in statement
pub fn lower_for_in_stmt(for_in_stmt: &swc::ForInStmt) -> Option<Statement> {
    let left = lower_for_lhs(&for_in_stmt.left)?;
    let iterable = lower_expr(&for_in_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_in_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::Expression(Box::new(Expression::ForIn {
        variable: Box::new(left),
        object: Box::new(iterable),
        body,
    })))
}

/// Lower a for-of statement
pub fn lower_for_of_stmt(for_of_stmt: &swc::ForOfStmt) -> Option<Statement> {
    let left = lower_for_lhs(&for_of_stmt.left)?;
    let iterable = lower_expr(&for_of_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_of_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::Expression(Box::new(Expression::ForOf {
        variable: Box::new(left),
        iterable: Box::new(iterable),
        body,
    })))
}

/// Lower a try-catch statement
pub fn lower_try_stmt(try_stmt: &swc::TryStmt) -> Option<Statement> {
    let body = Box::new(
        lower_stmt(&swc::Stmt::Block(try_stmt.block.clone()))
            .unwrap_or(Statement::Empty)
    );
    let catch_param = try_stmt.handler.as_ref().and_then(|catch| {
        catch.param.as_ref().and_then(|pat| {
            match pat {
                swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                _ => None,
            }
        })
    });
    let handler = if let Some(catch) = &try_stmt.handler {
        Box::new(
            lower_stmt(&swc::Stmt::Block(catch.body.clone()))
                .unwrap_or(Statement::Empty)
        )
    } else {
        Box::new(Statement::Empty)
    };
    Some(Statement::TryCatch { body, param: catch_param, handler })
}

/// Lower a switch statement into nested if-else chains
pub fn lower_switch(switch: &swc::SwitchStmt) -> Option<Statement> {
    let discriminant = lower_expr(&switch.discriminant).ok()?;
    let mut current: Option<Statement> = None;
    for case in switch.cases.iter().rev() {
        let case_body = case.cons.iter()
            .filter_map(lower_stmt)
            .collect::<Vec<_>>();
        let new_stmt = if let Some(test) = &case.test {
            let test_expr = lower_expr(test).ok()?;
            Statement::If {
                condition: Box::new(Expression::Binary {
                    op: BinaryOp::StrictEq,
                    left: Box::new(discriminant.clone()),
                    right: Box::new(test_expr),
                }),
                consequent: Box::new(Statement::Block(case_body)),
                alternate: current.map(Box::new),
            }
        } else {
            Statement::Block(case_body)
        };
        current = Some(new_stmt);
    }
    current.or(Some(Statement::Empty))
}

/// Lower a for loop init (variable declaration or expression)
pub fn lower_for_init(init: &swc::VarDeclOrExpr) -> Option<ForInit> {
    match init {
        swc::VarDeclOrExpr::VarDecl(decl) => {
            let first = decl.decls.first()?;
            let kind = match decl.kind {
                swc::VarDeclKind::Var => VarKind::Var,
                swc::VarDeclKind::Let => VarKind::Let,
                swc::VarDeclKind::Const => VarKind::Const,
            };
            let name = match &first.name {
                swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                _ => return None,
            };
            let init = first.init.as_ref().and_then(|e| lower_expr(e).ok());
            Some(ForInit::VarDeclaration { kind, name, init })
        }
        swc::VarDeclOrExpr::Expr(expr) => {
            Some(ForInit::Expression(Box::new(lower_expr(expr).ok()?)))
        }
    }
}

/// Lower the left-hand side of a for-in/for-of loop
pub fn lower_for_lhs(left: &swc::ForHead) -> Option<Expression> {
    match left {
        swc::ForHead::VarDecl(decl) => {
            let first = decl.decls.first()?;
            match &first.name {
                swc::Pat::Ident(ident) => {
                    Some(Expression::Identifier(atom_to_string(&ident.id.sym)))
                }
                _ => None,
            }
        }
        swc::ForHead::Pat(pat) => {
            match pat.as_ref() {
                swc::Pat::Ident(ident) => {
                    Some(Expression::Identifier(atom_to_string(&ident.id.sym)))
                }
                _ => None,
            }
        }
        swc::ForHead::UsingDecl(_) => None,
    }
}
