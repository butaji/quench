//! Control flow statement lowering
//!
//! Handles lowering of if, while, for, try-catch, switch statements.

use super::expr::lower_expr;
use super::pattern::{
    binding_to_expr, lower_array_assignment_target, lower_binding_elem,
    lower_object_assignment_target,
};
use super::stmt::lower_stmt;
use crate::ast::{
    BinaryOp, BindingElement, Expression, ForInit, ForInitDecl, PropertyKey, Statement, VarKind,
};
use oxc::ast::ast;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

static SWITCH_SCOPE_ID: AtomicUsize = AtomicUsize::new(0);
static CATCH_PARAM_ID: AtomicUsize = AtomicUsize::new(0);

fn next_switch_scope_id() -> usize {
    SWITCH_SCOPE_ID.fetch_add(1, Ordering::Relaxed)
}

fn next_catch_param_name() -> String {
    let id = CATCH_PARAM_ID.fetch_add(1, Ordering::Relaxed);
    format!("__quench_catch_param_{id}")
}

/// Lower an if statement
pub fn lower_if_stmt(if_stmt: &ast::IfStatement) -> Option<Statement> {
    let condition = lower_expr(&if_stmt.test).ok()?;
    let consequent = Box::new(lower_stmt(&if_stmt.consequent).unwrap_or(Statement::Empty));
    let alternate = if_stmt
        .alternate
        .as_ref()
        .map(|a| Box::new(lower_stmt(a).unwrap_or(Statement::Empty)));
    Some(Statement::If {
        condition: Box::new(condition),
        consequent,
        alternate,
    })
}

