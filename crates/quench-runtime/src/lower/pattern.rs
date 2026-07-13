//! Pattern lowering - destructuring and binding patterns

use super::expr::lower_expr;
use super::helpers::LowerError;
use crate::ast::{BindingElement, Expression, PropertyKey, Statement, VarKind};
use crate::lower::expr::lower_assignment_target;
use oxc::ast::ast;

/// Convert a BindingElement to an Expression for use in for-in/for-of loop headers
pub fn binding_to_expr(binding: BindingElement) -> Expression {
    match binding {
        BindingElement::Identifier(name) => Expression::Identifier(name),
        BindingElement::ArrayPattern(elements) => Expression::ArrayPattern(elements),
        BindingElement::ObjectPattern(props) => Expression::ObjectPattern(props),
        BindingElement::Default(binding, _) => binding_to_expr(*binding),
        BindingElement::AssignmentTarget(expr) => expr,
    }
}

/// Lower a binding pattern (for destructuring) to BindingElement
pub fn lower_binding_elem(pat: &ast::BindingPattern) -> Result<BindingElement, LowerError> {
    match &pat.kind {
        ast::BindingPatternKind::BindingIdentifier(ident) => {
            Ok(BindingElement::Identifier(ident.name.as_str().to_string()))
        }
        ast::BindingPatternKind::ArrayPattern(arr) => lower_array_pattern(arr),
        ast::BindingPatternKind::ObjectPattern(obj) => lower_object_pattern(obj),
        ast::BindingPatternKind::AssignmentPattern(assign) => Ok(BindingElement::Default(
            Box::new(lower_binding_elem(&assign.left)?),
            Box::new(lower_expr(&assign.right)?),
        )),
    }
}

fn lower_array_pattern(arr: &ast::ArrayPattern) -> Result<BindingElement, LowerError> {
    let mut elements: Vec<BindingElement> = arr
        .elements
        .iter()
        .filter_map(|e| e.as_ref().and_then(lower_elem_pat))
        .collect();

    // Handle trailing rest element
    if let Some(rest) = &arr.rest {
        elements.push(lower_binding_elem(&rest.argument)?);
    }

    Ok(BindingElement::ArrayPattern(elements))
}

pub fn lower_elem_pat(elem: &ast::BindingPattern) -> Option<BindingElement> {
    lower_binding_elem(elem).ok()
}

fn lower_object_pattern(obj: &ast::ObjectPattern) -> Result<BindingElement, LowerError> {
    let mut props: Vec<(PropertyKey, BindingElement)> = obj
        .properties
        .iter()
        .filter_map(lower_object_pat_prop)
        .collect();

    // Handle rest element
    if let Some(rest) = &obj.rest {
        let rest_elem = lower_binding_elem(&rest.argument)
            .map_err(|_| LowerError::new("Invalid rest binding"))?;
        let rest_key = PropertyKey::Ident("...".to_string());
        props.push((rest_key, rest_elem));
    }

    Ok(BindingElement::ObjectPattern(props))
}

pub fn lower_object_pat_prop(prop: &ast::BindingProperty) -> Option<(PropertyKey, BindingElement)> {
    let key = lower_prop_name_key(&prop.key)?;
    let value = lower_binding_elem(&prop.value).ok()?;
    Some((key, value))
}

/// Lower an AssignmentTargetProperty (used in object assignment targets like `for ({a} in x)`)
pub fn lower_assignment_target_prop(
    prop: &ast::AssignmentTargetProperty,
) -> Option<(PropertyKey, BindingElement)> {
    match prop {
        ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
            let key = PropertyKey::Ident(id.binding.name.as_str().to_string());
            let value = BindingElement::Identifier(id.binding.name.as_str().to_string());
            Some((key, value))
        }
        ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(prop) => {
            let key = lower_property_key(&prop.name)?;
            let value = lower_assignment_target_maybe_default(&prop.binding)?;
            Some((key, value))
        }
    }
}

