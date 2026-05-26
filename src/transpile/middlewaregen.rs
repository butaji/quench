//! Middleware chain generation
//!
//! Transforms Fresh-style `_middleware.ts` files into Axum middleware:
//!
//! ```typescript
//! // routes/_middleware.ts
//! import { FreshContext } from "$fresh/server.ts";
//!
//! export default async function handler(
//!   req: Request,
//!   ctx: FreshContext,
//! ) {
//!   // Add request ID
//!   ctx.state.requestId = crypto.randomUUID();
//!   
//!   // Continue to handler
//!   return await ctx.next();
//! }
//! ```
//!
//! Becomes:
//!
//! ```rust
//! pub async fn middleware(
//!     request: Request,
//!     next: Next,
//! ) -> Response {
//!     let mut request = request;
//!     
//!     // Add request ID
//!     let request_id = Uuid::new_v4().to_string();
//!     request.extensions_mut().insert(RequestId(request_id));
//!     
//!     // Call next middleware/handler
//!     next.run(request).await
//! }
//! ```

use super::hir::*;
use anyhow::{anyhow, Result};

/// Middleware information
#[derive(Debug, Clone)]
pub struct MiddlewareInfo {
    /// The path pattern (e.g., "blog" for routes/blog/_middleware.ts)
    pub path: Option<String>,
    
    /// The function parameters
    pub params: Vec<Param>,
    
    /// The function body statements
    pub body: Vec<Stmt>,
    
    /// Whether it's async
    pub is_async: bool,
    
    /// Is this the default export?
    pub is_default: bool,
}

