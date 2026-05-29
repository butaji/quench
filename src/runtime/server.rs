//! Server utilities

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageProps<T = serde_json::Value> {
    pub params: std::collections::HashMap<String, String>,
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
    pub params: std::collections::HashMap<String, String>,
    pub url: String,
}

impl HandlerContext {
    pub fn new() -> Self {
        Self {
            params: std::collections::HashMap::new(),
            url: String::new(),
        }
    }
}


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

pub struct PageResult {
    pub html: String,
    pub status: u16,
}
pub struct SsrEngine;

impl SsrEngine {
    pub fn new() -> Self {
        Self
    }
    pub fn render_page(&self, _title: &str, content: &str) -> PageResult {
        PageResult {
            html: format!("<html><body>{}</body></html>", content),
            status: 200,
        }
    }
}
