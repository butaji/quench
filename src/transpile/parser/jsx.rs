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
        children: coalesce_text_children(
            elem.children.iter().filter_map(convert_jsx_child).collect(),
        ),
        closing: elem.closing_element.as_ref().map(|c| hir::JSXClosing {
            name: convert_jsx_name(&c.name),
        }),
    }
}

/// Coalesce adjacent `JSXChild::Text` nodes into a single
/// Text node. The oxc parser tokenizes JSX text into
/// separate Text children for each whitespace-separated
/// word (e.g. "Centered Title" becomes ["Centered", "Title"]).
/// This function joins them back into a single string.
fn coalesce_text_children(children: Vec<hir::JSXChild>) -> Vec<hir::JSXChild> {
    let mut out: Vec<hir::JSXChild> = Vec::new();
    for child in children {
        match (&mut out.last_mut(), &child) {
            (Some(hir::JSXChild::Text(acc)), hir::JSXChild::Text(s)) => {
                acc.push(' ');
                acc.push_str(s);
            }
            _ => out.push(child),
        }
    }
    out
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
            let arg = convert_expr(&s.expression).ok()?;
            Some(hir::JSXChild::Spread { expr: arg })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpile::hir;

    #[test]
    fn coalesce_empty() {
        let input: Vec<hir::JSXChild> = vec![];
        let out = coalesce_text_children(input);
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn coalesce_single_text() {
        let input = vec![hir::JSXChild::Text("hello".into())];
        let out = coalesce_text_children(input);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], hir::JSXChild::Text(s) if s == "hello"));
    }

    #[test]
    fn coalesce_two_adjacent_texts() {
        // oxc splits "hello world" into ["hello", "world"]
        let input = vec![
            hir::JSXChild::Text("hello".into()),
            hir::JSXChild::Text("world".into()),
        ];
        let out = coalesce_text_children(input);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], hir::JSXChild::Text(s) if s == "hello world"));
    }

    #[test]
    fn coalesce_three_adjacent_texts() {
        // "Centered Title" → ["Centered", "Title"]
        let input = vec![
            hir::JSXChild::Text("Centered".into()),
            hir::JSXChild::Text("Title".into()),
        ];
        let out = coalesce_text_children(input);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], hir::JSXChild::Text(s) if s == "Centered Title"));
    }

    #[test]
    fn coalesce_mixed_children() {
        // Text + JSX + Text should NOT coalesce
        // the two Text nodes (they're not adjacent)
        let input = vec![
            hir::JSXChild::Text("before".into()),
            hir::JSXChild::JSX(hir::JSXExpr {
                opening: hir::JSXOpening {
                    name: hir::JSXName::Ident("br".into()),
                    attrs: vec![],
                    self_closing: true,
                },
                closing: None,
                children: vec![],
            }),
            hir::JSXChild::Text("after".into()),
        ];
        let out = coalesce_text_children(input);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn coalesce_many_adjacent() {
        // "a b c d" → ["a", "b", "c", "d"]
        let input = vec![
            hir::JSXChild::Text("a".into()),
            hir::JSXChild::Text("b".into()),
            hir::JSXChild::Text("c".into()),
            hir::JSXChild::Text("d".into()),
        ];
        let out = coalesce_text_children(input);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], hir::JSXChild::Text(s) if s == "a b c d"));
    }

    #[test]
    fn parse_simple_text_preserves_content() {
        // End-to-end: parse "<Text>Hello, World!</Text>"
        // and verify the HIR has the full text.
        let src = r#"
export default function App() {
  return <Text>Hello, World!</Text>;
}
"#;
        let module = crate::transpile::parser::parse_source(src, true).unwrap();
        // Find the return statement's JSX
        for item in &module.items {
            if let hir::ModuleItem::Decl(hir::Decl::Function(f)) = item {
                if let Some(block) = &f.body {
                    for stmt in &block.0 {
                        if let hir::Stmt::Return { arg: Some(hir::Expr::JSX(jsx)) } = stmt {
                            assert_eq!(jsx.children.len(), 1);
                            match &jsx.children[0] {
                                hir::JSXChild::Text(s) => {
                                    assert_eq!(s, "Hello, World!",
                                        "expected full text, got: {s:?}");
                                }
                                other => panic!("expected Text, got {other:?}"),
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn parse_multipart_text_coalesces() {
        // End-to-end: parse "<Text>Centered Title</Text>"
        // and verify the HIR has the full text after
        // coalescing. This is the exact case that
        // was producing "C" before the fix.
        let src = r#"
export default function App() {
  return <Text>Centered Title</Text>;
}
"#;
        let module = crate::transpile::parser::parse_source(src, true).unwrap();
        for item in &module.items {
            if let hir::ModuleItem::Decl(hir::Decl::Function(f)) = item {
                if let Some(block) = &f.body {
                    for stmt in &block.0 {
                        if let hir::Stmt::Return { arg: Some(hir::Expr::JSX(jsx)) } = stmt {
                            // After coalesce, should be 1 child
                            assert_eq!(jsx.children.len(), 1,
                                "expected 1 child after coalesce, got {}: {:?}",
                                jsx.children.len(), jsx.children);
                            match &jsx.children[0] {
                                hir::JSXChild::Text(s) => {
                                    assert_eq!(s, "Centered Title",
                                        "expected 'Centered Title', got: {s:?}");
                                }
                                other => panic!("expected Text, got {other:?}"),
                            }
                        }
                    }
                }
            }
        }
    }
}