/// Lower a while statement
pub fn lower_while_stmt(while_stmt: &ast::WhileStatement) -> Option<Statement> {
    let condition = lower_expr(&while_stmt.test).ok()?;
    let body = Box::new(lower_stmt(&while_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::While {
        condition: Box::new(condition),
        body,
    })
}

/// Lower a do-while statement: do { body } while (cond)
/// Emits Statement::DoWhile so eval_do_while can capture the body completion
/// value and return it when the condition is false.
pub fn lower_do_while_stmt(do_while: &ast::DoWhileStatement) -> Option<Statement> {
    let condition = lower_expr(&do_while.test).ok()?;
    let body = lower_stmt(&do_while.body).unwrap_or(Statement::Empty);
    Some(Statement::DoWhile {
        body: Box::new(body),
        condition: Box::new(condition),
        labels: Vec::new(),
    })
}

/// Lower a for statement
pub fn lower_for_stmt(for_stmt: &ast::ForStatement) -> Option<Statement> {
    if let Some(ast::ForStatementInit::VariableDeclaration(decl)) = &for_stmt.init {
        if matches!(
            decl.kind,
            ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing
        ) {
            return lower_using_for_stmt(for_stmt, decl);
        }
    }
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

fn lower_using_for_stmt(
    for_stmt: &ast::ForStatement,
    decl: &ast::VariableDeclaration,
) -> Option<Statement> {
    let is_async = decl.kind == ast::VariableDeclarationKind::AwaitUsing;
    let mut body = Vec::new();
    let mut resources = Vec::new();
    for item in &decl.declarations {
        let ast::BindingPattern::BindingIdentifier(id) = &item.id else {
            return None;
        };
        let name = id.name.as_str().to_string();
        body.push(Statement::VarDeclaration {
            kind: VarKind::Const,
            name: name.clone(),
            init: item.init.as_ref().and_then(|expr| lower_expr(expr).ok()),
        });
        body.push(Statement::RegisterDispose {
            name: name.clone(),
            is_async,
        });
        resources.push(name);
    }
    body.push(lower_for_without_init(for_stmt)?);
    let finalizer = resources
        .into_iter()
        .rev()
        .map(|name| Statement::Dispose { name, is_async })
        .collect();
    Some(Statement::Block(vec![Statement::Try {
        body: Box::new(Statement::SequenceDecls(body)),
        param: None,
        handler: None,
        finalizer: Some(Box::new(Statement::SequenceDecls(finalizer))),
    }]))
}

fn lower_for_without_init(for_stmt: &ast::ForStatement) -> Option<Statement> {
    Some(Statement::For {
        init: None,
        condition: for_stmt
            .test
            .as_ref()
            .and_then(|expr| lower_expr(expr).ok())
            .map(Box::new),
        update: for_stmt
            .update
            .as_ref()
            .and_then(|expr| lower_expr(expr).ok())
            .map(Box::new),
        body: Box::new(lower_stmt(&for_stmt.body).unwrap_or(Statement::Empty)),
    })
}

/// Lower a for-in statement
pub fn lower_for_in_stmt(for_in_stmt: &ast::ForInStatement) -> Option<Statement> {
    let iterable = lower_expr(&for_in_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_in_stmt.body).unwrap_or(Statement::Empty));

    let (var_decl_stmt, loop_binding) =
        if let ast::ForStatementLeft::VariableDeclaration(ref decl) = &for_in_stmt.left {
            let kind = match decl.kind {
                ast::VariableDeclarationKind::Var => VarKind::Var,
                ast::VariableDeclarationKind::Let => VarKind::Let,
                ast::VariableDeclarationKind::Const => VarKind::Const,
                ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing => {
                    VarKind::Const
                }
            };
            let has_pattern = decl
                .declarations
                .iter()
                .any(|d| !matches!(d.id, ast::BindingPattern::BindingIdentifier(_)));
            if matches!(kind, VarKind::Var) {
                let vd = if has_pattern {
                    crate::lower::stmt::lower_for_in_var_pattern_hoist(decl)
                } else {
                    crate::lower::stmt::lower_var_decl(decl)
                };
                (vd, None)
            } else {
                (None, Some(kind))
            }
        } else {
            (None, None)
        };

    let variable = lower_for_lhs(&for_in_stmt.left)?;
    let for_in_expr = Statement::Expression(Box::new(Expression::ForIn {
        variable: Box::new(variable),
        object: Box::new(iterable),
        body,
        loop_binding,
    }));

    // If there's a var/let/const declaration, wrap in a block so it runs first
    if let Some(var_stmt) = var_decl_stmt {
        Some(Statement::Block(vec![var_stmt, for_in_expr]))
    } else {
        Some(for_in_expr)
    }
}

/// Lower a for-of statement
pub fn lower_for_of_stmt(for_of_stmt: &ast::ForOfStatement) -> Option<Statement> {
    let iterable = lower_expr(&for_of_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_of_stmt.body).unwrap_or(Statement::Empty));
    let dispose_async = match &for_of_stmt.left {
        ast::ForStatementLeft::VariableDeclaration(decl) => match decl.kind {
            ast::VariableDeclarationKind::Using => Some(false),
            ast::VariableDeclarationKind::AwaitUsing => Some(true),
            _ => None,
        },
        _ => None,
    };

    let (var_decl_stmt, loop_binding) =
        if let ast::ForStatementLeft::VariableDeclaration(ref decl) = &for_of_stmt.left {
            let kind = match decl.kind {
                ast::VariableDeclarationKind::Var => VarKind::Var,
                ast::VariableDeclarationKind::Let => VarKind::Let,
                ast::VariableDeclarationKind::Const => VarKind::Const,
                ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing => {
                    VarKind::Const
                }
            };
            let has_pattern = decl
                .declarations
                .iter()
                .any(|d| !matches!(d.id, ast::BindingPattern::BindingIdentifier(_)));
            if matches!(kind, VarKind::Var) {
                let vd = if has_pattern {
                    crate::lower::stmt::lower_for_in_var_pattern_hoist(decl)
                } else {
                    crate::lower::stmt::lower_var_decl(decl)
                };
                (vd, None)
            } else {
                (None, Some(kind))
            }
        } else {
            (None, None)
        };

    let variable = lower_for_lhs(&for_of_stmt.left)?;
    let for_of_expr = Statement::Expression(Box::new(Expression::ForOf {
        variable: Box::new(variable),
        iterable: Box::new(iterable),
        body,
        await_of: for_of_stmt.r#await,
        loop_binding,
        dispose_async,
    }));

    if let Some(var_stmt) = var_decl_stmt {
        Some(Statement::Block(vec![var_stmt, for_of_expr]))
    } else {
        Some(for_of_expr)
    }
}

