//! Route handler code generation
//!
//! Transforms Fresh-style route files into Axum handlers:
//!
//! ```typescript
//! // routes/blog/[slug].tsx
//! export const handler = {
//!     GET: async (req, ctx) => {
//!         const post = await getPost(ctx.params.slug);
//!         return ctx.render({ post });
//!     }
//! };
//!
//! export default function BlogPost({ data }: PageProps) {
//!     return <article>{data.post.title}</article>;
//! }
//! ```
//!
//! Becomes:
//!
//! ```rust
//! pub async fn blog_slug_GET(req: Request, params: RouteParams) -> Response {
//!     let post = get_post(&params.slug).await;
//!     html! {
//!         <article>{ post.title }</article>
//!     }.render()
//! }
//! ```

use super::hir::*;
use anyhow::Result;

/// Route method
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
}

/// Route handler information
#[derive(Debug, Clone)]
pub struct RouteHandler {
    pub method: RouteMethod,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_async: bool,
}

/// Route information extracted from file path
#[derive(Debug, Clone)]
pub struct RouteInfo {
    /// The file path pattern (e.g., "blog/[slug]")
    pub pattern: String,
    
    /// The URL path (e.g., "/blog/:slug")
    pub path: String,
    
    /// Dynamic segments (e.g., ["slug"])
    pub segments: Vec<String>,
    
    /// HTTP method handlers
    pub handlers: Vec<RouteHandler>,
    
    /// Default export component (if any)
    pub component: Option<String>,
    
    /// File path
    pub file_path: String,
}

/// Parse a route file path into route info
pub fn parse_route_path(path: &str) -> RouteInfo {
    // Remove leading/trailing slashes
    let path = path.trim_matches('/');
    
    let mut segments = Vec::new();
    let mut url_path = String::new();
    
    for segment in path.split('/') {
        // Strip file extension for the last segment
        let clean_segment = if segment.ends_with(".tsx") || segment.ends_with(".ts") {
            &segment[..segment.len() - 4]
        } else {
            segment
        };
        
        if clean_segment.starts_with('[') && clean_segment.ends_with(']') {
            // Dynamic segment: [slug] -> :slug
            let param_name = &clean_segment[1..clean_segment.len()-1];
            segments.push(param_name.to_string());
            
            // Check for catch-all: [...slug]
            if param_name.starts_with("...") {
                url_path.push_str(&format!("/{{{}}}", &param_name[3..]));
            } else {
                url_path.push_str(&format!("/:{}", param_name));
            }
        } else {
            url_path.push_str(&format!("/{}", clean_segment));
        }
    }
    
    RouteInfo {
        pattern: path.to_string(),
        path: url_path,
        segments,
        handlers: Vec::new(),
        component: None,
        file_path: path.to_string(),
    }
}

/// Generate Axum route handler from route info
pub fn generate_route_handlers(route: &RouteInfo) -> Result<String> {
    let mut output = String::new();
    
    // Generate handler structs
    for handler in &route.handlers {
        let method_str = format!("{:?}", handler.method);
        let fn_name = format!(
            "{}_{}",
            route.pattern.replace('/', "_").replace('[', "_").replace(']', "_"),
            method_str
        );
        
        // Generate params struct
        let _params_struct = generate_params_struct(&route.segments);
        
        // Generate function
        let fn_sig = generate_handler_fn(handler, &route.segments);
        
        output.push_str(&format!(
            r#"
// {} {} handler
#[derive(serde::Deserialize)]
struct {} {{
    {}
}}

{}
pub async fn {}(req: Request, params: {}) -> Response {{
    // Auto-generated handler stub — implement your logic here
    let body = "Handler stub: not yet implemented".to_string();
    Response::builder()
        .status(501)
        .header("Content-Type", "text/plain")
        .body(Body::from(body))
        .unwrap()
}}
"#,
            route.path,
            method_str,
            format!("{}Params", fn_name.replace('-', "_")),
            route.segments.iter().map(|s| format!("    pub {}: String", s)).collect::<Vec<_>>().join(",\n"),
            fn_sig,
            fn_name,
            format!("{}Params", fn_name.replace('-', "_"))
        ));
    }
    
    // Generate component function
    if let Some(component_name) = &route.component {
        output.push_str(&generate_component_wrapper(component_name, &route.segments));
    }
    
    Ok(output)
}

