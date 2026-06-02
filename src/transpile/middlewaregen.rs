//! Middleware chain generation

use super::hir::*;

/// Describes a single middleware function extracted from a module.
///
/// Middleware in Fresh is exported as either `export const handler`
/// (per-route, with `(req, ctx, next) => ...` shape) or as a
/// `_middleware.ts` file (applies to the route's directory and
/// children, also `(req, ctx, next) => ...`).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MiddlewareInfo {
    pub path: Option<String>,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_async: bool,
    pub is_default: bool,
}

#[allow(dead_code)]
impl MiddlewareInfo {
    /// True if this middleware is the directory-scoped kind
    /// (`_middleware.ts`) — it has the implicit `next` parameter
    /// and wraps the downstream handler. False if it's an inline
    /// `handler = (req, ctx, next) => ...` defined on a route
    /// file.
    pub fn is_global(&self) -> bool {
        self.path.as_deref() == Some("_middleware")
    }
}

/// Extract middleware definitions from a module.
///
/// Mirrors `extract_handlers` but for `handler = (req, ctx, next) => ...`
/// or the `_middleware.ts` shape. The caller passes the file path
/// so we can distinguish the two.
pub fn extract_middleware(module: &Module, file_path: &str) -> Vec<MiddlewareInfo> {
    let is_middleware_file = is_middleware_file_path(file_path);
    let mut out = Vec::new();
    for item in &module.items {
        let Some(vd) = handler_var_decl(item) else {
            continue;
        };
        let Some(init) = &vd.init else { continue };
        if let Some(info) = middleware_from_init(init, is_middleware_file) {
            out.push(info);
        }
    }
    out
}

/// True if the file path points to a `_middleware.ts` file.
fn is_middleware_file_path(file_path: &str) -> bool {
    file_path
        .split('/')
        .next_back()
        .map(|f| f.starts_with("_middleware"))
        .unwrap_or(false)
}

/// If `item` is a `Decl::Variable` named "handler", return it.
fn handler_var_decl(item: &ModuleItem) -> Option<&VariableDecl> {
    match item {
        ModuleItem::Decl(Decl::Variable(vd)) if vd.name == "handler" => Some(vd),
        _ => None,
    }
}

