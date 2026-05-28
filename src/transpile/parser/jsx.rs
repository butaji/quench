//! JSX conversion

use crate::transpile::hir as hir;
use crate::transpile::parser::expr::convert_expr;

use oxc_ast::ast::*;

pub fn convert_jsx_element(elem: &JSXElement) -> hir::JSXExpr {
    let opening_elem = elem.opening_element.as_ref();
    let name = convert_jsx_name(&opening_elem.name);
    
    let opening = hir::JSXOpening {
        name,
        attrs: opening_elem.attributes.iter().filter_map(convert_jsx_attr).collect(),
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

fn convert_jsx_name(name: &JSXElementName) -> hir::JSXName {
    match name {
        JSXElementName::Identifier(id) => hir::JSXName::Ident(id.name.to_string()),
        JSXElementName::NamespacedName(ns) => hir::JSXName::Ident(format!("{}:{}", ns.namespace.name.to_string(), ns.name.name.to_string())),
        JSXElementName::MemberExpression(_) => hir::JSXName::Ident("member".to_string()),
        JSXElementName::IdentifierReference(id) => hir::JSXName::Ident(id.name.to_string()),
        JSXElementName::ThisExpression(_) => hir::JSXName::Ident("this".to_string()),
    }
}

fn convert_jsx_attr(attr: &JSXAttributeItem) -> Option<hir::JSXAttr> {
    match attr {
        JSXAttributeItem::Attribute(attr) => {
            let attr_name = match &attr.name {
                JSXAttributeName::Identifier(id) => id.name.to_string(),
                JSXAttributeName::NamespacedName(ns) => format!("{}:{}", ns.namespace.name.to_string(), ns.name.name.to_string()),
            };
            let value = attr.value.as_ref().and_then(convert_jsx_attr_value);
            Some(hir::JSXAttr::Attr { name: attr_name, value })
        }
        _ => None,
    }
}

fn convert_jsx_attr_value(value: &JSXAttributeValue) -> Option<hir::JSXAttrValue> {
    match value {
        JSXAttributeValue::ExpressionContainer(e) => {
            convert_jsx_expression(&e.expression).map(|e| hir::JSXAttrValue::Expr(e))
        }
        JSXAttributeValue::StringLiteral(s) => {
            Some(hir::JSXAttrValue::String(s.value.to_string()))
        }
        _ => None,
    }
}

pub fn convert_jsx_expression(expr: &JSXExpression) -> Option<hir::Expr> {
    match expr {
        JSXExpression::EmptyExpression(_) => Some(hir::Expr::Null),
        JSXExpression::Identifier(id) => Some(hir::Expr::Ident { name: id.name.to_string() }),
        JSXExpression::NumericLiteral(n) => Some(hir::Expr::Number(n.value)),
        JSXExpression::StringLiteral(s) => Some(hir::Expr::String(s.value.to_string())),
        JSXExpression::BooleanLiteral(b) => Some(hir::Expr::Boolean(b.value)),
        JSXExpression::NullLiteral(_) => Some(hir::Expr::Null),
        _ => expr.as_expression().and_then(convert_expr),
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
        _ => None,
    }
}
