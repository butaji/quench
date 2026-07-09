//! Destructuring pattern lowering functions

use swc_ecma_ast as swc;
use crate::ast::{Expression, PropertyKey, Statement, VarKind};

use crate::lower::helpers::{atom_to_string, wtf8_atom_to_string};
use crate::lower::pattern::{expand_nested_pattern, expand_nested_array_pattern};

/// Lower array destructuring pattern
pub fn lower_array_destructuring(
    kind: VarKind,
    arr: &swc::ArrayPat,
    init_expr: Option<Expression>,
    idx: usize,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    let temp_var_name = format!("__arr_src_{}", idx);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: temp_var_name.clone(),
        init: init_expr,
    });
    lower_array_elems(kind, arr, &temp_var_name, &mut stmts);
    stmts
}

fn lower_array_elems(
    kind: VarKind,
    arr: &swc::ArrayPat,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    for (i, elem) in arr.elems.iter().enumerate() {
        if let Some(elem) = elem {
            let member = array_member_access(temp_var_name, i);
            match elem {
                swc::Pat::Ident(id) => {
                    stmts.push(Statement::VarDeclaration {
                        kind,
                        name: atom_to_string(&id.id.sym),
                        init: Some(member),
                    });
                }
                _ => {
                    let elem_temp_name = format!("__arr_elem_{}", i);
                    stmts.push(Statement::VarDeclaration {
                        kind: VarKind::Const,
                        name: elem_temp_name.clone(),
                        init: Some(member),
                    });
                    stmts.extend(expand_nested_pattern(kind, elem, &elem_temp_name));
                }
            }
        }
    }
}

fn array_member_access(source_var: &str, index: usize) -> Expression {
    Expression::Member {
        object: Box::new(Expression::Identifier(source_var.to_string())),
        property: PropertyKey::Number(index as f64),
        computed: false,
    }
}

/// Lower object destructuring pattern
pub fn lower_object_destructuring(
    kind: VarKind,
    obj: &swc::ObjectPat,
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
    obj: &swc::ObjectPat,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    for prop in &obj.props {
        match prop {
            swc::ObjectPatProp::KeyValue(kv) => {
                handle_obj_kv_prop(kind, kv, temp_var_name, stmts);
            }
            swc::ObjectPatProp::Assign(assign) => {
                handle_obj_assign_prop(kind, assign, temp_var_name, stmts);
            }
            swc::ObjectPatProp::Rest(_) => {}
        }
    }
}

fn handle_obj_kv_prop(
    kind: VarKind,
    kv: &swc::KeyValuePatProp,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    let key_str = match &kv.key {
        swc::PropName::Ident(i) => atom_to_string(&i.sym),
        swc::PropName::Str(s) => wtf8_atom_to_string(&s.value),
        swc::PropName::Num(n) => n.value.to_string(),
        _ => return,
    };
    if key_str.is_empty() {
        return;
    }
    let kv_value_ref: &swc::Pat = &kv.value;
    let var_name = match kv_value_ref {
        swc::Pat::Ident(id) => atom_to_string(&id.id.sym),
        _ => format!("__obj_temp_{}", key_str),
    };
    let member = Expression::Member {
        object: Box::new(Expression::Identifier(temp_var_name.to_string())),
        property: PropertyKey::String(key_str.clone()),
        computed: false,
    };
    add_obj_destructure_stmts(kind, kv_value_ref, var_name, member, key_str, stmts);
}

fn handle_obj_assign_prop(
    kind: VarKind,
    assign: &swc::AssignPatProp,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    let var_name = atom_to_string(&assign.key.sym);
    let member = Expression::Member {
        object: Box::new(Expression::Identifier(temp_var_name.to_string())),
        property: PropertyKey::Ident(var_name.clone()),
        computed: false,
    };
    stmts.push(Statement::VarDeclaration {
        kind,
        name: var_name,
        init: Some(member),
    });
}

fn add_obj_destructure_stmts(
    kind: VarKind,
    kv_value_ref: &swc::Pat,
    var_name: String,
    member: Expression,
    key_str: String,
    stmts: &mut Vec<Statement>,
) {
    match kv_value_ref {
        swc::Pat::Ident(_) => {
            stmts.push(Statement::VarDeclaration {
                kind,
                name: var_name,
                init: Some(member),
            });
        }
        swc::Pat::Object(nested_obj) => {
            handle_obj_nested(kind, member, key_str, nested_obj, stmts);
        }
        swc::Pat::Array(nested_arr) => {
            handle_arr_nested(kind, member, key_str, nested_arr, stmts);
        }
        _ => {
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
    nested_obj: &swc::ObjectPat,
    stmts: &mut Vec<Statement>,
) {
    use crate::lower::pattern::expand_nested_object_pattern;
    let nested_temp_name = format!("__obj_prop_{}", key_str);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: nested_temp_name.clone(),
        init: Some(member),
    });
    stmts.extend(expand_nested_object_pattern(kind, nested_obj, &nested_temp_name));
}

fn handle_arr_nested(
    kind: VarKind,
    member: Expression,
    key_str: String,
    nested_arr: &swc::ArrayPat,
    stmts: &mut Vec<Statement>,
) {
    let nested_temp_name = format!("__obj_prop_{}", key_str);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: nested_temp_name.clone(),
        init: Some(member),
    });
    stmts.extend(expand_nested_array_pattern(kind, nested_arr, &nested_temp_name));
}

/// Wrap declarations in appropriate statement(s).
/// For var declarations, return them as individual statements to avoid
/// creating a new block scope (var is function-scoped, not block-scoped).
/// For let/const, wrap in a Block since they're block-scoped.
pub fn wrap_decls(decls: Vec<Statement>) -> Option<Statement> {
    if decls.is_empty() {
        return Some(Statement::Empty);
    }
    if decls.len() == 1 {
        return Some(decls.into_iter().next().unwrap());
    }

    // Check if all declarations are var - if so, don't wrap in Block
    // to avoid creating a new scope (var is function-scoped)
    let all_var = decls.iter().all(|s| {
        matches!(s, Statement::VarDeclaration { kind: VarKind::Var, .. })
    });

    if all_var {
        // Return as Sequence of individual statements
        // This is handled specially in the stack machine
        Some(Statement::SequenceDecls(decls))
    } else {
        Some(Statement::Block(decls))
    }
}
