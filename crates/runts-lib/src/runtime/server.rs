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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_props_param() {
        let mut params = HashMap::new();
        params.insert("id".to_string(), "42".to_string());
        let props: PageProps = PageProps {
            params,
            url: "/users/42".to_string(),
            data: serde_json::Value::Null,
        };
        assert_eq!(props.param("id"), Some("42"));
        assert_eq!(props.param("nonexistent"), None);
    }

    #[test]
    fn test_page_props_with_data() {
        let params = HashMap::new();
        let props = PageProps {
            params,
            url: "/api".to_string(),
            data: serde_json::json!({"count": 10}),
        };
        assert_eq!(props.url, "/api");
    }

    #[test]
    fn test_page_props_param_empty() {
        let props: PageProps = PageProps {
            params: HashMap::new(),
            url: "/".to_string(),
            data: serde_json::Value::Null,
        };
        assert_eq!(props.param("anything"), None);
    }

    #[test]
    fn test_handler_context_new() {
        let ctx = HandlerContext::new();
        assert!(ctx.params.is_empty());
        assert_eq!(ctx.url, "");
    }

    #[test]
    fn test_handler_context_with_data() {
        let mut ctx = HandlerContext::new();
        ctx.params.insert("key".to_string(), "value".to_string());
        ctx.url = "/test".to_string();
        assert_eq!(ctx.params.get("key"), Some(&"value".to_string()));
        assert_eq!(ctx.url, "/test");
    }

    #[test]
    fn test_method_from_str() {
        assert_eq!(Method::from_str("GET"), Some(Method::GET));
        assert_eq!(Method::from_str("get"), Some(Method::GET));
        assert_eq!(Method::from_str("POST"), Some(Method::POST));
        assert_eq!(Method::from_str("post"), Some(Method::POST));
        assert_eq!(Method::from_str("PUT"), Some(Method::PUT));
        assert_eq!(Method::from_str("put"), Some(Method::PUT));
        assert_eq!(Method::from_str("DELETE"), Some(Method::DELETE));
        assert_eq!(Method::from_str("delete"), Some(Method::DELETE));
        assert_eq!(Method::from_str("PATCH"), None);
        assert_eq!(Method::from_str("OPTIONS"), None);
        assert_eq!(Method::from_str("invalid"), None);
    }

    #[test]
    fn test_method_variants() {
        assert_eq!(Method::GET, Method::GET);
        assert_eq!(Method::HEAD, Method::HEAD);
        assert_eq!(Method::POST, Method::POST);
        assert_eq!(Method::PUT, Method::PUT);
        assert_eq!(Method::DELETE, Method::DELETE);
        assert_eq!(Method::CONNECT, Method::CONNECT);
        assert_eq!(Method::OPTIONS, Method::OPTIONS);
        assert_eq!(Method::TRACE, Method::TRACE);
        assert_eq!(Method::PATCH, Method::PATCH);
    }

    #[test]
    fn test_method_partial_eq() {
        assert_eq!(Method::GET, Method::GET);
        assert_ne!(Method::GET, Method::POST);
        assert_ne!(Method::PUT, Method::DELETE);
    }

    #[test]
    fn test_handler_context_clone() {
        let ctx1 = HandlerContext::new();
        let ctx2 = ctx1.clone();
        assert_eq!(ctx1.params.len(), ctx2.params.len());
    }

    #[test]
    fn test_page_props_with_string_data() {
        let props: PageProps<String> = PageProps {
            params: HashMap::new(),
            url: "/".to_string(),
            data: "test".to_string(),
        };
        assert_eq!(props.data, "test");
    }

    #[test]
    fn test_page_props_param_with_special_chars() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), "hello world".to_string());
        let props: PageProps = PageProps {
            params,
            url: "/".to_string(),
            data: serde_json::Value::Null,
        };
        assert_eq!(props.param("name"), Some("hello world"));
    }
}
