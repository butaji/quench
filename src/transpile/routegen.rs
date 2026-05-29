//! Route handler generation

use super::hir::*;

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

#[derive(Debug, Clone)]
pub struct RouteHandler {
    pub method: RouteMethod,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_async: bool,
}

#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub pattern: String,
    pub path: String,
    pub segments: Vec<String>,
    pub handlers: Vec<RouteHandler>,
    pub component: Option<String>,
    pub file_path: String,
}

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

pub fn extract_handlers(_module: &Module) -> Vec<RouteHandler> {
    vec![]
}
