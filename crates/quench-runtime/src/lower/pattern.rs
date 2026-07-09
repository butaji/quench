//! Pattern lowering - destructuring and binding patterns

use swc_ecma_ast as swc;
use crate::ast::{BindingElement, Expression, PropertyKey, Statement, VarKind};
use super::helpers::{atom_to_string, wtf8_atom_to_string, LowerError};

/// Lower a binding pattern (for destructuring) to BindingElement
pub fn lower_binding_elem(pat: &swc::Pat) -> Result<BindingElement, LowerError> {
    match pat {
        swc::Pat::Ident(ident) => {
            Ok(BindingElement::Identifier(atom_to_string(&ident.id.sym)))
        }
        swc::Pat::Array(arr) => lower_array_pattern(arr),
        swc::Pat::Object(obj) => lower_object_pattern(obj),
        swc::Pat::Rest(rest) => lower_binding_elem(&rest.arg),
        swc::Pat::Assign(assign) => lower_binding_elem(&assign.left),
        _ => Err(LowerError::new("Unsupported binding pattern")),
    }
}

fn lower_array_pattern(arr: &swc::ArrayPat) -> Result<BindingElement, LowerError> {
    let elements: Vec<BindingElement> = arr.elems.iter()
        .filter_map(|e| {
            match e {
                Some(elem) => lower_elem_pat(elem),
                None => Some(BindingElement::Identifier("__hole".to_string())),
            }
        })
        .collect();
    Ok(BindingElement::ArrayPattern(elements))
}

fn lower_elem_pat(elem: &swc::Pat) -> Option<BindingElement> {
    match elem {
        swc::Pat::Ident(id) => {
            Some(BindingElement::Identifier(atom_to_string(&id.id.sym)))
        }
        swc::Pat::Array(arr) => lower_binding_elem(&swc::Pat::Array(arr.clone())).ok(),
        swc::Pat::Object(obj) => lower_binding_elem(&swc::Pat::Object(obj.clone())).ok(),
        swc::Pat::Rest(rest) => lower_binding_elem(&rest.arg).ok(),
        swc::Pat::Assign(assign) => lower_binding_elem(&assign.left).ok(),
        _ => None,
    }
}

fn lower_object_pattern(obj: &swc::ObjectPat) -> Result<BindingElement, LowerError> {
    let props: Vec<(PropertyKey, BindingElement)> = obj.props.iter()
        .filter_map(lower_object_pat_prop)
        .collect();
    Ok(BindingElement::ObjectPattern(props))
}

fn lower_object_pat_prop(prop: &swc::ObjectPatProp) -> Option<(PropertyKey, BindingElement)> {
    match prop {
        swc::ObjectPatProp::KeyValue(kv) => {
            let key = lower_prop_name_key(&kv.key)?;
            let value = lower_binding_elem(&kv.value).ok()?;
            Some((key, value))
        }
        swc::ObjectPatProp::Assign(assign) => {
            let key = PropertyKey::Ident(atom_to_string(&assign.key.sym));
            let elem = BindingElement::Identifier(atom_to_string(&assign.key.sym));
            Some((key, elem))
        }
        swc::ObjectPatProp::Rest(_) => None,
    }
}

fn lower_prop_name_key(key: &swc::PropName) -> Option<PropertyKey> {
    match key {
        swc::PropName::Ident(i) => Some(PropertyKey::Ident(atom_to_string(&i.sym))),
        swc::PropName::Str(s) => Some(PropertyKey::String(wtf8_atom_to_string(&s.value))),
        swc::PropName::Num(n) => Some(PropertyKey::Number(n.value)),
        swc::PropName::Computed(_) => Some(PropertyKey::String("computed".to_string())),
        swc::PropName::BigInt(_) => Some(PropertyKey::String("bigint".to_string())),
    }
}