/// Convert an AssignmentTarget to a BindingElement for use in for-in/for-of
pub fn lower_assignment_target_to_binding(
    target: &ast::AssignmentTarget,
) -> Option<BindingElement> {
    match target {
        // Simple identifier: `a` in `for (a in x)` → Identifier
        ast::AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            Some(BindingElement::Identifier(ident.name.as_str().to_string()))
        }
        // Object destructuring: `for ({a} in x)` → ObjectPattern
        ast::AssignmentTarget::ObjectAssignmentTarget(obj) => {
            lower_object_assignment_target(obj).ok()
        }
        // Array destructuring: `for ([a, b] in x)` → ArrayPattern
        ast::AssignmentTarget::ArrayAssignmentTarget(arr) => {
            lower_array_assignment_target(arr).ok()
        }
        // Member expression: `for (obj.prop in x)` → Member expression (not a binding)
        ast::AssignmentTarget::StaticMemberExpression(_) => None,
        ast::AssignmentTarget::ComputedMemberExpression(_) => None,
        ast::AssignmentTarget::PrivateFieldExpression(_) => None,
        // TS type assertions — unwrap and convert the inner expression
        ast::AssignmentTarget::TSAsExpression(e) => expr_to_binding_elem(&e.expression),
        ast::AssignmentTarget::TSSatisfiesExpression(e) => expr_to_binding_elem(&e.expression),
        ast::AssignmentTarget::TSNonNullExpression(e) => expr_to_binding_elem(&e.expression),
        ast::AssignmentTarget::TSTypeAssertion(e) => expr_to_binding_elem(&e.expression),
        ast::AssignmentTarget::TSInstantiationExpression(e) => expr_to_binding_elem(&e.expression),
    }
}

/// Convert an Expression to a BindingElement, unwrapping TS type wrappers
fn expr_to_binding_elem(expr: &ast::Expression) -> Option<BindingElement> {
    match expr {
        // Identifier is a valid binding
        ast::Expression::Identifier(ident) => {
            Some(BindingElement::Identifier(ident.name.as_str().to_string()))
        }
        // Recursively unwrap TS type assertions
        ast::Expression::TSAsExpression(e) => expr_to_binding_elem(&e.expression),
        ast::Expression::TSSatisfiesExpression(e) => expr_to_binding_elem(&e.expression),
        ast::Expression::TSNonNullExpression(e) => expr_to_binding_elem(&e.expression),
        ast::Expression::TSTypeAssertion(e) => expr_to_binding_elem(&e.expression),
        ast::Expression::TSInstantiationExpression(e) => expr_to_binding_elem(&e.expression),
        _ => None,
    }
}

/// Lower an ObjectAssignmentTarget to BindingElement
pub fn lower_object_assignment_target(
    obj: &ast::ObjectAssignmentTarget,
) -> Result<BindingElement, LowerError> {
    let props: Vec<(PropertyKey, BindingElement)> = obj
        .properties
        .iter()
        .filter_map(|p| lower_assignment_target_prop(p))
        .collect();
    Ok(BindingElement::ObjectPattern(props))
}

/// Lower an ArrayAssignmentTarget to BindingElement
pub fn lower_array_assignment_target(
    arr: &ast::ArrayAssignmentTarget,
) -> Result<BindingElement, LowerError> {
    let mut elements: Vec<BindingElement> = Vec::new();
    for elem in &arr.elements {
        let elem_binding = match elem {
            Some(ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d)) => {
                let binding = match lower_assignment_target_to_binding(&d.binding) {
                    Some(b) => b,
                    None => continue,
                };
                let init = lower_expr(&d.init)?;
                Some(BindingElement::Default(Box::new(binding), Box::new(init)))
            }
            Some(elem) => {
                if let Some(target) = elem.as_assignment_target() {
                    lower_assignment_target_to_binding(target)
                } else {
                    Some(BindingElement::Identifier("__hole".to_string()))
                }
            }
            None => Some(BindingElement::Identifier("__hole".to_string())),
        };
        if let Some(binding) = elem_binding {
            elements.push(binding);
        }
    }
    // Handle rest element: `for ([...rest] in x)` — rest.target is AssignmentTarget
    if let Some(rest) = &arr.rest {
        if let Some(binding) = lower_assignment_target_to_binding(&rest.target) {
            elements.push(binding);
        }
    }
    Ok(BindingElement::ArrayPattern(elements))
}

