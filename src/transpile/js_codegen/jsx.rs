use super::super::hir::*;
use super::expr::expr_to_js;

pub fn jsx_to_js(jsx: &JSXExpr) -> String {
    let tag = match &jsx.opening.name { JSXName::Ident(s) => s.clone(), JSXName::Member { object, property } => format!("{}.{}", object, property), _ => "div".to_string() };
    let is_component = tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
    if is_component { let props = jsx_attrs_to_js(&jsx.opening.attrs); let children = jsx_children_to_js(&jsx.children); let all_props = if children.is_empty() || children == "null" { if props.is_empty() { "null".to_string() } else { format!("{{ {} }}", props) } } else { if props.is_empty() { format!("{{ children: {} }}", children) } else { format!("{{ {}, children: {} }}", props, children) } }; return format!("h({}, {})", tag, all_props); }
    let mut props = Vec::new();
    for attr in &jsx.opening.attrs { match attr { JSXAttr::Attr { name, value } => { let v = match value { Some(JSXAttrValue::String(s)) => format!("'{}'", s.replace('\'', "\\'")), Some(JSXAttrValue::Expr(e)) => expr_to_js(e), None => "true".to_string() }; let key = if name == "className" { "class" } else { name }; props.push(format!("'{}': {}", key, v)); } JSXAttr::Spread { expr } => props.push(format!("...{}", expr_to_js(expr))), _ => {} } }
    let children = jsx_children_to_js(&jsx.children); let props_str = if props.is_empty() { "null".to_string() } else { format!("{{ {} }}", props.join(", ")) };
    if children.is_empty() || children == "null" { format!("h('{}', {})", tag, props_str) } else { format!("h('{}', {}, {})", tag, props_str, children) }
}

pub fn jsx_attrs_to_js(attrs: &[JSXAttr]) -> String { attrs.iter().filter_map(|attr| match attr { JSXAttr::Attr { name, value } => { let v = match value { Some(JSXAttrValue::String(s)) => format!("'{}'", s.replace('\'', "\\'")), Some(JSXAttrValue::Expr(e)) => expr_to_js(e), None => "true".to_string() }; Some(format!("{}: {}", name, v)) } JSXAttr::Spread { expr } => Some(format!("...{}", expr_to_js(expr))), _ => None }).collect::<Vec<_>>().join(", ") }

pub fn jsx_children_to_js(children: &[JSXChild]) -> String { let items: Vec<String> = children.iter().filter_map(|c| jsx_child_to_js(c)).collect(); if items.is_empty() { "null".to_string() } else if items.len() == 1 { items.into_iter().next().unwrap() } else { format!("[{}]", items.join(", ")) } }

fn jsx_child_to_js(child: &JSXChild) -> Option<String> { match child { JSXChild::Text(s) => { let t = s.trim(); if t.is_empty() { None } else { Some(format!("'{}'", t.replace('\'', "\\'"))) } } JSXChild::Expr(e) => Some(expr_to_js(e)), JSXChild::JSX(j) => Some(jsx_to_js(j)), JSXChild::Fragment { children } => Some(jsx_children_to_js(children)), _ => None } }