/// Extract middleware from a module
pub fn extract_middleware(module: &Module) -> Vec<MiddlewareInfo> {
    let mut middlewares = Vec::new();
    
    for item in &module.items {
        match item {
            ModuleItem::Export(export) => {
                match export {
                    Export::Default { expr } => {
                        if let Expr::Function { decl } = expr.as_ref() {
                            middlewares.push(MiddlewareInfo {
                                path: None,
                                params: decl.params.clone(),
                                body: decl.body.as_ref()
                                    .map(|b| b.0.clone())
                                    .unwrap_or_default(),
                                is_async: decl.is_async,
                                is_default: true,
                            });
                        }
                    }
                    Export::Named { name, expr } => {
                        if name == "handler" || name.ends_with("Handler") {
                            if let Expr::Function { decl } = expr.as_ref() {
                                middlewares.push(MiddlewareInfo {
                                    path: None,
                                    params: decl.params.clone(),
                                    body: decl.body.as_ref()
                                        .map(|b| b.0.clone())
                                        .unwrap_or_default(),
                                    is_async: decl.is_async,
                                    is_default: false,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            ModuleItem::Decl(Decl::Function(f)) => {
                if f.name.ends_with("Middleware") || f.name.ends_with("Handler") {
                    middlewares.push(MiddlewareInfo {
                        path: None,
                        params: f.params.clone(),
                        body: f.body.as_ref()
                            .map(|b| b.0.clone())
                            .unwrap_or_default(),
                        is_async: f.is_async,
                        is_default: false,
                    });
                }
            }
            _ => {}
        }
    }
    
    middlewares
}

/// Generate Axum middleware from middleware info
pub fn generate_middleware(middleware: &MiddlewareInfo, is_global: bool) -> Result<String> {
    let async_prefix = if middleware.is_async { "async " } else { "" };
    
    // Check for ctx.next() call to determine middleware type
    let calls_next = middleware.body.iter().any(|stmt| {
        contains_next_call(stmt)
    });
    
    if calls_next {
        generate_middleware_fn(middleware, is_global)
    } else {
        generate_handler_fn(middleware)
    }
}

fn contains_next_call(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr { expr } => contains_next_in_expr(expr),
        Stmt::Block(stmts) => stmts.0.iter().any(contains_next_call),
        Stmt::If { test, consequent, alternate } => {
            contains_next_call(consequent) || 
            alternate.as_ref().map(|a| contains_next_call(a)).unwrap_or(false)
        }
        Stmt::While { body, .. } => contains_next_call(body),
        Stmt::For { body, .. } => contains_next_call(body),
        Stmt::ForIn { body, .. } => contains_next_call(body),
        Stmt::ForOf { body, .. } => contains_next_call(body),
        Stmt::Return { arg } => {
            arg.as_ref().map(|e| contains_next_in_expr(e)).unwrap_or(false)
        }
        _ => false,
    }
}

fn contains_next_in_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Call { callee, .. } => {
            if let Expr::Member { object, property, .. } = callee.as_ref() {
                if let Expr::Ident { name } = object.as_ref() {
                    if name == "ctx" || name == "context" {
                        if let Expr::Ident { name: prop_name } = property.as_ref() {
                            return prop_name == "next";
                        }
                    }
                }
            }
            false
        }
        Expr::Await { arg } => contains_next_in_expr(arg),
        Expr::Bin { left, right, .. } => {
            contains_next_in_expr(left) || contains_next_in_expr(right)
        }
        _ => false,
    }
}

fn generate_middleware_fn(middleware: &MiddlewareInfo, is_global: bool) -> String {
    let fn_name = if is_global {
        "global_middleware"
    } else {
        "route_middleware"
    };
    
    let async_prefix = if middleware.is_async { "async " } else { "async " }; // Middleware is always async
    
    // Extract state assignments from body
    let state_updates = extract_state_updates(&middleware.body);
    
    format!(
        r#"pub {async_prefix}fn {fn_name}(
    request: Request,
    next: Next,
) -> impl IntoResponse + Send + Sync {{
    let mut request = request;
    
    // State updates from middleware
    {}
    
    // Continue to next handler
    next.run(request).await
}}

fn extract_next(middleware: Vec<axum::middleware::FromRequestLayer>) -> axum::middleware::StackedFromRequestLayers {{
    // Combine all middleware layers
    axum::middleware::from_request_pipeline(move |request: Request, next: Next| async move {{
        next.run(request).await
    }})
}}"#,
        state_updates.join("\n    ")
    )
}

fn generate_handler_fn(middleware: &MiddlewareInfo) -> String {
    let async_prefix = if middleware.is_async { "async " } else { "async " };
    
    format!(
        r#"pub {async_prefix}fn middleware_handler(
    request: Request,
) -> impl IntoResponse + Send + Sync {{
    // Request handler
    // TODO: Implement handler logic
    todo!("Middleware handler")
}}"#
    )
}

fn extract_state_updates(stmts: &[Stmt]) -> Vec<String> {
    let mut updates = Vec::new();
    
    for stmt in stmts {
        if let Stmt::Expr { expr } = stmt {
            if let Expr::Assign { left, right, .. } = expr.as_ref() {
                if let Expr::Member { object, property, .. } = left.as_ref() {
                    if let Expr::Ident { name: obj_name } = object.as_ref() {
                        if obj_name == "ctx" || obj_name == "context" || obj_name == "state" {
                            if let Expr::Ident { name: prop_name } = property.as_ref() {
                                updates.push(format!(
                                    "    request.extensions_mut().insert({}::{}({:#?}));",
                                    to_pascal_case(prop_name),
                                    extract_rust_value(right)
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    
    updates
}

fn extract_rust_value(expr: &Expr) -> String {
    match expr {
        Expr::Call { callee, args, .. } => {
            let callee_str = match callee.as_ref() {
                Expr::Member { object, property, .. } => {
                    if let Expr::Ident { name: obj } = object.as_ref() {
                        if let Expr::Ident { name: prop } = property.as_ref() {
                            format!("{}.{}", obj, prop)
                        } else {
                            format!("{}.", obj)
                        }
                    } else {
                        "".to_string()
                    }
                }
                Expr::Ident { name } => name.clone(),
                _ => "".to_string(),
            };
            
            let args_str: Vec<String> = args.iter()
                .map(extract_rust_value)
                .collect();
            
            format!("{}({})", callee_str, args_str.join(", "))
        }
        Expr::Member { object, property, .. } => {
            let obj = match object.as_ref() {
                Expr::Ident { name } => name.clone(),
                _ => "?".to_string(),
            };
            let prop = match property.as_ref() {
                Expr::Ident { name } => name.clone(),
                _ => "?".to_string(),
            };
            format!("{}.{}", obj, prop)
        }
        Expr::Ident { name } => name.clone(),
        Expr::String(s) => format!("{:?}", s),
        Expr::Number(n) => n.to_string(),
        Expr::Boolean(b) => b.to_string(),
        Expr::Template { parts, exprs } => {
            let mut result = String::new();
            for (i, part) in parts.iter().enumerate() {
                if let TemplatePart::String(s) = part {
                    result.push_str(s);
                }
                if i < exprs.len() {
                    result.push_str(&format!("{{{}}}", extract_rust_value(&exprs[i])));
                }
            }
            format!("{:?}", result)
        }
        _ => "todo!()".to_string(),
    }
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    
    result
}

/// Generate layout wrapper component
pub fn generate_layout(child_name: &str, layout_name: &str) -> String {
    format!(
        r#"/// Layout wrapper for nested routes
pub fn {}_layout(child: VNode) -> VNode {{
    // Render the layout component wrapping the child
    html! {{
        <{}>
            {{ child }}
        </{}>
    }}
}}"#,
        child_name,
        to_snake_case(layout_name),
        to_snake_case(layout_name)
    )
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("request_id"), "RequestId");
        assert_eq!(to_pascal_case("user_id"), "UserId");
        assert_eq!(to_pascal_case("created_at"), "CreatedAt");
    }
}