/// Lower AssignmentTargetMaybeDefault (handles default values in array destructuring)
fn lower_assignment_target_maybe_default(
    target: &ast::AssignmentTargetMaybeDefault,
) -> Option<BindingElement> {
    match target {
        // `[a = default]`
        ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
            let binding = lower_assignment_target_to_binding(&d.binding)?;
            let init = lower_expr(&d.init).ok()?;
            Some(BindingElement::Default(Box::new(binding), Box::new(init)))
        }
        // Regular assignment target
        _ => {
            if let Some(target) = target.as_assignment_target() {
                if matches!(target, ast::AssignmentTarget::AssignmentTargetIdentifier(_)) {
                    lower_assignment_target_to_binding(target)
                } else {
                    let expression = lower_assignment_target(target).ok()?;
                    Some(BindingElement::AssignmentTarget(expression))
                }
            } else {
                None
            }
        }
    }
}

/// Lower an OXC PropertyKey to our PropertyKey
fn lower_property_key(key: &ast::PropertyKey) -> Option<PropertyKey> {
    match key {
        ast::PropertyKey::StaticIdentifier(i) => {
            Some(PropertyKey::Ident(i.name.as_str().to_string()))
        }
        ast::PropertyKey::PrivateIdentifier(i) => Some(PropertyKey::Ident(format!("#{}", i.name))),
        ast::PropertyKey::StringLiteral(s) => Some(PropertyKey::String(s.value.to_string())),
        ast::PropertyKey::NumericLiteral(n) => Some(PropertyKey::Number(n.value)),
        ast::PropertyKey::BigIntLiteral(b) => Some(PropertyKey::String(b.raw.to_string())),
        ast::PropertyKey::BooleanLiteral(b) => Some(PropertyKey::String(b.value.to_string())),
        ast::PropertyKey::NullLiteral(_) => Some(PropertyKey::String("null".to_string())),
        _ => None,
    }
}

fn lower_prop_name_key(key: &ast::PropertyKey) -> Option<PropertyKey> {
    match key {
        ast::PropertyKey::StaticIdentifier(i) => {
            Some(PropertyKey::Ident(i.name.as_str().to_string()))
        }
        ast::PropertyKey::PrivateIdentifier(i) => Some(PropertyKey::Ident(format!("#{}", i.name))),
        ast::PropertyKey::StringLiteral(s) => Some(PropertyKey::String(s.value.to_string())),
        ast::PropertyKey::NumericLiteral(n) => Some(PropertyKey::Number(n.value)),
        ast::PropertyKey::BigIntLiteral(b) => Some(PropertyKey::String(b.raw.to_string())),
        ast::PropertyKey::BooleanLiteral(b) => Some(PropertyKey::String(b.value.to_string())),
        ast::PropertyKey::NullLiteral(_) => Some(PropertyKey::String("null".to_string())),
        ast::PropertyKey::TemplateLiteral(_) => None,
        // Computed property names are handled via expression variants
        _ => {
            // Check if it's an identifier (static key) or other expression (computed key)
            if let ast::PropertyKey::StaticIdentifier(i) = key {
                Some(PropertyKey::Ident(i.name.as_str().to_string()))
            } else if let ast::PropertyKey::PrivateIdentifier(i) = key {
                Some(PropertyKey::Ident(format!("#{}", i.name)))
            } else if let ast::PropertyKey::StringLiteral(s) = key {
                Some(PropertyKey::String(s.value.to_string()))
            } else if let ast::PropertyKey::NumericLiteral(n) = key {
                Some(PropertyKey::Number(n.value))
            } else if let ast::PropertyKey::BigIntLiteral(b) = key {
                Some(PropertyKey::String(b.raw.to_string()))
            } else if let ast::PropertyKey::BooleanLiteral(b) = key {
                Some(PropertyKey::String(b.value.to_string()))
            } else if let ast::PropertyKey::NullLiteral(_) = key {
                Some(PropertyKey::String("null".to_string()))
            } else {
                None // Computed keys need special handling and can't be represented as static
            }
        }
    }
}

