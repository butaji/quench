//! JSX expression lowering
//!
//! Handles lowering of JSX elements, fragments, attributes, and children.

use super::expr::lower_expr;
use super::helpers::{atom_to_string, wtf8_atom_to_string, LowerError};
use crate::ast::{Expression, JsxAttrValue, JsxChild, JsxProp, JsxTagName};
use swc_ecma_ast as swc;

/// Lower a JSX member expression (e.g., Foo.Bar.Baz)
pub fn lower_jsx_member(member: &swc::JSXMemberExpr) -> Result<Expression, LowerError> {
    let obj = match &member.obj {
        swc::JSXObject::Ident(ident) => atom_to_string(&ident.sym),
        swc::JSXObject::JSXMemberExpr(nested) => {
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
    };
    let property = atom_to_string(&member.prop.sym);
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
pub fn lower_jsx_namespaced(ns: &swc::JSXNamespacedName) -> Result<Expression, LowerError> {
    let namespace = atom_to_string(&ns.ns.sym);
    let name = atom_to_string(&ns.name.sym);
    Ok(Expression::JsxElement {
        tag: JsxTagName::Namespaced { namespace, name },
        props: vec![],
        children: vec![],
    })
}

/// Lower a JSX element
pub fn lower_jsx_element(elem: &swc::JSXElement) -> Result<Expression, LowerError> {
    let tag = lower_jsx_element_name(&elem.opening.name)?;
    let props = lower_jsx_attributes(&elem.opening.attrs)?;
    let children = lower_jsx_children(&elem.children)?;

    Ok(Expression::JsxElement {
        tag,
        props,
        children,
    })
}

/// Lower a JSX fragment
pub fn lower_jsx_fragment(frag: &swc::JSXFragment) -> Result<Expression, LowerError> {
    let children = lower_jsx_children(&frag.children)?;
    Ok(Expression::JsxFragment { children })
}

/// Lower a JSX element name to JsxTagName
#[allow(clippy::complexity)]
pub fn lower_jsx_element_name(name: &swc::JSXElementName) -> Result<JsxTagName, LowerError> {
    match name {
        swc::JSXElementName::Ident(ident) => Ok(JsxTagName::Ident(atom_to_string(&ident.sym))),
        swc::JSXElementName::JSXMemberExpr(member) => {
            let obj = match &member.obj {
                swc::JSXObject::Ident(ident) => atom_to_string(&ident.sym),
                swc::JSXObject::JSXMemberExpr(nested) => {
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
            };
            let property = atom_to_string(&member.prop.sym);
            Ok(JsxTagName::Member {
                object: obj,
                property,
            })
        }
        swc::JSXElementName::JSXNamespacedName(ns) => {
            let namespace = atom_to_string(&ns.ns.sym);
            let name = atom_to_string(&ns.name.sym);
            Ok(JsxTagName::Namespaced { namespace, name })
        }
    }
}

/// Lower JSX attributes/props
#[allow(clippy::complexity)]
pub fn lower_jsx_attributes(attrs: &[swc::JSXAttrOrSpread]) -> Result<Vec<JsxProp>, LowerError> {
    let mut props = Vec::new();
    for attr_or_spread in attrs {
        match attr_or_spread {
            swc::JSXAttrOrSpread::JSXAttr(attr) => {
                let name = lower_jsx_attr_name(&attr.name)?;
                let value = match &attr.value {
                    Some(swc::JSXAttrValue::JSXExprContainer(expr)) => match &expr.expr {
                        swc::JSXExpr::Expr(expr) => JsxAttrValue::Expression(lower_expr(expr)?),
                        swc::JSXExpr::JSXEmptyExpr(_) => JsxAttrValue::Expression(Expression::Null),
                    },
                    Some(swc::JSXAttrValue::Str(s)) => {
                        JsxAttrValue::String(wtf8_atom_to_string(&s.value))
                    }
                    Some(_) | None => JsxAttrValue::Expression(Expression::Null),
                };
                props.push(JsxProp::Attr { name, value });
            }
            swc::JSXAttrOrSpread::SpreadElement(spread) => {
                let expr = lower_expr(&spread.expr)?;
                props.push(JsxProp::Spread(expr));
            }
        }
    }
    Ok(props)
}

/// Lower a JSX attribute name
fn lower_jsx_attr_name(name: &swc::JSXAttrName) -> Result<String, LowerError> {
    match name {
        swc::JSXAttrName::Ident(ident) => Ok(atom_to_string(&ident.sym)),
        swc::JSXAttrName::JSXNamespacedName(ns) => Ok(format!(
            "{}:{}",
            atom_to_string(&ns.ns.sym),
            atom_to_string(&ns.name.sym)
        )),
    }
}

/// Lower JSX children
#[allow(clippy::complexity)]
pub fn lower_jsx_children(children: &[swc::JSXElementChild]) -> Result<Vec<JsxChild>, LowerError> {
    let mut result = Vec::new();
    for child in children {
        match child {
            swc::JSXElementChild::JSXText(text) => {
                result.push(JsxChild::Text(text.value.to_string()));
            }
            swc::JSXElementChild::JSXExprContainer(expr) => {
                match &expr.expr {
                    swc::JSXExpr::Expr(inner) => {
                        result.push(JsxChild::Expression(lower_expr(inner)?));
                    }
                    swc::JSXExpr::JSXEmptyExpr(_) => {
                        // Empty expression, skip
                    }
                }
            }
            swc::JSXElementChild::JSXSpreadChild(spread) => {
                let expr = lower_expr(&spread.expr)?;
                result.push(JsxChild::Spread(expr));
            }
            swc::JSXElementChild::JSXElement(elem) => {
                let elem_expr = lower_jsx_element(elem)?;
                result.push(JsxChild::Element(Box::new(elem_expr)));
            }
            swc::JSXElementChild::JSXFragment(frag) => {
                let frag_expr = lower_jsx_fragment(frag)?;
                result.push(JsxChild::Element(Box::new(frag_expr)));
            }
        }
    }
    Ok(result)
}
