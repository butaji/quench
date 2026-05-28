//! JSX to html! macro transformer

pub mod transform;

pub use transform::*;

use super::hir::*;
use crate::util::to_snake_case;

#[allow(dead_code)]
pub struct JsxTransformer;
impl JsxTransformer { pub fn transform(jsx: &JSXExpr) -> String { Self::transform_jsx_element(jsx) } }
impl JsxTransformer {
    fn transform_jsx_element(jsx: &JSXExpr) -> String {
        let tag = Self::transform_jsx_name(&jsx.opening.name);
        let attrs = Self::transform_attrs(&jsx.opening.attrs);
        let children = Self::transform_children(&jsx.children);
        if jsx.opening.self_closing { if attrs.is_empty() { format!("html!(<{}/>)", tag) } else { format!("html!(<{} {} />)", tag, attrs) } }
        else { if attrs.is_empty() { format!("html!(<{}>{}</{}>)", tag, children, tag) } else { format!("html!(<{} {}>{}</{}>)", tag, attrs, children, tag) } }
    }

    fn transform_jsx_name(name: &JSXName) -> String {
        match name {
            JSXName::Ident(s) => if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) { to_snake_case(s) } else { s.clone() },
            JSXName::Member { object, property } => format!("{}_{}", to_snake_case(object), to_snake_case(property)),
            JSXName::Namespaced { ns, name } => format!("{}_{}", to_snake_case(ns), to_snake_case(name)),
            JSXName::Dynamic(_) => "dynamic_component".to_string(),
            JSXName::Fragment => String::new(),
        }
    }

    fn transform_attrs(attrs: &[JSXAttr]) -> String {
        attrs.iter().filter_map(|attr| match attr {
            JSXAttr::Attr { name, value } => { let rust_name = Self::jsx_attr_to_rust(name); let v = match value { Some(JSXAttrValue::String(s)) => format!("{:?}", s), Some(JSXAttrValue::Expr(e)) => Self::transform_expr(e), None => "true".to_string() }; Some(format!("{}={}", rust_name, v)) }
            _ => None,
        }).collect::<Vec<_>>().join(" ")
    }

    fn jsx_attr_to_rust(name: &str) -> String {
        let name = name.replace("className", "class").replace("htmlFor", "for");
        if name.starts_with("on") { name.replace("onClick", "on_click").replace("onChange", "on_change").replace("onSubmit", "on_submit") } else { name }
    }

    fn transform_children(children: &[JSXChild]) -> String { children.iter().filter_map(|c| Self::transform_child(c)).collect::<Vec<_>>().join(" ") }
    fn transform_child(child: &JSXChild) -> Option<String> { match child { JSXChild::Text(s) => Some(s.trim().to_string()), JSXChild::Expr(e) => Some(Self::transform_expr(e)), JSXChild::JSX(j) => Some(Self::transform_jsx_element(j)), _ => None } }
    fn transform_expr(expr: &Expr) -> String { match expr { Expr::Ident { name } => name.clone(), Expr::String(s) => format!("{:?}", s), Expr::Number(n) => n.to_string(), Expr::Boolean(b) => b.to_string(), _ => "{{}}".to_string() } }
    fn transform_template(template: &TemplateExpr) -> String { "template".to_string() }
    fn transform_object(props: &[ObjectProp]) -> String { "object".to_string() }
    fn type_to_rust(ty: &Type) -> String { match ty { Type::String => "String".to_string(), Type::Number => "f64".to_string(), Type::Boolean => "bool".to_string(), _ => "()".to_string() } }
}