/// Convert a handler init expression to a `MiddlewareInfo`, if its
/// shape is recognised.
fn middleware_from_init(init: &Expr, is_middleware_file: bool) -> Option<MiddlewareInfo> {
    let path = if is_middleware_file {
        Some("_middleware".to_string())
    } else {
        None
    };
    match init {
        Expr::ArrowFunction { params, .. } => Some(MiddlewareInfo {
            path,
            params: params.clone(),
            body: vec![],
            is_async: true,
            is_default: false,
        }),
        Expr::Function(fd) => Some(MiddlewareInfo {
            path,
            params: fd.params.clone(),
            body: fd.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
            is_async: fd.is_async,
            is_default: false,
        }),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn generate_middleware(
    middleware: &MiddlewareInfo,
    _is_global: bool,
) -> anyhow::Result<String> {
    // Minimal stub: emit a `tower::Layer`-friendly wrapper around
    // the handler signature. Downstream code can refine this into a
    // real axum middleware by adding `next.run(req)` plumbing.
    let name = middleware
        .path
        .clone()
        .unwrap_or_else(|| "inline_middleware".to_string());
    let params: Vec<String> = middleware
        .params
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let params_str = params.join(", ");
    let is_async = if middleware.is_async { "async " } else { "" };
    Ok(format!(
        "pub {}fn {}({}) {{ /* middleware body */ }}",
        is_async, name, params_str
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use super::super::hir::{
        Block, Decl, Expr, Module, ModuleItem, VariableDecl, VariableKind,
    };

    fn module_with_items(items: Vec<ModuleItem>) -> Module {
        Module {
            source: String::new(),
            items,
            types: HashMap::new(),
        }
    }

    fn handler_var(init: Expr) -> ModuleItem {
        ModuleItem::Decl(Decl::Variable(VariableDecl {
            name: "handler".to_string(),
            kind: VariableKind::Const,
            type_: None,
            init: Some(init),
            pattern: None,
        }))
    }

    #[test]
    fn extract_middleware_empty_module() {
        let module = module_with_items(vec![]);
        assert!(extract_middleware(&module, "routes/index.tsx").is_empty());
    }

    #[test]
    fn extract_middleware_picks_arrow_function() {
        // `_middleware.ts` is a typical file, with an arrow-function
        // handler that takes `(req, ctx, next)`.
        let params = three_param_middleware_params();
        let arrow = Expr::ArrowFunction {
            params,
            body: Box::new(Expr::Undefined),
            is_async: true,
        };
        let module = module_with_items(vec![handler_var(arrow)]);
        let middleware = extract_middleware(&module, "routes/_middleware.ts");
        assert_eq!(middleware.len(), 1);
        assert!(middleware[0].is_global());
        assert!(middleware[0].is_async);
        assert_eq!(middleware[0].params.len(), 3);
        assert_eq!(middleware[0].params[2].name, "next");
    }

    fn three_param_middleware_params() -> Vec<Param> {
        vec![
            param("req"),
            param("ctx"),
            param("next"),
        ]
    }

    fn param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_: None,
            default: None,
            optional: false,
            pattern: None,
            ownership: Default::default(),
        }
    }

    #[test]
    fn extract_middleware_inline_handler() {
        // `export const handler = async (req, ctx, next) => ...` on a
        // regular route file. The file path does NOT end in
        // _middleware, so `path` is None and `is_global()` is false.
        let arrow = Expr::ArrowFunction {
            params: vec![
                Param {
                    name: "req".to_string(),
                    type_: None,
                    default: None,
                    optional: false,
                    pattern: None,
                    ownership: Default::default(),
                },
            ],
            body: Box::new(Expr::Undefined),
            is_async: true,
        };
        let module = module_with_items(vec![handler_var(arrow)]);
        let middleware = extract_middleware(&module, "routes/about.tsx");
        assert_eq!(middleware.len(), 1);
        assert!(!middleware[0].is_global());
    }

    #[test]
    fn extract_middleware_picks_function_expression() {
        // `export const handler = async function(req, ctx, next) {}`
        let func = Expr::Function(FunctionDecl {
            name: String::new(),
            generics: vec![],
            params: vec![],
            return_type: None,
            body: Some(Block(vec![])),
            is_async: true,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        });
        let module = module_with_items(vec![handler_var(func)]);
        let middleware = extract_middleware(&module, "routes/_middleware.ts");
        assert_eq!(middleware.len(), 1);
        assert!(middleware[0].is_global());
    }

    #[test]
    fn extract_middleware_ignores_non_handler_variable() {
        // `export const config = ...` should be ignored.
        let module = module_with_items(vec![ModuleItem::Decl(Decl::Variable(
            VariableDecl {
                name: "config".to_string(),
                kind: VariableKind::Const,
                type_: None,
                init: Some(Expr::Object { members: vec![] }),
                pattern: None,
            },
        ))]);
        assert!(extract_middleware(&module, "routes/_middleware.ts").is_empty());
    }

    #[test]
    fn generate_middleware_emits_function_signature() {
        let mw = MiddlewareInfo {
            path: Some("logger".to_string()),
            params: vec![
                Param {
                    name: "req".to_string(),
                    type_: None,
                    default: None,
                    optional: false,
                    pattern: None,
                    ownership: Default::default(),
                },
                Param {
                    name: "next".to_string(),
                    type_: None,
                    default: None,
                    optional: false,
                    pattern: None,
                    ownership: Default::default(),
                },
            ],
            body: vec![],
            is_async: true,
            is_default: false,
        };
        let code = generate_middleware(&mw, true).unwrap();
        assert!(code.contains("async fn logger"));
        assert!(code.contains("req, next"));
    }
}