/// Expand a nested binding pattern into variable declarations
pub fn expand_nested_pattern(
    kind: VarKind,
    pat: &ast::BindingPattern,
    source_var: &str,
) -> Vec<Statement> {
    let source = Expression::Identifier(source_var.to_string());
    match &pat.kind {
        ast::BindingPatternKind::BindingIdentifier(ident) => {
            vec![Statement::VarDeclaration {
                kind,
                name: ident.name.as_str().to_string(),
                init: Some(source),
            }]
        }
        ast::BindingPatternKind::ArrayPattern(arr) => {
            expand_nested_array_pattern(kind, arr, source_var)
        }
        ast::BindingPatternKind::ObjectPattern(obj) => {
            expand_nested_object_pattern(kind, obj, source_var)
        }
        ast::BindingPatternKind::AssignmentPattern(assign) => {
            expand_nested_pattern(kind, &assign.left, source_var)
        }
    }
}

/// Expand array pattern: [a, b] from source_var
pub fn expand_nested_array_pattern(
    kind: VarKind,
    arr: &ast::ArrayPattern,
    source_var: &str,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    for (i, elem) in arr.elements.iter().enumerate() {
        if let Some(elem) = elem {
            let member = array_member_expr(source_var, i);
            match &elem.kind {
                ast::BindingPatternKind::BindingIdentifier(id) => {
                    stmts.push(Statement::VarDeclaration {
                        kind,
                        name: id.name.as_str().to_string(),
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
    // Handle trailing rest element
    if let Some(rest) = &arr.rest {
        let rest_temp_name = format!("{}_rest", source_var);
        stmts.push(Statement::VarDeclaration {
            kind: VarKind::Const,
            name: rest_temp_name.clone(),
            init: Some(Expression::Identifier(source_var.to_string())),
        });
        stmts.extend(expand_nested_pattern(kind, &rest.argument, &rest_temp_name));
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
    obj: &ast::ObjectPattern,
    source_var: &str,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    for prop in &obj.properties {
        let key_str = match &prop.key {
            ast::PropertyKey::Identifier(i) => i.name.as_str().to_string(),
            ast::PropertyKey::StringLiteral(s) => s.value.to_string(),
            ast::PropertyKey::NumericLiteral(n) => n.value.to_string(),
            _ => continue,
        };
        if key_str.is_empty() {
            continue;
        }

        let var_name = match &prop.value.kind {
            ast::BindingPatternKind::BindingIdentifier(id) => id.name.as_str().to_string(),
            _ => format!("{}_prop_{}", source_var, key_str),
        };
        let member = object_member_expr(source_var, &key_str);
        add_object_kv_stmts(
            kind,
            &prop.value,
            var_name,
            member,
            source_var,
            key_str,
            &mut stmts,
        );
    }

    // Handle rest element
    if let Some(rest) = &obj.rest {
        let rest_temp_name = format!("{}_rest", source_var);
        stmts.push(Statement::VarDeclaration {
            kind: VarKind::Const,
            name: rest_temp_name.clone(),
            init: Some(Expression::Identifier(source_var.to_string())),
        });
        stmts.extend(expand_nested_pattern(kind, &rest.argument, &rest_temp_name));
    }

    stmts
}

fn add_object_kv_stmts(
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

fn object_member_expr(source_var: &str, key: &str) -> Expression {
    Expression::Member {
        object: Box::new(Expression::Identifier(source_var.to_string())),
        property: PropertyKey::String(key.to_string()),
        computed: false,
    }
}
