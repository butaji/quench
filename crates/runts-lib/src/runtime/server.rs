//! Server utilities for Fresh compatibility
//!
//! This module provides types and utilities for building
//! Fresh-compatible route handlers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use http::{Response, StatusCode, header};

/// Page props - passed to page components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageProps<T = serde_json::Value> {
    /// Route params (e.g., { slug: "my-post" })
    pub params: HashMap<String, String>,
    /// URL
    pub url: String,
    /// Page data (returned from handler)
    pub data: T,
}

impl<T> PageProps<T> {
    /// Get a param by name
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }
}

/// Handler context - context passed to route handlers
#[derive(Debug, Clone)]
pub struct HandlerContext {
    /// Route params
    pub params: HashMap<String, String>,
    /// Request URL
    pub url: String,
    /// Request method
    pub method: String,
    /// Route state (shared between middleware)
    pub state: HashMap<String, serde_json::Value>,
}

impl HandlerContext {
    /// Create a new handler context
    pub fn new(url: String, method: String) -> Self {
        Self {
            params: HashMap::new(),
            url,
            method,
            state: HashMap::new(),
        }
    }
    
    /// Get a route param
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }
    
    /// Get state value
    pub fn state<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.state.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    
    /// Call the next handler (for middleware)
    pub async fn next(&self) -> Response<String> {
        Response::builder()
            .status(500)
            .body("Internal Server Error".to_string())
            .unwrap()
    }
    
    /// Render a not found response
    pub fn render_not_found(&self) -> Response<String> {
        Response::builder()
            .status(404)
            .body("Not Found".to_string())
            .unwrap()
    }
    
    /// Render with data
    pub fn render<T: Serialize>(&self, data: T) -> Response<String> {
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&data).unwrap_or_default())
            .unwrap()
    }
}

/// HTTP Method
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
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::GET),
            "HEAD" => Some(Self::HEAD),
            "POST" => Some(Self::POST),
            "PUT" => Some(Self::PUT),
            "DELETE" => Some(Self::DELETE),
            "CONNECT" => Some(Self::CONNECT),
            "OPTIONS" => Some(Self::OPTIONS),
            "TRACE" => Some(Self::TRACE),
            "PATCH" => Some(Self::PATCH),
            _ => None,
        }
    }
}

/// Route handler type
pub type Handler = Box<dyn Fn(http::Request<()>, HandlerContext) -> HandlerOutput + Send + Sync>;

/// Handler output future type
pub type HandlerOutput = Box<dyn Future<Output = Response<String>> + Send>;

/// Request type alias
pub type Request = http::Request<()>;

// =============================================================================
// Response builder functions
// =============================================================================

/// Create a new response with status 200
pub fn response(body: impl Into<String>) -> Response<String> {
    Response::builder()
        .status(200)
        .body(body.into())
        .unwrap()
}

/// Create an HTML response
pub fn html_response(body: impl Into<String>) -> Response<String> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(body.into())
        .unwrap()
}

/// Create a JSON response
pub fn json_response<T: Serialize>(value: &T) -> Result<Response<String>, serde_json::Error> {
    let body = serde_json::to_string(value)?;
    Ok(Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap())
}

/// Create a redirect response
pub fn redirect(location: &str) -> Response<String> {
    Response::builder()
        .status(302)
        .header(header::LOCATION, location)
        .body(String::new())
        .unwrap()
}

/// Create a not found response
pub fn not_found() -> Response<String> {
    Response::builder()
        .status(404)
        .body("Not Found".to_string())
        .unwrap()
}

/// Create an error response
pub fn error(status: StatusCode, message: &str) -> Response<String> {
    Response::builder()
        .status(status)
        .body(message.to_string())
        .unwrap()
}
