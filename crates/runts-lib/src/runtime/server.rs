//! Server utilities

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageProps<T = serde_json::Value> {
    pub params: HashMap<String, String>,
    pub url: String,
    pub data: T,
}

impl<T> PageProps<T> {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct HandlerContext {
    pub params: HashMap<String, String>,
    pub url: String,
}
impl HandlerContext {
    pub fn new() -> Self {
        Self {
            params: HashMap::new(),
            url: String::new(),
        }
    }
}

pub use http::{Request, Response};
pub type Handler = Box<dyn Fn(Request<()>, HandlerContext) -> HandlerOutput + Send + Sync>;
pub type HandlerOutput =
    std::future::Ready<Result<http::Response<axum::body::Body>, std::convert::Infallible>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    GET,
    HEAD,
    POST,
    PUT,
    DELETE,
    CONNECT,
    OPTIONS,
    TRACE,
    PATCH,
}
impl Method {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::GET),
            "HEAD" => Some(Self::HEAD),
            "POST" => Some(Self::POST),
            "PUT" => Some(Self::PUT),
            "DELETE" => Some(Self::DELETE),
            _ => None,
        }
    }
}
