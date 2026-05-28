//! JSX generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenJsx;

impl CodeGenJsx {
    pub fn jsx_to_rust(cg: &mut CodeGenerator, jsx: &JSXExpr) -> String {
        let tag = Self::jsx_tag_name(&jsx.opening.name);
        let attrs = Self::jsx_attrs_to_string(cg, &jsx.opening.attrs);
        let children = Self::jsx_children_to_string(cg, &jsx.children);
        if jsx.opening.self_closing { if attrs.is_empty() { format!("html!(<{}/>)", tag) } else { format!("html!(<{} {} />)", tag, attrs) } }
        else { if attrs.is_empty() { format!("html!(<{}>{}</{}>)", tag, children, tag) } else { format!("html!(<{} {}>{}</{}>)", tag, attrs, children, tag) } }
    }

    fn jsx_tag_name(name: &JSXName) -> String {
        match name { JSXName::Ident(s) => s.clone(), JSXName::Member { object, property } => format!("{}_{}", object, property), JSXName::Namespaced { ns, name } => format!("{}_{}", ns, name), JSXName::Dynamic(_) => "dynamic".to_string(), JSXName::Fragment => String::new() }
    }

    pub fn jsx_attrs_to_string(cg: &mut CodeGenerator, attrs: &[JSXAttr]) -> String {
        attrs.iter().filter_map(|attr| match attr {
            JSXAttr::Attr { name, value } => { let n = cg.jsx_attr_to_rust(name); let v = match value { Some(JSXAttrValue::String(s)) => format!("{:?}", s), Some(JSXAttrValue::Expr(e)) => cg.expr_to_rust(e), None => "true".to_string() }; Some(format!("{}={}", n, v)) }
            _ => None,
        }).collect::<Vec<_>>().join(" ")
    }

    fn jsx_children_to_string(cg: &mut CodeGenerator, children: &[JSXChild]) -> String {
        children.iter().filter_map(|c| Self::jsx_child_to_string(cg, c)).collect::<Vec<_>>().join(" ")
    }

    fn jsx_child_to_string(cg: &mut CodeGenerator, child: &JSXChild) -> Option<String> {
        match child { JSXChild::Text(s) => Some(s.trim().to_string()), JSXChild::Expr(e) => Some(cg.expr_to_rust(e)), JSXChild::JSX(j) => Some(Self::jsx_to_rust(cg, j)), _ => None }
    }
}