/// Lower a try-catch-finally statement
pub fn lower_try_stmt(try_stmt: &ast::TryStatement) -> Option<Statement> {
    let body = crate::lower::stmt::lower_statement_list(&try_stmt.block.body);
    let (catch_param, handler) = match try_stmt.handler.as_ref() {
        Some(catch) => {
            let mut handler_stmts = Vec::new();
            let catch_param = match catch.param.as_ref() {
                Some(param) => match &param.pattern {
                    ast::BindingPattern::BindingIdentifier(ident) => {
                        Some(ident.name.as_str().to_string())
                    }
                    _ => {
                        let catch_param = next_catch_param_name();
                        let pattern = lower_binding_elem(&param.pattern).ok()?;
                        handler_stmts.push(Statement::PatternDeclaration {
                            kind: VarKind::Let,
                            pattern,
                            init: Some(Expression::Identifier(catch_param.clone())),
                        });
                        let nested_body = Statement::Block(
                            catch.body.body.iter().filter_map(lower_stmt).collect(),
                        );
                        handler_stmts.push(nested_body);
                        Some(catch_param)
                    }
                },
                None => None,
            };
            if catch_param.is_some() && handler_stmts.is_empty() {
                handler_stmts.extend(catch.body.body.iter().filter_map(lower_stmt));
            } else if catch
                .param
                .as_ref()
                .is_none_or(|p| matches!(&p.pattern, ast::BindingPattern::BindingIdentifier(_)))
            {
                handler_stmts.extend(catch.body.body.iter().filter_map(lower_stmt));
            }
            (catch_param, Some(Box::new(Statement::Block(handler_stmts))))
        }
        None => (None, None),
    };
    let finalizer = try_stmt.finalizer.as_ref().map(|fin| {
        Box::new(Statement::Block(
            fin.body.iter().filter_map(lower_stmt).collect(),
        ))
    });
    Some(Statement::Try {
        body: Box::new(body),
        param: catch_param,
        handler,
        finalizer,
    })
}

/// Check if a list of lowered statements ends with an unconditional
/// control-flow exit (break, return, throw) that prevents fall-through.
fn ends_with_break_or_return(stmts: &[Statement]) -> bool {
    let mut can_fall_through = true;
    for stmt in stmts {
        if !can_fall_through {
            continue;
        }
        if matches!(
            stmt,
            Statement::Break(_) | Statement::Return(_) | Statement::Throw(_)
        ) {
            can_fall_through = false;
        }
    }
    !can_fall_through
}

