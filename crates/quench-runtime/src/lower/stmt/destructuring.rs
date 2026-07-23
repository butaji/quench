//! Destructuring pattern lowering functions

use crate::ast::{Expression, PropertyKey, Statement, VarKind};
use oxc::ast::ast;

use crate::lower::pattern::{
    expand_nested_array_pattern, expand_nested_pattern, lower_array_binding,
};

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

/// Lower object destructuring pattern
pub fn lower_object_destructuring(
    kind: VarKind,
    obj: &ast::ObjectPattern,
    init_expr: Option<Expression>,
    idx: usize,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    let temp_var_name = format!("__obj_src_{}", idx);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: temp_var_name.clone(),
        init: init_expr,
    });
    lower_object_props(kind, obj, &temp_var_name, &mut stmts);
    stmts
}

fn lower_object_props(
    kind: VarKind,
    obj: &ast::ObjectPattern,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    for prop in &obj.properties {
        let key_str = match &prop.key {
            ast::PropertyKey::StaticIdentifier(i) => i.name.as_str().to_string(),
            ast::PropertyKey::StringLiteral(s) => s.value.to_string(),
            ast::PropertyKey::NumericLiteral(n) => n.value.to_string(),
            _ => continue,
        };

        if prop.shorthand {
            // Shorthand: { x } or { x = default }
            let var_name = key_str.clone();
            let member = Expression::Member {
                object: Box::new(Expression::Identifier(temp_var_name.to_string())),
                property: PropertyKey::String(key_str.clone()),
                computed: false,
            };

            // Check for default value in the value (AssignmentPattern)
            if let ast::BindingPatternKind::AssignmentPattern(assign) = &prop.value.kind {
                if let Ok(default_expr) = crate::lower::expr::lower_expr(&assign.right) {
                    let initializer = Expression::Binary {
                        left: Box::new(member),
                        op: crate::ast::BinaryOp::NullishCoalescing,
                        right: Box::new(default_expr),
                    };
                    stmts.push(Statement::VarDeclaration {
                        kind,
                        name: var_name,
                        init: Some(initializer),
                    });
                }
            } else {
                stmts.push(Statement::VarDeclaration {
                    kind,
                    name: var_name,
                    init: Some(member),
                });
            }
        } else {
            // Key-value: { x: y } or { x: y = default }
            let kv_value_ref: &ast::BindingPattern = &prop.value;
            let var_name = match &kv_value_ref.kind {
                ast::BindingPatternKind::BindingIdentifier(id) => id.name.as_str().to_string(),
                _ => format!("__obj_temp_{}", key_str),
            };
            let member = Expression::Member {
                object: Box::new(Expression::Identifier(temp_var_name.to_string())),
                property: PropertyKey::String(key_str.clone()),
                computed: false,
            };
            add_obj_destructure_stmts(kind, kv_value_ref, var_name, member, key_str, stmts);
        }
    }

    // Handle rest element
    if let Some(rest) = &obj.rest {
        let rest_temp_name = format!("__obj_rest_{}", temp_var_name);
        stmts.push(Statement::VarDeclaration {
            kind: VarKind::Const,
            name: rest_temp_name.clone(),
            init: Some(Expression::Identifier(temp_var_name.to_string())),
        });
        stmts.extend(expand_nested_pattern(kind, &rest.argument, &rest_temp_name));
    }
}

fn add_obj_destructure_stmts(
    kind: VarKind,
    kv_value_ref: &ast::BindingPattern,
    var_name: String,
    member: Expression,
    key_str: String,
    stmts: &mut Vec<Statement>,
) {
    match &kv_value_ref.kind {
        ast::BindingPatternKind::BindingIdentifier(_) => {
            stmts.push(Statement::VarDeclaration {
                kind,
                name: var_name,
                init: Some(member),
            });
        }
        ast::BindingPatternKind::ObjectPattern(nested_obj) => {
            handle_obj_nested(kind, member, key_str, nested_obj, stmts);
        }
        ast::BindingPatternKind::ArrayPattern(nested_arr) => {
            handle_arr_nested(kind, member, key_str, nested_arr, stmts);
        }
        ast::BindingPatternKind::AssignmentPattern(_) => {
            stmts.push(Statement::VarDeclaration {
                kind,
                name: var_name,
                init: Some(member),
            });
        }
    }
}

fn handle_obj_nested(
    kind: VarKind,
    member: Expression,
    key_str: String,
    nested_obj: &ast::ObjectPattern,
    stmts: &mut Vec<Statement>,
) {
    use crate::lower::pattern::expand_nested_object_pattern;
    let nested_temp_name = format!("__obj_prop_{}", key_str);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: nested_temp_name.clone(),
        init: Some(member),
    });
    stmts.extend(expand_nested_object_pattern(
        kind,
        nested_obj,
        &nested_temp_name,
    ));
}

fn handle_arr_nested(
    kind: VarKind,
    member: Expression,
    key_str: String,
    nested_arr: &ast::ArrayPattern,
    stmts: &mut Vec<Statement>,
) {
    let nested_temp_name = format!("__obj_prop_{}", key_str);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: nested_temp_name.clone(),
        init: Some(member),
    });
    stmts.extend(expand_nested_array_pattern(
        kind,
        nested_arr,
        &nested_temp_name,
    ));
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