/// Expand a nested binding pattern into variable declarations
pub fn expand_nested_pattern(
    kind: VarKind,
    pat: &swc::Pat,
    source_var: &str,
) -> Vec<Statement> {
    let source = Expression::Identifier(source_var.to_string());
    match pat {
        swc::Pat::Ident(ident) => {
            vec![Statement::VarDeclaration {
                kind,
                name: atom_to_string(&ident.id.sym),
                init: Some(source),
            }]
        }
        swc::Pat::Array(arr) => expand_nested_array_pattern(kind, arr, source_var),
        swc::Pat::Object(obj) => expand_nested_object_pattern(kind, obj, source_var),
        _ => vec![],
    }
}

/// Expand array pattern: [a, b] from source_var
pub fn expand_nested_array_pattern(
    kind: VarKind,
    arr: &swc::ArrayPat,
    source_var: &str,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    for (i, elem) in arr.elems.iter().enumerate() {
        if let Some(elem) = elem {
            let member = array_member_expr(source_var, i);
            match elem {
                swc::Pat::Ident(id) => {
                    stmts.push(Statement::VarDeclaration {
                        kind,
                        name: atom_to_string(&id.id.sym),
                        init: Some(member),
                    });
                }
                _ => {
                    let temp_name = format!("{}_item_{}", source_var, i);
                    stmts.push(Statement::VarDeclaration {
                        kind,
                        name: temp_name.clone(),
                        init: Some(member),
                    });
                    stmts.extend(expand_nested_pattern(kind, elem, &temp_name));
                }
            }
        }
    }
    stmts
}

fn array_member_expr(source_var: &str, index: usize) -> Expression {
    Expression::Member {
        object: Box::new(Expression::Identifier(source_var.to_string())),
        property: PropertyKey::Number(index as f64),
        computed: true,
    }
}

/// Expand object pattern: {a, b} from source_var
pub fn expand_nested_object_pattern(
    kind: VarKind,
    obj: &swc::ObjectPat,
    source_var: &str,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    for prop in &obj.props {
        match prop {
            swc::ObjectPatProp::KeyValue(kv) => {
                let key_str = match &kv.key {
                    swc::PropName::Ident(i) => atom_to_string(&i.sym),
                    swc::PropName::Str(s) => wtf8_atom_to_string(&s.value),
                    swc::PropName::Num(n) => n.value.to_string(),
                    _ => continue,
                };
                if key_str.is_empty() {
                    continue;
                }
                let kv_value_ref: &swc::Pat = &kv.value;
                let var_name = match kv_value_ref {
                    swc::Pat::Ident(id) => atom_to_string(&id.id.sym),
                    _ => format!("{}_prop_{}", source_var, key_str),
                };
                let member = object_member_expr(source_var, &key_str);
                add_object_kv_stmts(kind, kv_value_ref, var_name, member, source_var, key_str, &mut stmts);
            }
            swc::ObjectPatProp::Assign(assign) => {
                let var_name = atom_to_string(&assign.key.sym);
                let member = object_member_expr(source_var, &var_name);
                stmts.push(Statement::VarDeclaration {
                    kind,
                    name: var_name,
                    init: Some(member),
                });
            }
            swc::ObjectPatProp::Rest(_) => {}
        }
    }
    stmts
}

fn add_object_kv_stmts(
    kind: VarKind,
    kv_value_ref: &swc::Pat,
    var_name: String,
    member: Expression,
    source_var: &str,
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
            let nested_temp_name = format!("{}_prop_{}", source_var, key_str);
            stmts.push(Statement::VarDeclaration {
                kind: VarKind::Const,
                name: nested_temp_name.clone(),
                init: Some(member),
            });
            stmts.extend(expand_nested_object_pattern(kind, nested_obj, &nested_temp_name));
        }
        swc::Pat::Array(nested_arr) => {
            let nested_temp_name = format!("{}_prop_{}", source_var, key_str);
            stmts.push(Statement::VarDeclaration {
                kind: VarKind::Const,
                name: nested_temp_name.clone(),
                init: Some(member),
            });
            stmts.extend(expand_nested_array_pattern(kind, nested_arr, &nested_temp_name));
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

fn object_member_expr(source_var: &str, key: &str) -> Expression {
    Expression::Member {
        object: Box::new(Expression::Identifier(source_var.to_string())),
        property: PropertyKey::String(key.to_string()),
        computed: false,
    }
}