/// Lower a switch statement into nested if-else chains
pub fn lower_switch(switch: &ast::SwitchStatement) -> Option<Statement> {
    let discriminant = lower_expr(&switch.discriminant).ok()?;
    let switch_scope_id = next_switch_scope_id();
    let disc_name = format!("__quench_switch_discriminant_{switch_scope_id}");
    let loop_name = format!("__quench_switch_{switch_scope_id}");
    let own_bodies: Vec<Vec<Statement>> = switch
        .cases
        .iter()
        .map(|case| case.consequent.iter().filter_map(lower_stmt).collect())
        .collect();

    // Compute effective bodies: walk backwards, accumulating bodies
    // of cases that don't end with break/return. Effective body for
    // case[i] = own[i] + (own[i+1] + own[i+2] + ... until break).
    // Prepend own[i] before accumulated suffix to maintain source order.
    let case_count = switch.cases.len();
    let mut effective_bodies: Vec<Vec<Statement>> = Vec::with_capacity(case_count);
    let mut suffix: Vec<Statement> = Vec::new(); // accumulated suffix for fall-through
    for i in (0..case_count).rev() {
        let ends_with_break = ends_with_break_or_return(&own_bodies[i]);
        let mut effective = own_bodies[i].clone();
        // If the current case doesn't end with break/return, it falls
        // through to the accumulated suffix (subsequent cases).
        if !ends_with_break {
            effective.extend(suffix.clone());
        }
        // Update suffix for the next iteration (going backwards):
        // suffix becomes own[i] if it has break/return, or own[i] + old_suffix.
        suffix = if ends_with_break {
            own_bodies[i].clone()
        } else {
            let mut s = own_bodies[i].clone();
            s.append(&mut suffix);
            s
        };
        effective_bodies.push(effective);
    }
    effective_bodies.reverse();

    // Build the if-else chain. Default case must come LAST so that
    // cases after default in source order are still reachable.
    // First, collect all non-default cases and their effective bodies.
    let mut non_default: Vec<(usize, Vec<Statement>)> = Vec::new();
    let mut default_body: Option<Statement> = None;
    for (i, case) in switch.cases.iter().enumerate() {
        if case.test.is_some() {
            non_default.push((i, effective_bodies[i].clone()));
        } else {
            default_body = Some(Statement::SequenceDecls(effective_bodies[i].clone()));
        }
    }

    // Build the chain: non-default cases in reverse order, then default at the end.
    let mut current: Option<Statement> = default_body;
    for (i, case_body) in non_default.into_iter().rev() {
        let test = switch.cases[i].test.as_ref().unwrap();
        let test_expr = lower_expr(test).ok()?;
        current = Some(Statement::If {
            condition: Box::new(Expression::Binary {
                op: BinaryOp::StrictEq,
                left: Box::new(Expression::Identifier(disc_name.clone())),
                right: Box::new(test_expr),
            }),
            consequent: Box::new(Statement::SequenceDecls(case_body)),
            alternate: current.map(Box::new),
        });
    }
    let chain = current.unwrap_or(Statement::Empty);

    // The if-else chain cannot contain a `break` meant for the switch: it
    // would escape to the enclosing function or loop. Wrap the chain in a
    // one-shot counter loop, which consumes Break (and Continue) and always
    // terminates. `return` inside a case body still propagates.
    let for_stmt = Statement::For {
        init: Some(ForInit::VarDeclaration {
            kind: VarKind::Var,
            name: loop_name.clone(),
            init: Some(Expression::Number(0.0)),
        }),
        condition: Some(Box::new(Expression::Binary {
            op: BinaryOp::Lt,
            left: Box::new(Expression::Identifier(loop_name.clone())),
            right: Box::new(Expression::Number(1.0)),
        })),
        update: Some(Box::new(Expression::Update {
            op: crate::ast::UpdateOp::Increment,
            argument: Box::new(Expression::Identifier(loop_name)),
            prefix: false,
        })),
        body: Box::new(chain),
    };

    let mut switch_scope_stmts = Vec::new();
    for decl_name in collect_switch_case_decls(&own_bodies) {
        // Switch CaseBlockDeclarationInstantiation performs predecl of
        // lexical declarations (including function declarations) before
        // evaluating selector expressions.
        switch_scope_stmts.push(Statement::VarDeclaration {
            kind: VarKind::Let,
            name: decl_name,
            init: None,
        });
    }
    switch_scope_stmts.push(for_stmt);

    let lowered_switch = Statement::Block(vec![
        Statement::VarDeclaration {
            kind: VarKind::Var,
            name: disc_name,
            init: Some(discriminant),
        },
        Statement::Block(switch_scope_stmts),
    ]);
    Some(lowered_switch)
}

