//! Control flow statement lowering
//!
//! Handles lowering of if, while, for, try-catch, switch statements.

use super::expr::lower_expr;
use super::helpers::atom_to_string;
use super::pattern::{lower_elem_pat, lower_object_pat_prop};
use super::stmt::lower_stmt;
use crate::ast::{
    BinaryOp, BindingElement, Expression, ForInit, PropertyKey, Statement, UnaryOp, VarKind,
};
use swc_ecma_ast as swc;

/// Lower an if statement
pub fn lower_if_stmt(if_stmt: &swc::IfStmt) -> Option<Statement> {
    let condition = lower_expr(&if_stmt.test).ok()?;
    let consequent = Box::new(lower_stmt(&if_stmt.cons).unwrap_or(Statement::Empty));
    let alternate = if_stmt
        .alt
        .as_ref()
        .map(|a| Box::new(lower_stmt(a).unwrap_or(Statement::Empty)));
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
    Some(Statement::While {
        condition: Box::new(condition),
        body,
    })
}

/// Lower a do-while statement: do { body } while (cond)
/// Desugars to: while (true) { body; if (!cond) break; }
pub fn lower_do_while_stmt(do_while: &swc::DoWhileStmt) -> Option<Statement> {
    let condition = lower_expr(&do_while.test).ok()?;
    let body = lower_stmt(&do_while.body).unwrap_or(Statement::Empty);
    let break_check = Statement::If {
        condition: Box::new(Expression::Unary {
            op: UnaryOp::Not,
            argument: Box::new(condition),
        }),
        consequent: Box::new(Statement::Break(None)),
        alternate: None,
    };
    Some(Statement::While {
        condition: Box::new(Expression::Boolean(true)),
        body: Box::new(Statement::Block(vec![body, break_check])),
    })
}

/// Lower a for statement
pub fn lower_for_stmt(for_stmt: &swc::ForStmt) -> Option<Statement> {
    let init = for_stmt.init.as_ref().and_then(lower_for_init);
    let condition = for_stmt
        .test
        .as_ref()
        .and_then(|e| lower_expr(e).ok())
        .map(Box::new);
    let update = for_stmt
        .update
        .as_ref()
        .and_then(|e| lower_expr(e).ok())
        .map(Box::new);
    let body = Box::new(lower_stmt(&for_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::For {
        init,
        condition,
        update,
        body,
    })
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
    let body =
        Box::new(lower_stmt(&swc::Stmt::Block(try_stmt.block.clone())).unwrap_or(Statement::Empty));
    let catch_param = try_stmt.handler.as_ref().and_then(|catch| {
        catch.param.as_ref().and_then(|pat| match pat {
            swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
            _ => None,
        })
    });
    let handler = if let Some(catch) = &try_stmt.handler {
        Box::new(lower_stmt(&swc::Stmt::Block(catch.body.clone())).unwrap_or(Statement::Empty))
    } else {
        Box::new(Statement::Empty)
    };
    Some(Statement::TryCatch {
        body,
        param: catch_param,
        handler,
    })
}

/// Lower a switch statement into nested if-else chains
pub fn lower_switch(switch: &swc::SwitchStmt) -> Option<Statement> {
    let discriminant = lower_expr(&switch.discriminant).ok()?;

    // Compute each case's effective body: a case with no statements falls
    // through to the next case's body (JS fall-through semantics).
    let mut effective_bodies: Vec<Vec<Statement>> = Vec::with_capacity(switch.cases.len());
    let mut next_body: Vec<Statement> = Vec::new();
    for case in switch.cases.iter().rev() {
        let own: Vec<Statement> = case.cons.iter().filter_map(lower_stmt).collect();
        let effective = if own.is_empty() {
            next_body.clone()
        } else {
            next_body = own.clone();
            own
        };
        effective_bodies.push(effective);
    }
    effective_bodies.reverse();

    let mut current: Option<Statement> = None;
    for (case, case_body) in switch.cases.iter().zip(effective_bodies).rev() {
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
    let chain = current.unwrap_or(Statement::Empty);
    // The if-else chain cannot contain a `break` meant for the switch: it
    // would escape to the enclosing function or loop. Wrap the chain in a
    // one-shot counter loop, which consumes Break (and Continue) and always
    // terminates. `return` inside a case body still propagates.
    Some(Statement::For {
        init: Some(ForInit::VarDeclaration {
            kind: VarKind::Var,
            name: "__quench_switch__".to_string(),
            init: Some(Expression::Number(0.0)),
        }),
        condition: Some(Box::new(Expression::Binary {
            op: BinaryOp::Lt,
            left: Box::new(Expression::Identifier("__quench_switch__".to_string())),
            right: Box::new(Expression::Number(1.0)),
        })),
        update: Some(Box::new(Expression::Update {
            op: crate::ast::UpdateOp::Increment,
            argument: Box::new(Expression::Identifier("__quench_switch__".to_string())),
            prefix: false,
        })),
        body: Box::new(chain),
    })
}

/// Lower a for loop init (variable declaration or expression)
#[allow(clippy::complexity)]
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
#[allow(clippy::complexity)]
pub fn lower_for_lhs(left: &swc::ForHead) -> Option<Expression> {
    match left {
        swc::ForHead::VarDecl(decl) => {
            let first = decl.decls.first()?;
            match &first.name {
                swc::Pat::Ident(ident) => {
                    Some(Expression::Identifier(atom_to_string(&ident.id.sym)))
                }
                swc::Pat::Array(arr) => lower_array_lhs(arr),
                swc::Pat::Object(obj) => lower_object_lhs(obj),
                _ => None,
            }
        }
        swc::ForHead::Pat(pat) => match pat.as_ref() {
            swc::Pat::Ident(ident) => Some(Expression::Identifier(atom_to_string(&ident.id.sym))),
            swc::Pat::Array(arr) => lower_array_lhs(arr),
            swc::Pat::Object(obj) => lower_object_lhs(obj),
            _ => None,
        },
        swc::ForHead::UsingDecl(_) => None,
    }
}

fn lower_array_lhs(arr: &swc::ArrayPat) -> Option<Expression> {
    let elements: Vec<BindingElement> = arr
        .elems
        .iter()
        .filter_map(|e| match e {
            Some(elem) => lower_elem_pat(elem),
            None => Some(BindingElement::Identifier("__hole".to_string())),
        })
        .collect();
    Some(Expression::ArrayPattern(elements))
}

fn lower_object_lhs(obj: &swc::ObjectPat) -> Option<Expression> {
    let props: Vec<(PropertyKey, BindingElement)> =
        obj.props.iter().filter_map(lower_object_pat_prop).collect();
    Some(Expression::ObjectPattern(props))
}
