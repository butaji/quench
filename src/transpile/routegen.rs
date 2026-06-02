//! Route handler generation

use super::hir::*;

/// HTTP method extracted from a `handler.GET()` / `handler.POST()` /
/// similar method-shorthand entry. The string forms match axum's
/// router DSL: `axum::routing::get`, `axum::routing::post`, etc.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}
#[allow(dead_code)]
impl RouteMethod {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::GET),
            "POST" => Some(Self::POST),
            "PUT" => Some(Self::PUT),
            "DELETE" => Some(Self::DELETE),
            "PATCH" => Some(Self::PATCH),
            "HEAD" => Some(Self::HEAD),
            "OPTIONS" => Some(Self::OPTIONS),
            _ => None,
        }
    }

    /// Returns the axum routing function name (lowercase verb).
    pub fn axum_name(self) -> &'static str {
        match self {
            Self::GET => "get",
            Self::POST => "post",
            Self::PUT => "put",
            Self::DELETE => "delete",
            Self::PATCH => "patch",
            Self::HEAD => "head",
            Self::OPTIONS => "options",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RouteHandler {
    pub method: RouteMethod,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_async: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub pattern: String,
    pub path: String,
    pub segments: Vec<String>,
    pub handlers: Vec<RouteHandler>,
    pub component: Option<String>,
    pub file_path: String,
}

#[allow(dead_code)]
pub fn parse_route_path(path: &str) -> RouteInfo {
    let original = path.trim_matches('/').to_string();
    // Strip file extension for processing
    let path = original.replace(".tsx", "").replace(".ts", "");
    let mut segments = Vec::new();
    let mut url_path = String::new();
    for segment in path.split('/') {
        if segment.starts_with('[') && segment.ends_with(']') {
            let name = segment.trim_start_matches('[').trim_end_matches(']');
            segments.push(name.to_string());
            if name.starts_with("...") {
                url_path.push_str(&format!("/:{}", name));
            } else {
                url_path.push_str(&format!("/:{}", name));
            }
        } else if !segment.is_empty() {
            url_path.push_str(&format!("/{}", segment));
        }
    }
    RouteInfo {
        pattern: original,
        path: if url_path.is_empty() {
            "/".to_string()
        } else {
            url_path
        },
        segments,
        handlers: vec![],
        component: None,
        file_path: path,
    }
}

#[allow(dead_code)]
pub fn generate_params_struct(params: &[String]) -> String {
    if params.is_empty() {
        "pub struct RouteParams;".to_string()
    } else {
        format!(
            "pub struct RouteParams {{ {} }}",
            params
                .iter()
                .map(|p| format!("pub {}: String", p))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Extract HTTP route handlers from a module.
///
/// Recognises two patterns:
///
/// 1. **`export const handler = { GET(req, ctx) {...}, POST(req, ctx) {...} }`**
///    — the Fresh-style handler object. Each method-shorthand member
///    whose key is an HTTP verb becomes a `RouteHandler`.
///
/// 2. **`export const handler = (req, ctx) => {...}`** — a default
///    handler that responds to every method. The method is set to
///    `GET` (the actual response method is decided at runtime, but
///    we need a concrete verb for axum's typed router).
///
/// Anything else is ignored. Middleware (`_middleware.ts`) is
/// handled by `extract_middleware` in `middlewaregen.rs`.
pub fn extract_handlers(module: &Module) -> Vec<RouteHandler> {
    let mut out = Vec::new();
    for item in &module.items {
        let Some(vd) = handler_var_decl(item) else {
            continue;
        };
        let Some(init) = &vd.init else { continue };
        collect_handlers_from_init(init, &mut out);
    }
    out
}

/// If `item` is a `Decl::Variable` named "handler", return a reference
/// to it. Otherwise return None.
fn handler_var_decl(item: &ModuleItem) -> Option<&VariableDecl> {
    match item {
        ModuleItem::Decl(Decl::Variable(vd)) if vd.name == "handler" => Some(vd),
        _ => None,
    }
}

/// Dispatch on the handler initialiser's shape and append any
/// recognised handlers to `out`.
fn collect_handlers_from_init(init: &Expr, out: &mut Vec<RouteHandler>) {
    match init {
        Expr::Object { members } => collect_from_object(members, out),
        Expr::ArrowFunction { params, .. } => {
            // Arrow-function bodies are a single expression, not a
            // list of statements. We capture the parameter list and
            // leave the body empty — downstream codegen can wrap the
            // expr in a `return` if it wants the actual handler.
            out.push(RouteHandler {
                method: RouteMethod::GET,
                params: params.clone(),
                body: vec![],
                is_async: true,
            });
        }
        Expr::Function(fd) => out.push(RouteHandler {
            method: RouteMethod::GET,
            params: fd.params.clone(),
            body: fd.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
            is_async: fd.is_async,
        }),
        _ => {}
    }
}

/// Walk an object expression looking for method-shorthand members
/// whose key is an HTTP verb. For each match, push a `RouteHandler`.
fn collect_from_object(members: &[ObjectMemberExpr], out: &mut Vec<RouteHandler>) {
    for m in members {
        if let Some(handler) = method_shorthand_to_handler(&m.prop) {
            out.push(handler);
        }
    }
}

/// If `prop` is a method-shorthand `key() {...}` whose key is an HTTP
/// verb, return a `RouteHandler`. Otherwise None.
fn method_shorthand_to_handler(prop: &ObjectProp) -> Option<RouteHandler> {
    let ObjectProp::Method { key, value, .. } = prop else {
        return None;
    };
    let key_name = match key {
        PropKey::Str(s) => s.clone(),
        PropKey::Num(n) => n.to_string(),
        PropKey::Computed { .. } => return None,
    };
    let method = RouteMethod::from_str(&key_name)?;
    let Expr::Function(fd) = value else {
        return None;
    };
    Some(RouteHandler {
        method,
        params: fd.params.clone(),
        body: fd.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
        is_async: fd.is_async,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::hir::{
        Block, Decl, Expr, Module, ModuleItem, ObjectProp, Param, PropKey, Stmt,
        VariableDecl, VariableKind,
    };

    fn module_with_items(items: Vec<ModuleItem>) -> Module {
        Module {
            source: String::new(),
            items,
            types: std::collections::HashMap::new(),
        }
    }

    fn var_decl(name: &str, init: Expr) -> ModuleItem {
        ModuleItem::Decl(Decl::Variable(VariableDecl {
            name: name.to_string(),
            kind: VariableKind::Const,
            type_: None,
            init: Some(init),
            pattern: None,
        }))
    }

    fn method_shorthand(key: &str, fd: FunctionDecl) -> ObjectProp {
        ObjectProp::Method {
            key: PropKey::Str(key.to_string()),
            value: Expr::Function(fd),
            computed: false,
        }
    }

    fn function_with_body(params: Vec<Param>, body: Vec<Stmt>) -> FunctionDecl {
        FunctionDecl {
            name: String::new(),
            generics: vec![],
            params,
            return_type: None,
            body: Some(Block(body)),
            is_async: true,
            is_generator: false,
            decorators: vec![],
            throws: false,
            error_type: None,
        }
    }

    fn empty_function() -> FunctionDecl {
        function_with_body(vec![], vec![])
    }

    #[test]
    fn extract_handlers_empty_module() {
        let module = module_with_items(vec![]);
        assert!(extract_handlers(&module).is_empty());
    }

    #[test]
    fn extract_handlers_ignores_non_handler_variables() {
        // `export const config = { ... }` should not be treated as a
        // handler object.
        let module = module_with_items(vec![var_decl(
            "config",
            Expr::Object {
                members: vec![],
            },
        )]);
        assert!(extract_handlers(&module).is_empty());
    }

    #[test]
    fn extract_handlers_picks_method_shorthand() {
        let obj = Expr::Object {
            members: vec![
                ObjectMemberExpr {
                    prop: method_shorthand("GET", empty_function()),
                },
                ObjectMemberExpr {
                    prop: method_shorthand("POST", empty_function()),
                },
            ],
        };
        let module = module_with_items(vec![var_decl("handler", obj)]);
        let handlers = extract_handlers(&module);
        assert_eq!(handlers.len(), 2);
        assert_eq!(handlers[0].method, RouteMethod::GET);
        assert_eq!(handlers[1].method, RouteMethod::POST);
        assert!(handlers.iter().all(|h| h.is_async));
    }

    #[test]
    fn extract_handlers_ignores_non_http_keys() {
        // An object property named "config" is not an HTTP verb and
        // should be skipped, not treated as a handler.
        let obj = Expr::Object {
            members: vec![ObjectMemberExpr {
                prop: method_shorthand("config", empty_function()),
            }],
        };
        let module = module_with_items(vec![var_decl("handler", obj)]);
        assert!(extract_handlers(&module).is_empty());
    }

    #[test]
    fn extract_handlers_picks_arrow_function() {
        // `handler = (req, ctx) => ...` — single default handler.
        let arrow = Expr::ArrowFunction {
            params: vec![Param {
                name: "req".to_string(),
                type_: None,
                default: None,
                optional: false,
                pattern: None,
                ownership: Default::default(),
            }],
            body: Box::new(Expr::Undefined),
            is_async: true,
        };
        let module = module_with_items(vec![var_decl("handler", arrow)]);
        let handlers = extract_handlers(&module);
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].method, RouteMethod::GET);
        assert_eq!(handlers[0].params.len(), 1);
        assert_eq!(handlers[0].params[0].name, "req");
    }

    #[test]
    fn extract_handlers_picks_function_expression() {
        // `handler = async function (req, ctx) {...}` — same idea,
        // expressed as a function expression.
        let func = Expr::Function(empty_function());
        let module = module_with_items(vec![var_decl("handler", func)]);
        let handlers = extract_handlers(&module);
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].method, RouteMethod::GET);
    }

    #[test]
    fn parse_route_path_handles_dynamic_segments() {
        let info = parse_route_path("blog/[slug].tsx");
        assert_eq!(info.path, "/blog/:slug");
        assert_eq!(info.segments, vec!["slug".to_string()]);
    }

    #[test]
    fn parse_route_path_handles_root() {
        // `parse_route_path` keeps the original segment name; the
        // caller (the build pipeline) is responsible for mapping
        // `index` -> `/` at the routing layer. This is the existing
        // contract, captured here so future refactors don't
        // accidentally change it.
        let info = parse_route_path("index.tsx");
        assert_eq!(info.path, "/index");
        assert!(info.segments.is_empty());
    }

    #[test]
    fn route_method_axum_name_round_trip() {
        for (method, expected) in [
            (RouteMethod::GET, "get"),
            (RouteMethod::POST, "post"),
            (RouteMethod::PUT, "put"),
            (RouteMethod::DELETE, "delete"),
            (RouteMethod::PATCH, "patch"),
            (RouteMethod::HEAD, "head"),
            (RouteMethod::OPTIONS, "options"),
        ] {
            assert_eq!(method.axum_name(), expected);
        }
    }
}