fn collect_switch_case_decls(case_bodies: &[Vec<Statement>]) -> Vec<String> {
    let mut names = HashSet::new();
    let mut out = Vec::new();

    fn collect_from_stmt(stmt: &Statement, names: &mut HashSet<String>, out: &mut Vec<String>) {
        match stmt {
            Statement::VarDeclaration { kind, name, .. } => {
                if *kind != VarKind::Var && names.insert(name.clone()) {
                    out.push(name.clone());
                }
            }
            Statement::PatternDeclaration { kind, pattern, .. } => {
                if *kind != VarKind::Var {
                    collect_pattern_names(pattern, names, out);
                }
            }
            Statement::FunctionDeclaration { name, .. }
            | Statement::ClassDeclaration { name, .. } => {
                if names.insert(name.clone()) {
                    out.push(name.clone());
                }
            }
            Statement::Block(stmts) => {
                for inner in stmts {
                    collect_from_stmt(inner, names, out);
                }
            }
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                collect_from_stmt(consequent, names, out);
                if let Some(alt) = alternate {
                    collect_from_stmt(alt, names, out);
                }
            }
            Statement::While { body, .. } => {
                collect_from_stmt(body, names, out);
            }
            Statement::DoWhile { body, .. } => {
                collect_from_stmt(body, names, out);
            }
            Statement::For { body, .. } => {
                collect_from_stmt(body, names, out);
            }
            Statement::ForIn { body, .. } => {
                collect_from_stmt(body, names, out);
            }
            Statement::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                collect_from_stmt(body, names, out);
                if let Some(handler) = handler {
                    collect_from_stmt(handler, names, out);
                }
                if let Some(finalizer) = finalizer {
                    collect_from_stmt(finalizer, names, out);
                }
            }
            _ => {}
        }
    }

    fn collect_pattern_names(
        pattern: &BindingElement,
        names: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        match pattern {
            BindingElement::Identifier(name) => {
                if names.insert(name.clone()) {
                    out.push(name.clone());
                }
            }
            BindingElement::ArrayPattern(patterns) => {
                for inner in patterns {
                    collect_pattern_names(inner, names, out);
                }
            }
            BindingElement::ObjectPattern(props) => {
                for (_, value) in props {
                    collect_pattern_names(value, names, out);
                }
            }
            BindingElement::Default(inner, _) => {
                collect_pattern_names(inner, names, out);
            }
            BindingElement::Rest(inner) => {
                collect_pattern_names(inner, names, out);
            }
            BindingElement::AssignmentTarget(_) => {}
        }
    }

    for case in case_bodies {
        for stmt in case {
            collect_from_stmt(stmt, &mut names, &mut out);
        }
    }
    out
}

/// Lower a for loop init (variable declaration or expression)
#[allow(clippy::complexity)]
pub fn lower_for_init(init: &ast::ForStatementInit) -> Option<ForInit> {
    match init {
        ast::ForStatementInit::VariableDeclaration(decl) => {
            let kind = match decl.kind {
                ast::VariableDeclarationKind::Var => VarKind::Var,
                ast::VariableDeclarationKind::Let => VarKind::Let,
                ast::VariableDeclarationKind::Const => VarKind::Const,
                ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing => {
                    VarKind::Const
                }
            };
            if decl.declarations.len() > 1 {
                let mut decls = Vec::with_capacity(decl.declarations.len());
                for d in &decl.declarations {
                    decls.push(lower_for_init_decl(d, kind)?);
                }
                return Some(ForInit::DeclarationList { kind, decls });
            }
            let first = decl.declarations.first()?;
            lower_for_init_decl(first, kind).map(|item| {
                if let Some(name) = item.name {
                    ForInit::VarDeclaration {
                        kind,
                        name,
                        init: item.init,
                    }
                } else {
                    ForInit::PatternDeclaration {
                        kind,
                        pattern: item.pattern.expect("non-identifier decl has pattern"),
                        init: item.init,
                    }
                }
            })
        }
        _ => {
            if let Some(expr) = init.as_expression() {
                Some(ForInit::Expression(Box::new(lower_expr(expr).ok()?)))
            } else {
                None
            }
        }
    }
}

