//! Middleware chain generation

use super::hir::*;

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
pub fn extract_middleware(_module: &Module) -> Vec<MiddlewareInfo> {
    vec![]
}
#[allow(dead_code)]
pub fn generate_middleware(
    _middleware: &MiddlewareInfo,
    _is_global: bool,
) -> anyhow::Result<String> {
    Ok(String::new())
}
