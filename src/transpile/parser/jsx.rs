//! JSX conversion
//!
//! allow:too_many_lines,complexity

use crate::transpile::hir;
use crate::transpile::parser::expr::convert_expr;

use oxc_ast::ast::*;

pub fn convert_jsx_element(elem: &JSXElement) -> hir::JSXExpr {
    let opening_elem = elem.opening_element.as_ref();
    let name = convert_jsx_name(&opening_elem.name);

    let opening = hir::JSXOpening {
        name,
        attrs: opening_elem
            .attributes
            .iter()
            .filter_map(convert_jsx_attr)
            .collect(),
        self_closing: elem.closing_element.is_none(),
    };

    hir::JSXExpr {
        opening,
        children: elem.children.iter().filter_map(convert_jsx_child).collect(),
        closing: elem.closing_element.as_ref().map(|c| hir::JSXClosing {
            name: convert_jsx_name(&c.name),
        }),
    }
}

pub fn convert_jsx_fragment(frag: &JSXFragment) -> hir::JSXExpr {
    hir::JSXExpr {
        opening: hir::JSXOpening {
            name: hir::JSXName::Fragment,
            attrs: vec![],
            self_closing: false,
        },
        children: frag.children.iter().filter_map(convert_jsx_child).collect(),
        closing: None,
    }
}

fn convert_jsx_name(name: &JSXElementName) -> hir::JSXName {
    match name {
        JSXElementName::Identifier(id) => hir::JSXName::Ident(id.name.to_string()),
        JSXElementName::NamespacedName(ns) => hir::JSXName::Namespaced {
            ns: ns.namespace.name.to_string(),
            name: ns.name.name.to_string(),
        },
        JSXElementName::MemberExpression(m) => {
            // Handle <Foo.Bar> - member expression like React.Foo
            let object_name = match &m.object {
                JSXMemberExpressionObject::IdentifierReference(id) => id.name.to_string(),
                JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
                JSXMemberExpressionObject::MemberExpression(inner) => {
                    // Recursively handle nested member expressions
                    match &inner.object {
                        JSXMemberExpressionObject::IdentifierReference(id) => id.name.to_string(),
                        _ => String::new(),
                    }
                }
            };
            hir::JSXName::Member {
                object: object_name,
                property: m.property.name.to_string(),
            }
        }
        JSXElementName::IdentifierReference(id) => hir::JSXName::Ident(id.name.to_string()),
        JSXElementName::ThisExpression(_) => {
            // Handle <this.Foo> - 'this' in JSX
            hir::JSXName::Dynamic(Box::new(hir::Expr::This))
        }
    }
}

fn convert_jsx_attr(attr: &JSXAttributeItem) -> Option<hir::JSXAttr> {
    match attr {
        JSXAttributeItem::Attribute(attr) => {
            let attr_name = match &attr.name {
                JSXAttributeName::Identifier(id) => id.name.to_string(),
                JSXAttributeName::NamespacedName(ns) => format!(
                    "{}:{}",
                    ns.namespace.name.to_string(),
                    ns.name.name.to_string()
                ),
            };
            let value = attr.value.as_ref().and_then(convert_jsx_attr_value);
            Some(hir::JSXAttr::Attr {
                name: attr_name,
                value,
            })
        }
        JSXAttributeItem::SpreadAttribute(s) => {
            // Handle {...props}
            let arg = convert_expr(&s.argument).ok()?;
            Some(hir::JSXAttr::Spread { expr: arg })
        }
    }
}

fn convert_jsx_attr_value(value: &JSXAttributeValue) -> Option<hir::JSXAttrValue> {
    match value {
        JSXAttributeValue::ExpressionContainer(e) => {
            convert_jsx_expression(&e.expression).map(|e| hir::JSXAttrValue::Expr(e))
        }
        JSXAttributeValue::StringLiteral(s) => Some(hir::JSXAttrValue::String(s.value.to_string())),
        JSXAttributeValue::Element(e) => {
            // Handle JSX element as attribute value
            Some(hir::JSXAttrValue::Expr(hir::Expr::JSX(convert_jsx_element(e))))
        }
        JSXAttributeValue::Fragment(_) => None, // Fragments can't be attribute values
        _ => None,
    }
}

// allow:complexity
pub fn convert_jsx_expression(expr: &JSXExpression) -> Option<hir::Expr> {
    match expr {
        JSXExpression::EmptyExpression(_) => Some(hir::Expr::Null),
        JSXExpression::Identifier(id) => Some(hir::Expr::Ident {
            name: id.name.to_string(),
        }),
        JSXExpression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        JSXExpression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        JSXExpression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        JSXExpression::NullLiteral(_) => Some(hir::Expr::Null),
        JSXExpression::ThisExpression(_) => Some(hir::Expr::This),
        JSXExpression::Super(_) => Some(hir::Expr::Super),
        JSXExpression::BigIntLiteral(b) => Some(hir::Expr::BigInt(b.raw.as_ref().map(|s| s.to_string()).unwrap_or_else(|| b.value.to_string()).parse().unwrap_or(0))),
        _ => expr.as_expression().and_then(|e| convert_expr(e).ok()),
    }
}

pub fn convert_jsx_child(child: &JSXChild) -> Option<hir::JSXChild> {
    match child {
        JSXChild::Text(t) => Some(hir::JSXChild::Text(t.value.to_string())),
        JSXChild::ExpressionContainer(e) => {
            convert_jsx_expression(&e.expression).map(|expr| hir::JSXChild::Expr(expr))
        }
        JSXChild::Element(elem) => Some(hir::JSXChild::JSX(convert_jsx_element(elem))),
        JSXChild::Fragment(frag) => Some(hir::JSXChild::JSX(hir::JSXExpr {
            opening: hir::JSXOpening {
                name: hir::JSXName::Fragment,
                attrs: vec![],
                self_closing: false,
            },
            children: frag.children.iter().filter_map(convert_jsx_child).collect(),
            closing: None,
        })),
        JSXChild::Spread(s) => {
            // Handle {...children} spread children
            let arg = convert_expr(&s.expression).ok()?;
            Some(hir::JSXChild::Spread { expr: arg })
        }
    }
}
