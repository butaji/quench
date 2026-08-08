use super::{expand_nested_array_pattern, expand_nested_object_pattern};
use crate::ast::{Expression, PropertyKey, Statement, VarKind};
use oxc::ast::ast;

pub fn add_object_kv_stmts(
    kind: VarKind,
    kv_value_ref: &ast::BindingPattern,
    var_name: String,
    member: Expression,
    source_var: &str,
    key_str: String,
    stmts: &mut Vec<Statement>,
) {
    match &kv_value_ref.kind {
        ast::BindingPatternKind::BindingIdentifier(_) => {
            push_simple_decl(kind, var_name, member, stmts)
        }
        ast::BindingPatternKind::ObjectPattern(nested_obj) => {
            handle_nested_object(kind, member, source_var, key_str, nested_obj, stmts);
        }
        ast::BindingPatternKind::ArrayPattern(nested_arr) => {
            handle_nested_array(kind, member, source_var, key_str, nested_arr, stmts);
        }
        ast::BindingPatternKind::AssignmentPattern(_) => {
            push_simple_decl(kind, var_name, member, stmts)
        }
    }
}

fn push_simple_decl(kind: VarKind, name: String, init: Expression, stmts: &mut Vec<Statement>) {
    stmts.push(Statement::VarDeclaration {
        kind,
        name,
        init: Some(init),
    });
}

fn handle_nested_object(
    kind: VarKind,
    member: Expression,
    source_var: &str,
    key_str: String,
    nested_obj: &ast::ObjectPattern,
    stmts: &mut Vec<Statement>,
) {
    let nested_temp_name = format!("{}_prop_{}", source_var, key_str);
    push_const_decl(nested_temp_name.clone(), member, stmts);
    stmts.extend(expand_nested_object_pattern(
        kind,
        nested_obj,
        &nested_temp_name,
    ));
}

fn handle_nested_array(
    kind: VarKind,
    member: Expression,
    source_var: &str,
    key_str: String,
    nested_arr: &ast::ArrayPattern,
    stmts: &mut Vec<Statement>,
) {
    let nested_temp_name = format!("{}_prop_{}", source_var, key_str);
    push_const_decl(nested_temp_name.clone(), member, stmts);
    stmts.extend(expand_nested_array_pattern(
        kind,
        nested_arr,
        &nested_temp_name,
    ));
}

fn push_const_decl(name: String, init: Expression, stmts: &mut Vec<Statement>) {
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name,
        init: Some(init),
    });
}

pub fn object_member_expr(source_var: &str, key: &str) -> Expression {
    Expression::Member {
        object: Box::new(Expression::Identifier(source_var.to_string())),
        property: PropertyKey::String(key.to_string()),
        computed: false,
    }
}