fn lower_for_init_decl(decl: &ast::VariableDeclarator, _kind: VarKind) -> Option<ForInitDecl> {
    let init = decl.init.as_ref().and_then(|e| lower_expr(e).ok());
    match &decl.id {
        ast::BindingPattern::BindingIdentifier(ident) => Some(ForInitDecl {
            name: Some(ident.name.as_str().to_string()),
            pattern: None,
            init,
        }),
        _ => {
            let pattern = lower_binding_elem(&decl.id).ok()?;
            Some(ForInitDecl {
                name: None,
                pattern: Some(pattern),
                init,
            })
        }
    }
}

/// Lower the left-hand side of a for-in/for-of loop
#[allow(clippy::complexity)]
pub fn lower_for_lhs(left: &ast::ForStatementLeft) -> Option<Expression> {
    match left {
        ast::ForStatementLeft::VariableDeclaration(decl) => {
            let first = decl.declarations.first()?;
            match &first.id {
                ast::BindingPattern::BindingIdentifier(ident) => {
                    Some(Expression::Identifier(ident.name.as_str().to_string()))
                }
                ast::BindingPattern::ArrayPattern(arr) => lower_array_lhs(arr),
                ast::BindingPattern::ObjectPattern(obj) => lower_object_lhs(obj),
                ast::BindingPattern::AssignmentPattern(_) => None,
            }
        }
        // ForStatementLeft inherits AssignmentTarget variants via macro
        ast::ForStatementLeft::AssignmentTargetIdentifier(ident_ref) => {
            Some(Expression::Identifier(ident_ref.name.as_str().to_string()))
        }
        // Array and object assignment targets in for-in/for-of
        ast::ForStatementLeft::ArrayAssignmentTarget(arr) => {
            lower_array_assignment_target(arr).ok().map(binding_to_expr)
        }
        ast::ForStatementLeft::ObjectAssignmentTarget(obj) => lower_object_assignment_target(obj)
            .ok()
            .map(binding_to_expr),
        // TS type assertions on for-statement left side
        ast::ForStatementLeft::TSAsExpression(e) => lower_expr(&e.expression).ok(),
        ast::ForStatementLeft::TSSatisfiesExpression(e) => lower_expr(&e.expression).ok(),
        ast::ForStatementLeft::TSNonNullExpression(e) => lower_expr(&e.expression).ok(),
        ast::ForStatementLeft::TSTypeAssertion(e) => lower_expr(&e.expression).ok(),
        ast::ForStatementLeft::StaticMemberExpression(sm) => {
            let obj = lower_expr(&sm.object).ok()?;
            Some(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(sm.property.name.as_str().to_string()),
                computed: false,
            })
        }
        ast::ForStatementLeft::ComputedMemberExpression(cm) => {
            let obj = lower_expr(&cm.object).ok()?;
            let prop = lower_expr(&cm.expression).ok()?;
            Some(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Computed(Box::new(prop)),
                computed: true,
            })
        }
        ast::ForStatementLeft::PrivateFieldExpression(pf) => {
            let obj = lower_expr(&pf.object).ok()?;
            Some(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(crate::value::private_name_key(
                    pf.field.name.as_str(),
                )),
                computed: false,
            })
        }
    }
}

fn lower_array_lhs(arr: &ast::ArrayPattern) -> Option<Expression> {
    match crate::lower::pattern::lower_array_binding(arr).ok()? {
        BindingElement::ArrayPattern(elements) => Some(Expression::ArrayPattern(elements)),
        _ => None,
    }
}

