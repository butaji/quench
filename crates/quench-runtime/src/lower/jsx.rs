//! JSX expression lowering
//!
//! Handles lowering of JSX elements, fragments, attributes, and children.

use super::expr::lower_expr;
use super::helpers::LowerError;
use crate::ast::{Expression, JsxAttrValue, JsxChild, JsxProp, JsxTagName};
use oxc::ast::ast as ast;

/// Lower a JSX member expression (e.g., Foo.Bar.Baz)
pub fn lower_jsx_member(member: &ast::JSXMemberExpression) -> Result<Expression, LowerError> {
    let obj = match &member.object {
        ast::JSXMemberExpressionObject::IdentifierReference(ident) => ident.name.as_str().to_string(),
        ast::JSXMemberExpressionObject::MemberExpression(nested) => {
            let nested_result = lower_jsx_member(nested)?;
            match nested_result {
                Expression::JsxElement {
                    tag: JsxTagName::Member { object, property },
                    ..
                } => {
                    format!("{}.{}", object, property)
                }
                _ => return Err(LowerError::new("Invalid nested JSX member expression")),
            }
        }
        ast::JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
    };
    let property = member.property.name.as_str().to_string();
    Ok(Expression::JsxElement {
        tag: JsxTagName::Member {
            object: obj,
            property,
        },
        props: vec![],
        children: vec![],
    })
}

/// Lower a JSX namespaced name (e.g., ns:Name)
pub fn lower_jsx_namespaced(ns: &ast::JSXNamespacedName) -> Result<Expression, LowerError> {
    let namespace = ns.namespace.name.as_str().to_string();
    let name = ns.property.name.as_str().to_string();
    Ok(Expression::JsxElement {
        tag: JsxTagName::Namespaced { namespace, name },
        props: vec![],
        children: vec![],
    })
}

/// Lower a JSX element
pub fn lower_jsx_element(elem: &ast::JSXElement) -> Result<Expression, LowerError> {
    let tag = lower_jsx_element_name(&elem.opening_element.name)?;
    let props = lower_jsx_attributes(&elem.opening_element.attributes)?;
    let children = lower_jsx_children(&elem.children)?;

    Ok(Expression::JsxElement {
        tag,
        props,
        children,
    })
}

/// Lower a JSX fragment
pub fn lower_jsx_fragment(frag: &ast::JSXFragment) -> Result<Expression, LowerError> {
    let children = lower_jsx_children(&frag.children)?;
    Ok(Expression::JsxFragment { children })
}

/// Lower a JSX element name to JsxTagName
pub fn lower_jsx_element_name(name: &ast::JSXElementName) -> Result<JsxTagName, LowerError> {
    match name {
        ast::JSXElementName::IdentifierReference(ident) => Ok(JsxTagName::Ident(ident.name.as_str().to_string())),
        ast::JSXElementName::Identifier(ident) => Ok(JsxTagName::Ident(ident.name.as_str().to_string())),
        ast::JSXElementName::ThisExpression(_) => Ok(JsxTagName::Ident("this".to_string())),
        ast::JSXElementName::MemberExpression(member) => {
            let obj = match &member.object {
                ast::JSXMemberExpressionObject::IdentifierReference(ident) => ident.name.as_str().to_string(),
                ast::JSXMemberExpressionObject::MemberExpression(nested) => {
                    let nested_result = lower_jsx_member(nested)?;
                    match nested_result {
                        Expression::JsxElement {
                            tag: JsxTagName::Member { object, property },
                            ..
                        } => {
                            format!("{}.{}", object, property)
                        }
                        _ => return Err(LowerError::new("Invalid nested JSX member expression")),
                    }
                }
                ast::JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
            };
            let property = member.property.name.as_str().to_string();
            Ok(JsxTagName::Member {
                object: obj,
                property,
            })
        }
        ast::JSXElementName::NamespacedName(ns) => {
            let namespace = ns.namespace.name.as_str().to_string();
            let name = ns.property.name.as_str().to_string();
            Ok(JsxTagName::Namespaced { namespace, name })
        }
    }
}

/// Lower JSX attributes/props
pub fn lower_jsx_attributes(attrs: &[ast::JSXAttributeItem]) -> Result<Vec<JsxProp>, LowerError> {
    let mut props = Vec::new();
    for attr in attrs {
        match attr {
            ast::JSXAttributeItem::Attribute(attr) => {
                let name = lower_jsx_attr_name(&attr.name)?;
                let value = match &attr.value {
                    Some(ast::JSXAttributeValue::StringLiteral(s)) => {
                        JsxAttrValue::String(s.value.to_string())
                    }
                    Some(ast::JSXAttributeValue::ExpressionContainer(expr)) => {
                        // JSXExpression inherits from Expression, use as_expression()
                        if let Some(inner) = expr.expression.as_expression() {
                            JsxAttrValue::Expression(lower_expr(inner)?)
                        } else {
                            JsxAttrValue::Expression(Expression::Null)
                        }
                    }
                    Some(_) | None => JsxAttrValue::Expression(Expression::Null),
                };
                props.push(JsxProp::Attr { name, value });
            }
            ast::JSXAttributeItem::SpreadAttribute(spread) => {
                let expr = lower_expr(&spread.argument)?;
                props.push(JsxProp::Spread(expr));
            }
        }
    }
    Ok(props)
}

/// Lower a JSX attribute name
fn lower_jsx_attr_name(name: &ast::JSXAttributeName) -> Result<String, LowerError> {
    match name {
        ast::JSXAttributeName::Identifier(ident) => Ok(ident.name.as_str().to_string()),
        ast::JSXAttributeName::NamespacedName(ns) => Ok(format!("{}:{}", ns.namespace.name.as_str(), ns.property.name.as_str())),
    }
}

/// Lower JSX children
pub fn lower_jsx_children(children: &[ast::JSXChild]) -> Result<Vec<JsxChild>, LowerError> {
    let mut result = Vec::new();
    for child in children {
        match child {
            ast::JSXChild::Text(text) => {
                result.push(JsxChild::Text(text.value.to_string()));
            }
            ast::JSXChild::ExpressionContainer(expr) => {
                // JSXExpression inherits from Expression, use as_expression()
                if let Some(inner) = expr.expression.as_expression() {
                    result.push(JsxChild::Expression(lower_expr(inner)?));
                }
                // Empty expression is skipped
            }
            ast::JSXChild::Element(elem) => {
                let elem_expr = lower_jsx_element(elem)?;
                result.push(JsxChild::Element(Box::new(elem_expr)));
            }
            ast::JSXChild::Fragment(frag) => {
                let frag_expr = lower_jsx_fragment(frag)?;
                result.push(JsxChild::Element(Box::new(frag_expr)));
            }
            // SpreadChild might not exist in all OXC versions, skip if not present
            _ => {}
        }
    }
    Ok(result)
}
