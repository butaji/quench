//! Middleware chain generation

use super::hir::*;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct MiddlewareInfo {
    pub path: Option<String>,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_async: bool,
    pub is_default: bool,
}

pub fn extract_middleware(module: &Module) -> Vec<MiddlewareInfo> {
    module.items.iter().filter_map(|item| match item {
        ModuleItem::Export(Export::Default { expr }) => extract_fn_info(expr, true),
        ModuleItem::Export(Export::NamedWithValue { name, value }) if name == "handler" || name.ends_with("Handler") => extract_fn_info(value, false),
        ModuleItem::Decl(Decl::Function(f)) if f.name.ends_with("Middleware") || f.name.ends_with("Handler") => Some(to_middleware_info(f, false)),
        _ => None,
    }).collect()
}

fn extract_fn_info(expr: &Expr, is_default: bool) -> Option<MiddlewareInfo> {
    if let Expr::Function { decl } = expr {
        Some(MiddlewareInfo { path: None, params: decl.params.clone(), body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(), is_async: decl.is_async, is_default })
    } else { None }
}

fn to_middleware_info(f: &FunctionDecl, is_default: bool) -> MiddlewareInfo {
    MiddlewareInfo { path: None, params: f.params.clone(), body: f.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(), is_async: f.is_async, is_default }
}

pub fn generate_middleware(middleware: &MiddlewareInfo, is_global: bool) -> Result<String> {
    if middleware.body.iter().any(|s| has_next_call(s)) { generate_middleware_fn(middleware, is_global) } else { generate_handler_fn(middleware) }
}

fn has_next_call(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr { expr } => expr_has_next(expr),
        Stmt::Block(stmts) => stmts.iter().any(has_next_call),
        Stmt::If { consequent, alternate, .. } => has_next_call(consequent) || alternate.as_ref().map_or(false, |a| has_next_call(a)),
        Stmt::While { body, .. } => has_next_call(body),
        Stmt::For { body, .. } => has_next_call(body),
        Stmt::Return { arg } => arg.as_ref().map_or(false, expr_has_next),
        _ => false,
    }
}

fn expr_has_next(expr: &Expr) -> bool {
    match expr { Expr::Call { callee, .. } => matches!(callee.as_ref(), Expr::Member { property, .. } if matches!(property.as_ref(), Expr::Ident(i) if i.name == "next")), _ => false }
}

fn generate_middleware_fn(middleware: &MiddlewareInfo, is_global: bool) -> Result<String> {
    let prefix = if is_global { "Global" } else { "" };
    Ok(format!("pub async fn {}middleware() -> Middleware {{ {}.into() }}", prefix, "axum::middleware::from_fn(|_: Request, next: Next| Box::pin(async move {{ next.run(_).await }))"))
}

fn generate_handler_fn(middleware: &MiddlewareInfo) -> Result<String> {
    Ok(format!("pub async fn handler() -> impl IntoResponse {{ Response::new(\"handler\") }}"))
}