fn lower_object_lhs(obj: &ast::ObjectPattern) -> Option<Expression> {
    match crate::lower::pattern::lower_object_binding(obj).ok()? {
        BindingElement::ObjectPattern(props) => Some(Expression::ObjectPattern(props)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{BinaryOp, Expression, ForInit, PropertyKey, Statement, VarKind};

    #[test]
    fn test_statement_has_explicit_return_true() {
        let stmt = Statement::Block(vec![Statement::Return(Some(Box::new(Expression::Number(
            42.0,
        ))))]);
        assert!(stmt.has_explicit_return());
    }

    #[test]
    fn test_statement_has_explicit_return_nested_if() {
        let stmt = Statement::If {
            condition: Box::new(Expression::Boolean(true)),
            consequent: Box::new(Statement::Return(Some(Box::new(Expression::Number(1.0))))),
            alternate: None,
        };
        assert!(stmt.has_explicit_return());
    }

    #[test]
    fn test_statement_has_explicit_return_false() {
        let stmt = Statement::Block(vec![Statement::Expression(Box::new(Expression::Number(
            42.0,
        )))]);
        assert!(!stmt.has_explicit_return());
    }

    #[test]
    fn test_statement_has_explicit_return_nested_no_return() {
        let stmt = Statement::If {
            condition: Box::new(Expression::Boolean(true)),
            consequent: Box::new(Statement::Block(vec![Statement::Expression(Box::new(
                Expression::Number(1.0),
            ))])),
            alternate: None,
        };
        assert!(!stmt.has_explicit_return());
    }

    #[test]
    fn test_statement_has_explicit_return_try_with_return() {
        let stmt = Statement::Try {
            body: Box::new(Statement::Block(vec![Statement::Return(Some(Box::new(
                Expression::Number(1.0),
            )))])),
            param: None,
            handler: None,
            finalizer: None,
        };
        assert!(stmt.has_explicit_return());
    }

    #[test]
    fn test_statement_has_explicit_return_while() {
        let stmt = Statement::While {
            condition: Box::new(Expression::Boolean(true)),
            body: Box::new(Statement::Return(Some(Box::new(Expression::Number(5.0))))),
        };
        assert!(stmt.has_explicit_return());
    }

    #[test]
    fn test_binary_op_precedence() {
        assert_eq!(BinaryOp::Or.precedence(), 1);
        assert_eq!(BinaryOp::And.precedence(), 2);
        assert_eq!(BinaryOp::StrictEq.precedence(), 6);
        assert_eq!(BinaryOp::Add.precedence(), 9);
        assert_eq!(BinaryOp::Mul.precedence(), 10);
        assert_eq!(BinaryOp::Pow.precedence(), 11);
    }

    #[test]
    fn test_binary_op_precedence_nullish() {
        assert_eq!(BinaryOp::NullishCoalescing.precedence(), 1);
    }

    #[test]
    fn test_var_kind_derives() {
        assert_eq!(VarKind::Var, VarKind::Var);
        assert_eq!(VarKind::Let, VarKind::Let);
        assert_eq!(VarKind::Const, VarKind::Const);
    }

    #[test]
    fn test_for_init_variants() {
        let var_init = ForInit::VarDeclaration {
            kind: VarKind::Let,
            name: "x".to_string(),
            init: Some(Expression::Number(1.0)),
        };
        let expr_init = ForInit::Expression(Box::new(Expression::Number(0.0)));
        assert!(matches!(var_init, ForInit::VarDeclaration { .. }));
        assert!(matches!(expr_init, ForInit::Expression(_)));
    }

    #[test]
    fn test_property_key_variants() {
        let ident = PropertyKey::Ident("foo".to_string());
        let string = PropertyKey::String("bar".to_string());
        let number = PropertyKey::Number(42.0);
        let computed = PropertyKey::Computed(Box::new(Expression::Identifier("key".to_string())));
        assert!(matches!(ident, PropertyKey::Ident(_)));
        assert!(matches!(string, PropertyKey::String(_)));
        assert!(matches!(number, PropertyKey::Number(_)));
        assert!(matches!(computed, PropertyKey::Computed(_)));
    }
}
