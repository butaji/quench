//! JSX generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenJsx;

impl CodeGenJsx {
    pub fn jsx_to_rust(cg: &CodeGenerator, x: &JSXExpr) -> String {
        Self::jsx_element_to_string(cg, x, 0)
    }

    pub fn jsx_element_to_string(cg: &CodeGenerator, x: &JSXExpr, _depth: usize) -> String {
        let tag_str = Self::jsx_name_to_string(&x.opening.name);
        let is_component = tag_str.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        let (attrs_str, has_children) = Self::jsx_attrs_to_string(cg, &x.opening.attrs, &x.children, is_component);
        let inner_str = Self::jsx_element_inner(cg, x);

        if x.opening.self_closing {
            if is_component {
                format!("{}({})", tag_str, attrs_str)
            } else {
                format!("html!(\"<{}{} />\", {})", tag_str, attrs_str, inner_str)
            }
        } else if is_component {
            if has_children {
                format!("{}({} {{\n    html! {{ {} }}\n}})", tag_str, attrs_str, inner_str)
            } else {
                format!("{}({})", tag_str, attrs_str)
            }
        } else {
            let closing = x.closing.as_ref().map(|c| Self::jsx_name_to_string(&c.name)).unwrap_or_default();
            if has_children {
                format!("html!(\"<{} {}>{}</{}>\", {})", tag_str, attrs_str, inner_str, closing, inner_str)
            } else {
                format!("html!(\"<{}{}></{}>\", {})", tag_str, attrs_str, closing, inner_str)
            }
        }
    }

    fn jsx_name_to_string(name: &JSXName) -> String {
        match name {
            JSXName::Ident(s) => s.clone(),
            JSXName::Member { object, property } => format!("{}.{}", object, property),
            JSXName::Fragment => "Fragment".to_string(),
            JSXName::Namespaced { ns, name: n } => format!("{}:{}", ns, n),
            JSXName::Dynamic(expr) => format!("{{{:?}}}", expr),
        }
    }

    fn jsx_attrs_to_string(cg: &CodeGenerator, attrs: &[JSXAttr], children: &[JSXChild], is_component: bool) -> (String, bool) {
        let has_children = !children.is_empty();
        if attrs.is_empty() && !has_children {
            return ("{}".to_string(), has_children);
        }
        let mut pairs: Vec<String> = attrs.iter().filter_map(|a| {
            match a {
                JSXAttr::Attr { name, value } => {
                    let val = value.as_ref().map(|v| Self::jsx_attr_value_to_rust(cg, v)).unwrap_or_default();
                    if is_component {
                        Some(format!("{}: {}", name, val))
                    } else {
                        Some(format!("{}=\"{}\"", name, val))
                    }
                }
                JSXAttr::Spread { expr } => {
                    Some(cg.expr_to_rust(expr))
                }
                JSXAttr::Event { name, handler } => {
                    Some(format!("on_{}=\"{}\"", name, cg.expr_to_rust(handler)))
                }
                JSXAttr::Bool { name } => {
                    if is_component {
                        Some(format!("{}: true", name))
                    } else {
                        Some(name.clone())
                    }
                }
                JSXAttr::Expr { name, expr } => {
                    let val = Self::jsx_attr_value_to_rust(cg, &JSXAttrValue::Expr(expr.clone()));
                    if let Some(n) = name {
                        if is_component {
                            Some(format!("{}: {}", n, val))
                        } else {
                            Some(format!("{}=\"{}\"", n, val))
                        }
                    } else {
                        Some(val)
                    }
                }
            }
        }).collect();
        if has_children {
            pairs.push("children: Vec::new()".to_string());
        }
        let attrs_str = pairs.join(", ");
        (format!("{{{}}}", attrs_str), has_children)
    }

    pub fn jsx_element_inner(cg: &CodeGenerator, x: &JSXExpr) -> String {
        let mut parts: Vec<String> = Vec::new();
        for child in &x.children {
            parts.push(cg.jsx_child_to_rust(child));
        }
        parts.join(", ")
    }

    pub fn jsx_attr_value_to_rust(cg: &CodeGenerator, v: &JSXAttrValue) -> String {
        match v {
            JSXAttrValue::String(s) => s.clone(),
            JSXAttrValue::Expr(expr) => cg.expr_to_rust(expr),
        }
    }

    pub fn jsx_child_to_rust(cg: &CodeGenerator, child: &JSXChild) -> String {
        match child {
            JSXChild::Text(s) => format!("VNode::Text(\"{}\".to_string())", s.replace('"', "\\\"")),
            JSXChild::Expr(expr) => {
                let e = cg.expr_to_rust(expr);
                if matches!(expr, Expr::Null | Expr::Undefined) {
                    "VNode::Empty".to_string()
                } else {
                    format!("runts_lib::runtime::helpers::to_vnode({})", e)
                }
            }
            JSXChild::JSX(x) => Self::jsx_element_to_string(cg, x, 0),
            JSXChild::Fragment { .. } => "VNode::Empty".to_string(),
            JSXChild::Spread { expr } => cg.expr_to_rust(expr),
        }
    }

    pub fn expr_needs_clone(e: &Expr) -> bool {
        matches!(e, Expr::Function { .. } | Expr::Arrow { .. })
    }

    pub fn jsx_style_value_to_rust(cg: &CodeGenerator, v: &JSXAttrValue) -> String {
        match v {
            JSXAttrValue::String(s) => format!("\"{}\".to_string()", s.replace(':', "\": \"").replace(';', "\", \"")),
            JSXAttrValue::Expr(expr) => {
                let e = cg.expr_to_rust(expr);
                if matches!(expr, Expr::Object { .. }) {
                    format!("runts_lib::runtime::helpers::style_to_string({})", e)
                } else { e }
            }
        }
    }
}
