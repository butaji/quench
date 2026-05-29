//! JSX to HTML renderer
//! 
//! Renders HIR JSX AST to HTML strings for SSR.

use crate::transpile::hir::{
    JSXChild, JSXName, JSXOpening, JSXClosing, JSXAttr, JSXAttrValue,
};

/// Render JSX to HTML string
pub fn render_jsx(
    name: &JSXName,
    opening: &JSXOpening,
    closing: Option<&JSXClosing>,
    children: &[JSXChild],
) -> String {
    let tag = jsx_name_to_string(name);
    let attrs = render_attrs(&opening.attrs);
    
    let mut html = String::new();
    
    // Fragment renders children only
    if tag == ">" {
        return children.iter().map(render_child).collect();
    }
    
    // Check if should be self-closing
    let self_close = opening.self_closing || is_void_element(&tag);
    
    // Opening tag with attributes
    if attrs.is_empty() {
        html.push_str(&format!("<{}>", tag));
    } else {
        html.push_str(&format!("<{} {}>", tag, attrs));
    }
    
    if !self_close {
        // Render children
        for child in children {
            html.push_str(&render_child(child));
        }
        // Closing tag
        if let Some(c) = closing {
            html.push_str(&format!("</{}>", jsx_name_to_string(&c.name)));
        } else {
            html.push_str(&format!("</{}>", tag));
        }
    }
    
    html
}

fn jsx_name_to_string(name: &JSXName) -> String {
    match name {
        JSXName::Ident(s) => s.clone(),
        JSXName::Member { object, property } => format!("{}.{}", object, property),
        JSXName::Namespaced { ns, name } => format!("{}:{}", ns, name),
        JSXName::Dynamic(_) => "[dynamic]".to_string(),
        JSXName::Fragment => ">".to_string(),
    }
}

fn render_attrs(attrs: &[JSXAttr]) -> String {
    attrs.iter().filter_map(render_attr).collect::<Vec<_>>().join("")
}

fn render_attr(attr: &JSXAttr) -> Option<String> {
    match attr {
        JSXAttr::Spread { .. } => None,
        JSXAttr::Attr { name, value } => Some(format_attr(name, value)),
    }
}

fn format_attr(name: &str, value: &Option<JSXAttrValue>) -> String {
    let val = attr_value_to_string(value, name);
    let rendered = if name == "className" { format!(" class=\"{}\"", val) }
                   else if name.starts_with("on") { String::new() }
                   else { format!(" {}=\"{}\"", to_kebab_case(name), val) };
    if name == "style" { format!(" style=\"{}\"", val) } else { rendered }
}

fn attr_value_to_string(value: &Option<JSXAttrValue>, name: &str) -> String {
    match value {
        Some(JSXAttrValue::String(s)) => s.clone(),
        Some(JSXAttrValue::Expr(_)) => "[expr]".to_string(),
        _ => name.to_string(),
    }
}

fn render_child(child: &JSXChild) -> String {
    match child {
        JSXChild::Text(s) => escape_html(s),
        JSXChild::Expr(_) => "[expr]".to_string(),
        JSXChild::JSX(jsx) => {
            render_jsx(&jsx.opening.name, &jsx.opening, jsx.closing.as_ref(), &jsx.children)
        }
        JSXChild::Fragment { children } => children.iter().map(render_child).collect(),
        JSXChild::Spread { .. } => String::new(),
    }
}

fn render_children_inline(children: &[JSXChild]) -> String {
    children.iter().map(render_child).collect()
}

fn is_void_element(tag: &str) -> bool {
    matches!(tag, "area" | "base" | "br" | "col" | "embed" | "hr" | "img" 
             | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr")
}

fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpile::hir::{JSXName, JSXOpening};
    
    #[test]
    fn test_basic_element() {
        let opening = JSXOpening {
            name: JSXName::Ident("div".to_string()),
            attrs: vec![],
            self_closing: false,
        };
        let result = render_jsx(
            &opening.name,
            &opening,
            None,
            &[JSXChild::Text("Hello".to_string())],
        );
        assert_eq!(result, "<div>Hello</div>");
    }
    
    #[test]
    fn test_self_closing() {
        let opening = JSXOpening {
            name: JSXName::Ident("br".to_string()),
            attrs: vec![],
            self_closing: true,
        };
        let result = render_jsx(&opening.name, &opening, None, &[]);
        assert_eq!(result, "<br>");
    }
    
    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<div>"), "&lt;div&gt;");
    }
}
