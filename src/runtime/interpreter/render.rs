//! Rendering utilities for interpreter
// allow:complexity

use crate::transpile::hir::{Expr, ModuleItem, Decl, FunctionDecl, Stmt};

use super::*;

/// Render a value to HTML string
pub fn render_value(value: &Value) -> String {
    match value {
        Value::Null | Value::Undefined => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => html_escape(s),
        Value::Array(arr) => arr.iter().map(render_value).collect(),
        Value::Object(map) => {
            let inner: String = map.values().map(render_value).collect();
            format!("<span>{}</span>", inner)
        }
        Value::VNode(vnode) => render_vnode(vnode),
        Value::Function(_) => String::new(),
    }
}

fn render_vnode(vnode: &VNodeValue) -> String {
    let attrs: String = vnode
        .attrs
        .iter()
        .map(|(k, v)| format!(" {}=\"{}\"", k, render_value(v)))
        .collect();
    let children: String = vnode.children.iter().map(render_value).collect();
    if children.is_empty() {
        format!("<{}{} />", vnode.tag, attrs)
    } else {
        format!("<{}{}>{}</{}>", vnode.tag, attrs, children, vnode.tag)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Render a component's body
pub fn render_component_body(body: &[Stmt], ctx: &EvalContext) -> String {
    body.iter().map(|s| render_stmt(s, ctx)).collect()
}

/// Render a statement to HTML
fn render_stmt(stmt: &Stmt, ctx: &EvalContext) -> String {
    match stmt {
        Stmt::Return { arg: Some(expr) } => format!("{{{{{}}}}}", render_expr(expr, ctx)),
        Stmt::Block(stmts) => stmts.iter().map(|s| render_stmt(s, ctx)).collect(),
        _ => String::new(),
    }
}

/// Render an expression
fn render_expr(expr: &Expr, ctx: &EvalContext) -> String {
    match expr {
        Expr::String(s) => s.clone(),
        Expr::Ident { name } => ctx.scope.get(name).map(|v| format!("{}", v)).unwrap_or_else(|| format!("{{{}}}", name)),
        _ => String::new(),
    }
}

/// Execute a route from module items
pub fn execute_module_items(items: &[ModuleItem], ctx: &EvalContext) -> String {
    let mut html = String::new();
    for item in items {
        if let ModuleItem::Decl(Decl::Function(func)) = item {
            if func.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                html.push_str(&format!("<div data-component=\"{}\">", func.name));
                if let Some(body) = &func.body {
                    html.push_str(&render_component_body(&body.0, ctx));
                }
                html.push_str("</div>");
            }
        }
    }
    html
}