pub fn generate_params_struct(segments: &[String]) -> String {
    if segments.is_empty() {
        return "pub struct RouteParams;".to_string();
    }
    
    let fields: Vec<String> = segments.iter()
        .map(|s| format!("    pub {}: String", s))
        .collect();
    
    format!(
        "#[derive(serde::Deserialize)]\npub struct RouteParams {{\n{}\n}}",
        fields.join(",\n")
    )
}

fn generate_handler_fn(handler: &RouteHandler, _segments: &[String]) -> String {
    let async_prefix = if handler.is_async { "async " } else { "" };
    
    format!(
        "#[axum::handler]\n{}fn handler(req: Request, ctx: HandlerContext) -> impl IntoResponse",
        async_prefix
    )
}

fn generate_component_wrapper(component_name: &str, segments: &[String]) -> String {
    let props_type = format!("{}Props", component_name);
    let _params_type = if segments.is_empty() {
        "()".to_string()
    } else {
        format!("RouteParams")
    };
    
    format!(
        r#"
/// Component for route {}
#[component]
pub fn {}(props: {}) -> VNode {{
    // Auto-generated component stub
    html!(<div class_name = "placeholder">"Component {} - not yet implemented"</div>)
}}
"#,
        format!("/{}", segments.join("/")),
        to_snake_case(component_name),
        props_type,
        component_name
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

/// Extract handlers from a module's exports
pub fn extract_handlers(module: &Module) -> Vec<RouteHandler> {
    let mut handlers = Vec::new();
    
    for item in &module.items {
        if let ModuleItem::Export(export) = item {
            // Try to find handler export
            let handler_object = match export {
                Export::NamedWithValue { name, value } if name == "handler" => {
                    if let Expr::Object { props } = value {
                        Some(props)
                    } else {
                        None
                    }
                }
                Export::Named { name, .. } if name == "handler" => None,
                Export::Default { expr } => {
                    // Check if this is a handler export
                    if let Expr::Object { props } = expr {
                        // Check if it looks like a handler (has GET/POST/etc)
                        if props.iter().any(|p| {
                            if let ObjectProp::Init { key: PropKey::Ident(s), .. } = p {
                                matches!(s.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH")
                            } else {
                                false
                            }
                        }) {
                            Some(props)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            
            if let Some(props) = handler_object {
                for prop in props {
                    if let ObjectProp::Init { key: PropKey::Ident(method), value } = prop {
                        if let Some(route_method) = RouteMethod::from_str(method) {
                            if let Expr::Arrow { params, body, is_async } = value {
                                let body_stmts = match body.as_ref() {
                                    Stmt::Block(stmts) => stmts.clone(),
                                    _ => vec![body.as_ref().clone()],
                                };
                                handlers.push(RouteHandler {
                                    method: route_method,
                                    params: params.clone(),
                                    body: body_stmts,
                                    is_async: *is_async,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    
    handlers
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_route_path() {
        let route = parse_route_path("blog/[slug]");
        assert_eq!(route.pattern, "blog/[slug]");
        assert_eq!(route.path, "/blog/:slug");
        assert_eq!(route.segments, vec!["slug"]);
    }
    
    #[test]
    fn test_parse_route_path_nested() {
        let route = parse_route_path("products/[category]/[id]");
        assert_eq!(route.path, "/products/:category/:id");
        assert_eq!(route.segments, vec!["category", "id"]);
    }
    
    #[test]
    fn test_parse_route_path_catch_all() {
        let route = parse_route_path("api/[...path]");
        // Full path including the route prefix
        assert_eq!(route.path, "/api/{path}");
        // Segments stores the raw param name including ... for catch-all
        assert!(route.segments.contains(&"...path".to_string()));
    }
}
