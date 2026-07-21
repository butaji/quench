//! Control flow statement lowering
//!
//! Handles lowering of if, while, for, try-catch, switch statements.

use super::expr::lower_expr;
use super::pattern::{
    binding_to_expr, lower_array_assignment_target, lower_elem_pat, lower_object_assignment_target,
    lower_object_pat_prop,
};
use super::stmt::lower_stmt;
use crate::ast::{BinaryOp, BindingElement, Expression, ForInit, PropertyKey, Statement, VarKind};
use oxc::ast::ast;

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
pub fn lower_for_in_stmt(for_in_stmt: &ast::ForInStatement) -> Option<Statement> {
    let iterable = lower_expr(&for_in_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_in_stmt.body).unwrap_or(Statement::Empty));

    // When the left side is a VariableDeclaration, also emit the var/let/const
    // declaration so the binding is created in the environment.
    // For patterns, pass the iterable so the pattern can access elements from it.
    let var_decl_stmt =
        if let ast::ForStatementLeft::VariableDeclaration(ref decl) = &for_in_stmt.left {
            let has_pattern = decl
                .declarations
                .iter()
                .any(|d| !matches!(d.id.kind, ast::BindingPatternKind::BindingIdentifier(_)));
            if has_pattern {
                crate::lower::stmt::lower_var_decl_impl(decl, Some(iterable.clone()))
            } else {
                crate::lower::stmt::lower_var_decl(decl)
            }
        } else {
            None
        };

    let variable = lower_for_lhs(&for_in_stmt.left)?;
    let for_in_expr = Statement::Expression(Box::new(Expression::ForIn {
        variable: Box::new(variable),
        object: Box::new(iterable),
        body,
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

    // When the left side is a VariableDeclaration, also emit the var/let/const
    // declaration so the binding is created in the environment.
    // For patterns, pass the iterable so the pattern can access elements from it.
    let var_decl_stmt =
        if let ast::ForStatementLeft::VariableDeclaration(ref decl) = &for_of_stmt.left {
            let has_pattern = decl
                .declarations
                .iter()
                .any(|d| !matches!(d.id.kind, ast::BindingPatternKind::BindingIdentifier(_)));
            let mut vd = if has_pattern {
                crate::lower::stmt::lower_var_decl_impl(decl, Some(iterable.clone()))
            } else {
                crate::lower::stmt::lower_var_decl(decl)
            };
            // For-of with const should create a new binding per iteration (ES spec),
            // but our runtime reassigns the same binding. Use let to avoid errors.
            if let Some(Statement::VarDeclaration { ref mut kind, .. }) = vd {
                if *kind == VarKind::Const {
                    *kind = VarKind::Let;
                }
            }
            vd
        } else {
            None
        };

    let variable = lower_for_lhs(&for_of_stmt.left)?;
    let for_of_expr = Statement::Expression(Box::new(Expression::ForOf {
        variable: Box::new(variable),
        iterable: Box::new(iterable),
        body,
    }));

    if let Some(var_stmt) = var_decl_stmt {
        Some(Statement::Block(vec![var_stmt, for_of_expr]))
    } else {
        Some(for_of_expr)
    }
}

/// Lower a try-catch-finally statement
pub fn lower_try_stmt(try_stmt: &ast::TryStatement) -> Option<Statement> {
    let body = try_stmt
        .block
        .body
        .iter()
        .filter_map(lower_stmt)
        .collect::<Vec<_>>();
    let catch_param = try_stmt.handler.as_ref().and_then(|catch| {
        catch
            .param
            .as_ref()
            .and_then(|pat| match &pat.pattern.kind {
                ast::BindingPatternKind::BindingIdentifier(ident) => {
                    Some(ident.name.as_str().to_string())
                }
                _ => None,
            })
    });
    let handler = try_stmt.handler.as_ref().map(|catch| {
        Box::new(Statement::Block(
            catch.body.body.iter().filter_map(lower_stmt).collect(),
        ))
    });
    let finalizer = try_stmt.finalizer.as_ref().map(|fin| {
        Box::new(Statement::Block(
            fin.body.iter().filter_map(lower_stmt).collect(),
        ))
    });
    Some(Statement::Try {
        body: Box::new(Statement::Block(body)),
        param: catch_param,
        handler,
        finalizer,
    })
}

/// Lower a switch statement into nested if-else chains
pub fn lower_switch(switch: &ast::SwitchStatement) -> Option<Statement> {
    let discriminant = lower_expr(&switch.discriminant).ok()?;

    // Compute each case's effective body: a case with no statements falls
    // through to the next case's body (JS fall-through semantics).
    let mut effective_bodies: Vec<Vec<Statement>> = Vec::with_capacity(switch.cases.len());
    let mut next_body: Vec<Statement> = Vec::new();
    for case in switch.cases.iter().rev() {
        let own: Vec<Statement> = case.consequent.iter().filter_map(lower_stmt).collect();
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
pub fn lower_for_init(init: &ast::ForStatementInit) -> Option<ForInit> {
    match init {
        ast::ForStatementInit::VariableDeclaration(decl) => {
            let first = decl.declarations.first()?;
            let kind = match decl.kind {
                ast::VariableDeclarationKind::Var => VarKind::Var,
                ast::VariableDeclarationKind::Let => VarKind::Let,
                ast::VariableDeclarationKind::Const => VarKind::Const,
                // Using/AwaitUsing not supported in this runtime
                ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing => {
                    return None;
                }
            };
            let name = match &first.id.kind {
                ast::BindingPatternKind::BindingIdentifier(ident) => {
                    ident.name.as_str().to_string()
                }
                _ => return None,
            };
            let init = first.init.as_ref().and_then(|e| lower_expr(e).ok());
            Some(ForInit::VarDeclaration { kind, name, init })
        }
        // ForStatementInit inherits Expression variants via macro
        _ => {
            // Try to match as expression
            if let Some(expr) = init.as_expression() {
                Some(ForInit::Expression(Box::new(lower_expr(expr).ok()?)))
            } else {
                None
            }
        }
    }
}

/// Lower the left-hand side of a for-in/for-of loop
#[allow(clippy::complexity)]
pub fn lower_for_lhs(left: &ast::ForStatementLeft) -> Option<Expression> {
    match left {
        ast::ForStatementLeft::VariableDeclaration(decl) => {
            let first = decl.declarations.first()?;
            match &first.id.kind {
                ast::BindingPatternKind::BindingIdentifier(ident) => {
                    Some(Expression::Identifier(ident.name.as_str().to_string()))
                }
                ast::BindingPatternKind::ArrayPattern(arr) => lower_array_lhs(arr),
                ast::BindingPatternKind::ObjectPattern(obj) => lower_object_lhs(obj),
                ast::BindingPatternKind::AssignmentPattern(_) => None,
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
        ast::ForStatementLeft::TSInstantiationExpression(e) => lower_expr(&e.expression).ok(),
        // Member expression variants on for-statement left side — not bindings, return None
        ast::ForStatementLeft::ComputedMemberExpression(_) => None,
        ast::ForStatementLeft::StaticMemberExpression(_) => None,
        ast::ForStatementLeft::PrivateFieldExpression(_) => None,
    }
}

fn lower_array_lhs(arr: &ast::ArrayPattern) -> Option<Expression> {
    let elements: Vec<BindingElement> = arr
        .elements
        .iter()
        .filter_map(|e| e.as_ref().and_then(lower_elem_pat))
        .chain(arr.rest.as_ref().and_then(|r| lower_elem_pat(&r.argument)))
        .collect();
    Some(Expression::ArrayPattern(elements))
}

fn lower_object_lhs(obj: &ast::ObjectPattern) -> Option<Expression> {
    let props: Vec<(PropertyKey, BindingElement)> = obj
        .properties
        .iter()
        .filter_map(lower_object_pat_prop)
        .collect();
    Some(Expression::ObjectPattern(props))
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
