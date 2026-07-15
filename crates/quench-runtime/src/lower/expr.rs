//! Expression lowering - convert OXC expressions to runtime AST expressions

use super::helpers::LowerError;
use super::helpers::{assign_op_to_bin, lower_bin_op, lower_logical_op, lower_unary_op};
use super::jsx::{lower_jsx_element, lower_jsx_fragment, lower_jsx_member, lower_jsx_namespaced};
use super::literals::{lower_tagged_template, lower_template_literal};
use super::opt_chain::lower_opt_chain;
use super::stmt::lower_formal_params;
use crate::ast::Statement;
use crate::ast::{
    ArrowBody, Class, ClassMember, Expression, Param, PropertyKey, PropertyValue, UpdateOp,
};
use oxc::ast::ast;
use oxc::syntax::operator::{AssignmentOperator, LogicalOperator, UpdateOperator};

/// Lower an OXC Expression to our Expression
#[allow(clippy::complexity)]
pub fn lower_expr(expr: &ast::Expression) -> Result<Expression, LowerError> {
    match expr {
        ast::Expression::Identifier(ident) => {
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::Expression::ThisExpression(_) => Ok(Expression::Identifier("this".to_string())),
        ast::Expression::ArrayExpression(arr) => lower_array_expr(arr),
        ast::Expression::ObjectExpression(obj) => lower_object_expr(obj),
        ast::Expression::FunctionExpression(func) => lower_fn_expr(func),
        ast::Expression::ArrowFunctionExpression(arrow) => lower_arrow_expr(arrow),
        ast::Expression::YieldExpression(yield_expr) => lower_yield_expr(yield_expr),
        ast::Expression::MetaProperty(meta) => {
            // `new.target` — runtime resolves via GetNewTarget.
            if meta.meta.name == "new" && meta.property.name == "target" {
                Ok(Expression::Identifier("new.target".to_string()))
            } else {
                // `import.meta` and other meta properties — not supported.
                Ok(Expression::Undefined)
            }
        }
        ast::Expression::AwaitExpression(await_expr) => lower_expr(&await_expr.argument),
        ast::Expression::ParenthesizedExpression(paren) => lower_expr(&paren.expression),
        ast::Expression::BinaryExpression(bin) => lower_bin_expr(bin),
        ast::Expression::LogicalExpression(logical) => lower_logical_expr(logical),
        ast::Expression::UnaryExpression(unary) => lower_unary_expr(unary),
        ast::Expression::UpdateExpression(update) => lower_update_expr(update),
        ast::Expression::AssignmentExpression(assign) => lower_assign_expr(assign),
        ast::Expression::StaticMemberExpression(member) => lower_static_member_expr(member),
        ast::Expression::ComputedMemberExpression(member) => lower_computed_member_expr(member),
        ast::Expression::PrivateFieldExpression(member) => lower_private_field_expr(member),
        ast::Expression::Super(_) => Ok(Expression::Undefined),
        ast::Expression::CallExpression(call) => lower_call_expr(call),
        ast::Expression::NewExpression(new_expr) => lower_new_expr(new_expr),
        ast::Expression::SequenceExpression(seq) => lower_seq_expr(seq),
        ast::Expression::ConditionalExpression(cond) => lower_cond_expr(cond),
        ast::Expression::ChainExpression(chain) => lower_opt_chain(chain),
        ast::Expression::StringLiteral(s) => Ok(Expression::String(s.value.to_string())),
        ast::Expression::NumericLiteral(n) => Ok(Expression::Number(n.value)),
        ast::Expression::BooleanLiteral(b) => Ok(Expression::Boolean(b.value)),
        ast::Expression::NullLiteral(_) => Ok(Expression::Null),
        ast::Expression::RegExpLiteral(r) => Ok(Expression::RegExp {
            pattern: r.regex.pattern.to_string(),
            flags: r.regex.flags.to_string(),
        }),
        ast::Expression::BigIntLiteral(_) => Ok(Expression::Undefined),
        ast::Expression::TaggedTemplateExpression(tagged) => lower_tagged_template(tagged),
        ast::Expression::TemplateLiteral(tpl) => lower_template_literal(tpl),
        ast::Expression::ClassExpression(class_expr) => lower_class_expr(class_expr),
        ast::Expression::JSXElement(elem) => lower_jsx_element(elem),
        ast::Expression::JSXFragment(frag) => lower_jsx_fragment(frag),
        ast::Expression::TSAsExpression(e) => lower_expr(&e.expression),
        ast::Expression::TSSatisfiesExpression(e) => lower_expr(&e.expression),
        ast::Expression::TSTypeAssertion(e) => lower_expr(&e.expression),
        ast::Expression::TSNonNullExpression(e) => lower_expr(&e.expression),
        ast::Expression::TSInstantiationExpression(e) => lower_expr(&e.expression),
        _ => Ok(Expression::Undefined),
    }
}

fn lower_array_expr(arr: &ast::ArrayExpression) -> Result<Expression, LowerError> {
    let elements: Vec<Expression> = arr
        .elements
        .iter()
        .map(|elem| {
            match elem {
                ast::ArrayExpressionElement::SpreadElement(spread) => {
                    Ok(Expression::Spread(Box::new(lower_expr(&spread.argument)?)))
                }
                ast::ArrayExpressionElement::Elision(_) => Ok(Expression::Elision),
                // Use as_expression() to convert the boxed variant to Expression
                elem => lower_expr(
                    elem.as_expression()
                        .ok_or(LowerError::new("Invalid array element"))?,
                ),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Expression::Array(elements))
}

fn lower_object_expr(obj: &ast::ObjectExpression) -> Result<Expression, LowerError> {
    let ast_props: Vec<_> = obj.properties.iter().collect();
    let mut result: Vec<(PropertyKey, PropertyValue)> = Vec::new();
    let mut i = 0;

    while i < ast_props.len() {
        let prop = match &ast_props[i] {
            ast::ObjectPropertyKind::ObjectProperty(p) => p,
            ast::ObjectPropertyKind::SpreadProperty(_) => {
                i += 1;
                continue;
            }
        };

        // Check if it's a getter or setter (OXC recognizes these properly)
        if prop.kind == ast::PropertyKind::Get || prop.kind == ast::PropertyKind::Set {
            let (lowered_key, lowered_val) = lower_object_prop(prop)?;
            result.push((lowered_key, lowered_val));
            i += 1;
            continue;
        }

        // Fallback: OXC 0.47 misclassifies `set`/`get` accessors in object
        // literals as `PropertyKind::Init`. Detect by the property key being
        // "get"/"set" and the value being a function expression. When matched,
        // peek ahead to find the actual property name in the next property.
        let key_name: Option<String> = match &prop.key {
            ast::PropertyKey::StaticIdentifier(name) => Some(name.name.to_string()),
            _ => None,
        };

        if let (Some(name), ast::Expression::FunctionExpression(func)) =
            (key_name.as_deref(), &prop.value)
        {
            if name == "get" || name == "set" {
                // Peek ahead: the next property holds the actual property name
                let actual_key = if i + 1 < ast_props.len() {
                    if let ast::ObjectPropertyKind::ObjectProperty(next_prop) = &ast_props[i + 1] {
                        // Use the next property's key as the actual property name
                        let next_key = lower_prop_name_key_oxc(&next_prop.key)?;
                        // Skip the next property (it's the actual { x: ... } part)
                        i += 1;
                        next_key
                    } else {
                        // No next property, use "get"/"set" as the literal property name
                        PropertyKey::Ident(name.to_string())
                    }
                } else {
                    PropertyKey::Ident(name.to_string())
                };

                let lowered_val = if name == "get" {
                    let body = func
                        .body
                        .as_ref()
                        .map(|b| super::helpers::lower_fn_body(b))
                        .unwrap_or_default();
                    PropertyValue::Getter {
                        params: vec![],
                        body,
                    }
                } else {
                    let param = func
                        .params
                        .items
                        .first()
                        .and_then(|p| {
                            if let ast::BindingPatternKind::BindingIdentifier(ident) = &p.pattern.kind {
                                Some(ident.name.as_str().to_string())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "value".to_string());
                    let body = func
                        .body
                        .as_ref()
                        .map(|b| super::helpers::lower_fn_body(b))
                        .unwrap_or_default();
                    PropertyValue::Setter { param, body }
                };
                result.push((actual_key, lowered_val));
                i += 1;
                continue;
            }
        }

        // Regular property (not a get/set accessor keyword)
        let (lowered_key, lowered_val) = lower_object_prop(prop)?;
        result.push((lowered_key, lowered_val));
        i += 1;
    }

    Ok(Expression::Object(result))
}

fn lower_object_prop(
    prop: &ast::ObjectProperty,
) -> Result<(PropertyKey, PropertyValue), LowerError> {
    let key = lower_prop_name_key_oxc(&prop.key)?;

    // Check if it's a getter or setter (OXC-recognised)
    if prop.kind == ast::PropertyKind::Get {
        let body = if let ast::Expression::FunctionExpression(func) = &prop.value {
            func.body
                .as_ref()
                .map(|b| super::helpers::lower_fn_body(b))
                .unwrap_or_default()
        } else {
            vec![]
        };
        return Ok((
            key,
            PropertyValue::Getter {
                params: vec![],
                body,
            },
        ));
    }

    if prop.kind == ast::PropertyKind::Set {
        let param = match &prop.value {
            ast::Expression::FunctionExpression(func) => func
                .params
                .items
                .first()
                .and_then(|p| {
                    if let ast::BindingPatternKind::BindingIdentifier(ident) = &p.pattern.kind {
                        Some(ident.name.as_str().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "value".to_string()),
            _ => "value".to_string(),
        };
        let body = match &prop.value {
            ast::Expression::FunctionExpression(func) => func
                .body
                .as_ref()
                .map(|b| super::helpers::lower_fn_body(b))
                .unwrap_or_default(),
            ast::Expression::ArrowFunctionExpression(arrow) => {
                if arrow.expression {
                    vec![]
                } else {
                    let stmts = arrow
                        .body
                        .statements
                        .iter()
                        .filter_map(super::stmt::lower_stmt)
                        .collect();
                    stmts
                }
            }
            _ => vec![],
        };
        return Ok((key, PropertyValue::Setter { param, body }));
    }

    // Check if it's a method (shorthand method like { foo() {} })
    if prop.method {
        return lower_method_prop_from_value(&prop.key, &prop.value);
    }

    // Regular property
    let value = lower_expr(&prop.value)?;
    Ok((key, PropertyValue::Value(value)))
}

fn lower_method_prop_from_value(
    key: &ast::PropertyKey,
    value: &ast::Expression,
) -> Result<(PropertyKey, PropertyValue), LowerError> {
    let key = lower_prop_name_key_oxc(key)?;
    if let ast::Expression::FunctionExpression(func) = value {
        let params = lower_formal_params(&func.params);
        let body = func
            .body
            .as_ref()
            .map(|b| super::helpers::lower_fn_body(b))
            .unwrap_or_default();
        Ok((
            key,
            PropertyValue::Value(Expression::FunctionExpression {
                name: None,
                params,
                body,
            }),
        ))
    } else {
        let value = lower_expr(value)?;
        Ok((key, PropertyValue::Value(value)))
    }
}

fn lower_fn_expr(func: &ast::Function) -> Result<Expression, LowerError> {
    let name = func.id.as_ref().map(|i| i.name.as_str().to_string());
    let params = lower_formal_params(&func.params);
    let body = func
        .body
        .as_ref()
        .map(|b| super::helpers::lower_fn_body(b))
        .unwrap_or_default();
    Ok(Expression::FunctionExpression { name, params, body })
}

fn lower_arrow_expr(arrow: &ast::ArrowFunctionExpression) -> Result<Expression, LowerError> {
    let params = lower_formal_params(&arrow.params);
    // In OXC, arrow.body is always a FunctionBody
    // If arrow.expression is true, it's an expression body (implicit return)
    // OXC stores expression bodies as a single Expression statement (not Return)
    let body = if arrow.expression {
        // Expression body
        let stmts = arrow
            .body
            .statements
            .iter()
            .filter_map(super::stmt::lower_stmt)
            .collect::<Vec<_>>();
        if stmts.len() == 1 {
            match &stmts[0] {
                // OXC stores expression body as Statement::Expression, not Return
                Statement::Expression(expr) => ArrowBody::Expression(*expr.clone()),
                // Fallback: might be Return if some cases produce it
                Statement::Return(Some(expr)) => ArrowBody::Expression(*expr.clone()),
                _ => ArrowBody::Block(std::rc::Rc::new(stmts)),
            }
        } else {
            ArrowBody::Block(std::rc::Rc::new(stmts))
        }
    } else {
        // Block body
        ArrowBody::Block(std::rc::Rc::new(super::helpers::lower_fn_body(&arrow.body)))
    };
    Ok(Expression::ArrowFunction {
        params,
        body: Box::new(body),
    })
}

fn lower_yield_expr(yield_expr: &ast::YieldExpression) -> Result<Expression, LowerError> {
    if yield_expr.delegate {
        return Err(LowerError::new("Yield delegate not supported"));
    }
    match &yield_expr.argument {
        Some(expr) => lower_expr(expr),
        None => Ok(Expression::Undefined),
    }
}

fn lower_bin_expr(bin: &ast::BinaryExpression) -> Result<Expression, LowerError> {
    let left = lower_expr(&bin.left)?;
    let right = lower_expr(&bin.right)?;
    let op = lower_bin_op(&bin.operator)?;
    Ok(Expression::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn lower_logical_expr(logical: &ast::LogicalExpression) -> Result<Expression, LowerError> {
    let left = lower_expr(&logical.left)?;
    let right = lower_expr(&logical.right)?;
    let op = lower_logical_op(&logical.operator)?;
    Ok(Expression::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn lower_unary_expr(unary: &ast::UnaryExpression) -> Result<Expression, LowerError> {
    let arg = lower_expr(&unary.argument)?;
    let op = lower_unary_op(&unary.operator)?;
    Ok(Expression::Unary {
        op,
        argument: Box::new(arg),
    })
}

fn lower_update_expr(update: &ast::UpdateExpression) -> Result<Expression, LowerError> {
    // In OXC, update.argument is SimpleAssignmentTarget, convert it
    let arg = lower_simple_assignment_target(&update.argument)?;
    let op = if update.operator == UpdateOperator::Increment {
        UpdateOp::Increment
    } else {
        UpdateOp::Decrement
    };
    Ok(Expression::Update {
        op,
        argument: Box::new(arg),
        prefix: update.prefix,
    })
}

/// Check if an assign op is a logical compound assignment (||=, &&=, ??=)
fn is_logical_compound_op(op: &AssignmentOperator) -> bool {
    matches!(
        op,
        AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalNullish
    )
}

fn lower_assign_expr(assign: &ast::AssignmentExpression) -> Result<Expression, LowerError> {
    let left = lower_assignment_target(&assign.left)?;
    let right = lower_expr(&assign.right)?;
    if assign.operator == AssignmentOperator::Assign {
        Ok(Expression::Assignment {
            left: Box::new(left),
            right: Box::new(right),
        })
    } else if is_logical_compound_op(&assign.operator) {
        let comp_op = assign_op_to_bin(&assign.operator)?;
        Ok(Expression::LogicalCompoundAssignment {
            op: comp_op,
            left: Box::new(left),
            right: Box::new(right),
        })
    } else {
        let bin_op = assign_op_to_bin(&assign.operator)?;
        Ok(Expression::CompoundAssignment {
            op: bin_op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }
}

fn lower_static_member_expr(
    member: &ast::StaticMemberExpression,
) -> Result<Expression, LowerError> {
    // `super.x` — runtime dispatches to SuperMember.
    if matches!(member.object, ast::Expression::Super(_)) {
        let property = PropertyKey::Ident(member.property.name.as_str().to_string());
        return Ok(Expression::Member {
            object: Box::new(Expression::Identifier("super".to_string())),
            property,
            computed: false,
        });
    }
    let obj = lower_expr(&member.object)?;
    let property = PropertyKey::Ident(member.property.name.as_str().to_string());
    Ok(Expression::Member {
        object: Box::new(obj),
        property,
        computed: false,
    })
}

fn lower_computed_member_expr(
    member: &ast::ComputedMemberExpression,
) -> Result<Expression, LowerError> {
    let obj = lower_expr(&member.object)?;
    let property = PropertyKey::Computed(Box::new(lower_expr(&member.expression)?));
    Ok(Expression::Member {
        object: Box::new(obj),
        property,
        computed: true,
    })
}

fn lower_private_field_expr(
    member: &ast::PrivateFieldExpression,
) -> Result<Expression, LowerError> {
    let obj = lower_expr(&member.object)?;
    let property = PropertyKey::Ident(member.field.name.as_str().to_string());
    Ok(Expression::Member {
        object: Box::new(obj),
        property,
        computed: false,
    })
}

fn lower_call_expr(call: &ast::CallExpression) -> Result<Expression, LowerError> {
    let callee = match &call.callee {
        ast::Expression::ImportExpression(_) => {
            return Err(LowerError::new("import() not supported"));
        }
        // `super(args)` — runtime dispatches to SuperCall.
        ast::Expression::Super(_) => Expression::Identifier("super".to_string()),
        _ => lower_expr(&call.callee)?,
    };
    let mut args = Vec::new();
    for arg in &call.arguments {
        let expr = match arg {
            ast::Argument::SpreadElement(spread) => {
                Expression::Spread(Box::new(lower_expr(&spread.argument)?))
            }
            // Use as_expression() to convert boxed variant to Expression
            arg => lower_expr(
                arg.as_expression()
                    .ok_or(LowerError::new("Invalid argument"))?,
            )?,
        };
        args.push(expr);
    }
    Ok(Expression::Call {
        callee: Box::new(callee),
        arguments: args,
    })
}

fn lower_new_expr(new_expr: &ast::NewExpression) -> Result<Expression, LowerError> {
    let constructor = lower_expr(&new_expr.callee)?;
    let mut args = Vec::new();
    for arg in &new_expr.arguments {
        let expr = match arg {
            ast::Argument::SpreadElement(spread) => {
                Expression::Spread(Box::new(lower_expr(&spread.argument)?))
            }
            // Use as_expression() to convert boxed variant to Expression
            arg => lower_expr(
                arg.as_expression()
                    .ok_or(LowerError::new("Invalid argument"))?,
            )?,
        };
        args.push(expr);
    }
    Ok(Expression::New {
        constructor: Box::new(constructor),
        arguments: args,
    })
}

fn lower_seq_expr(seq: &ast::SequenceExpression) -> Result<Expression, LowerError> {
    let exprs: Vec<Expression> = seq
        .expressions
        .iter()
        .filter_map(|e| lower_expr(e).ok())
        .collect();
    Ok(Expression::Sequence(exprs))
}

fn lower_cond_expr(cond: &ast::ConditionalExpression) -> Result<Expression, LowerError> {
    let test = lower_expr(&cond.test)?;
    let consequent = lower_expr(&cond.consequent)?;
    let alternate = lower_expr(&cond.alternate)?;
    Ok(Expression::Conditional {
        condition: Box::new(test),
        consequent: Box::new(consequent),
        alternate: Box::new(alternate),
    })
}

pub(crate) fn lower_member_prop(
    prop: &ast::IdentifierName,
) -> Result<(PropertyKey, bool), LowerError> {
    Ok((PropertyKey::Ident(prop.name.as_str().to_string()), false))
}

pub fn lower_assignment_target(target: &ast::AssignmentTarget) -> Result<Expression, LowerError> {
    if let Some(binding) = crate::lower::pattern::lower_assignment_target_to_binding(target) {
        return Ok(crate::lower::pattern::binding_to_expr(binding));
    }
    match target {
        ast::AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::AssignmentTarget::StaticMemberExpression(sm) => {
            let obj = lower_expr(&sm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(sm.property.name.as_str().to_string()),
                computed: false,
            })
        }
        ast::AssignmentTarget::ComputedMemberExpression(cm) => {
            let obj = lower_expr(&cm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Computed(Box::new(lower_expr(&cm.expression)?)),
                computed: true,
            })
        }
        ast::AssignmentTarget::PrivateFieldExpression(pf) => {
            let obj = lower_expr(&pf.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(pf.field.name.as_str().to_string()),
                computed: false,
            })
        }
        // TS type assertions: strip the type and lower the inner expression
        ast::AssignmentTarget::TSAsExpression(e) => lower_expr(&e.expression),
        ast::AssignmentTarget::TSSatisfiesExpression(e) => lower_expr(&e.expression),
        ast::AssignmentTarget::TSNonNullExpression(e) => lower_expr(&e.expression),
        ast::AssignmentTarget::TSTypeAssertion(e) => lower_expr(&e.expression),
        ast::AssignmentTarget::TSInstantiationExpression(e) => lower_expr(&e.expression),
        _ => Err(LowerError::new("Unsupported assignment target")),
    }
}

fn lower_simple_assignment_target(
    target: &ast::SimpleAssignmentTarget,
) -> Result<Expression, LowerError> {
    // Same as lower_assignment_target but for SimpleAssignmentTarget
    match target {
        ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::SimpleAssignmentTarget::StaticMemberExpression(sm) => {
            let obj = lower_expr(&sm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(sm.property.name.as_str().to_string()),
                computed: false,
            })
        }
        ast::SimpleAssignmentTarget::ComputedMemberExpression(cm) => {
            let obj = lower_expr(&cm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Computed(Box::new(lower_expr(&cm.expression)?)),
                computed: true,
            })
        }
        ast::SimpleAssignmentTarget::PrivateFieldExpression(pf) => {
            let obj = lower_expr(&pf.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(pf.field.name.as_str().to_string()),
                computed: false,
            })
        }
        // TS type assertions: strip the type and lower the inner expression
        ast::SimpleAssignmentTarget::TSAsExpression(e) => lower_expr(&e.expression),
        ast::SimpleAssignmentTarget::TSSatisfiesExpression(e) => lower_expr(&e.expression),
        ast::SimpleAssignmentTarget::TSNonNullExpression(e) => lower_expr(&e.expression),
        ast::SimpleAssignmentTarget::TSTypeAssertion(e) => lower_expr(&e.expression),
        ast::SimpleAssignmentTarget::TSInstantiationExpression(e) => lower_expr(&e.expression),
        // Complex assignment targets not supported
        _ => Err(LowerError::new("Complex assignment target not supported")),
    }
}

fn lower_class_expr(class_expr: &ast::Class) -> Result<Expression, LowerError> {
    let class = lower_class(class_expr)?;
    let name: Option<String> = class_expr.id.as_ref().map(|i| i.name.as_str().to_string());
    Ok(Expression::Class(Class {
        name,
        super_class: class.super_class,
        body: class.body,
    }))
}

pub fn lower_class(class: &ast::Class) -> Result<Class, LowerError> {
    let name: Option<String> = None;
    let super_class = class.super_class.as_ref().and_then(|e| lower_expr(e).ok());
    let body = class
        .body
        .body
        .iter()
        .map(lower_class_member)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Class {
        name,
        super_class: super_class.map(Box::new),
        body,
    })
}

#[allow(clippy::complexity)]
fn lower_class_member(member: &ast::ClassElement) -> Result<ClassMember, LowerError> {
    match member {
        // ClassElement has MethodDefinition which includes constructors (kind == Constructor)
        ast::ClassElement::MethodDefinition(method) => {
            if method.kind == ast::MethodDefinitionKind::Constructor {
                lower_constructor(method)
            } else {
                lower_method(method)
            }
        }
        ast::ClassElement::PropertyDefinition(prop) => lower_class_prop(prop),
        ast::ClassElement::StaticBlock(_) => Err(LowerError::new("Static blocks not supported")),
        ast::ClassElement::AccessorProperty(_) => {
            Err(LowerError::new("Accessor properties not supported"))
        }
        _ => Err(LowerError::new("Unsupported class member")),
    }
}

fn lower_constructor(constructor: &ast::MethodDefinition) -> Result<ClassMember, LowerError> {
    let ps: Vec<String> = constructor
        .value
        .params
        .items
        .iter()
        .filter_map(|p| match &p.pattern.kind {
            ast::BindingPatternKind::BindingIdentifier(ident) => {
                Some(ident.name.as_str().to_string())
            }
            _ => None,
        })
        .collect();
    let body = constructor
        .value
        .body
        .as_ref()
        .map(|b| {
            b.statements
                .iter()
                .filter_map(super::stmt::lower_stmt)
                .collect()
        })
        .unwrap_or_default();
    Ok(ClassMember::Constructor { params: ps, body })
}

#[allow(clippy::complexity)]
fn lower_method(method: &ast::MethodDefinition) -> Result<ClassMember, LowerError> {
    let name = lower_prop_name_key_oxc(&method.key)?;
    let is_static = method.r#static;
    let ps: Vec<Param> = lower_formal_params(&method.value.params);
    let body = method
        .value
        .body
        .as_ref()
        .map(|b| {
            b.statements
                .iter()
                .filter_map(super::stmt::lower_stmt)
                .collect()
        })
        .unwrap_or_default();
    match method.kind {
        ast::MethodDefinitionKind::Get => Ok(ClassMember::Getter { name, body }),
        ast::MethodDefinitionKind::Set => {
            let param = ps.first().map(|p| p.name.clone()).unwrap_or_default();
            Ok(ClassMember::Setter { name, param, body })
        }
        _ => {
            if is_static {
                Ok(ClassMember::StaticMethod {
                    name,
                    params: ps,
                    body,
                })
            } else {
                Ok(ClassMember::Method {
                    name,
                    params: ps,
                    body,
                })
            }
        }
    }
}

fn lower_class_prop(prop: &ast::PropertyDefinition) -> Result<ClassMember, LowerError> {
    let name = lower_prop_name_key_oxc(&prop.key)?;
    let value = match &prop.value {
        Some(expr) => lower_expr(expr)?,
        None => Expression::Undefined,
    };
    if prop.r#static {
        Ok(ClassMember::StaticField { name, value })
    } else {
        Ok(ClassMember::Field { name, value })
    }
}

/// Lower a property key to PropertyKey
fn lower_prop_name_key_oxc(key: &ast::PropertyKey) -> Result<PropertyKey, LowerError> {
    match key {
        ast::PropertyKey::StaticIdentifier(i) => {
            Ok(PropertyKey::Ident(i.name.as_str().to_string()))
        }
        ast::PropertyKey::PrivateIdentifier(i) => Ok(PropertyKey::Ident(format!("#{}", i.name))),
        ast::PropertyKey::StringLiteral(s) => Ok(PropertyKey::String(s.value.to_string())),
        ast::PropertyKey::NumericLiteral(n) => Ok(PropertyKey::Number(n.value)),
        ast::PropertyKey::BigIntLiteral(b) => Ok(PropertyKey::String(b.raw.to_string())),
        ast::PropertyKey::BooleanLiteral(b) => Ok(PropertyKey::String(b.value.to_string())),
        ast::PropertyKey::NullLiteral(_) => Ok(PropertyKey::String("null".to_string())),
        // In OXC, computed property names use Expression variants directly in PropertyKey
        // Use to_expression() to get the expression and lower it
        ast::PropertyKey::TemplateLiteral(t) => {
            // Template literal in property key position - get static part if no expressions
            if t.expressions.is_empty() {
                // No expressions, we can use the quasi content as a static string
                let static_part = t
                    .quasis
                    .first()
                    .map(|q| {
                        q.value
                            .cooked
                            .as_ref()
                            .map(|c| c.as_str())
                            .unwrap_or(q.value.raw.as_str())
                            .to_string()
                    })
                    .unwrap_or_default();
                Ok(PropertyKey::String(static_part))
            } else {
                // Has expressions - treat as computed
                let expr = key.to_expression();
                Ok(PropertyKey::Computed(Box::new(lower_expr(expr)?)))
            }
        }
        _ => {
            // For other expression variants (computed property names),
            // use to_expression() and lower as a computed key
            let expr = key.to_expression();
            Ok(PropertyKey::Computed(Box::new(lower_expr(expr)?)))
        }
    }
}
