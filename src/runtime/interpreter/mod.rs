//! HIR Interpreter for Development Mode
//!
//! Executes HIR directly without Rust code generation.

pub mod eval;
pub mod render;

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::transpile::hir::*;

/// Global interpreter state
pub struct Interpreter {
    modules: Arc<RwLock<HashMap<String, Module>>>,
    components: Arc<RwLock<HashMap<String, ComponentDef>>>,
    handlers: Arc<RwLock<HashMap<String, HandlerInfo>>>,
    layouts: Arc<RwLock<HashMap<String, LayoutInfo>>>,
    middleware: Arc<RwLock<Vec<MiddlewareInfo>>>,
    error_pages: Arc<RwLock<HashMap<u16, String>>>,
    islands: Arc<RwLock<HashMap<String, IslandInfo>>>,
}

/// Component definition
#[derive(Clone)]
struct ComponentDef {
    name: String,
    file_path: String,
    params: Vec<Param>,
    body: Vec<Stmt>,
}

/// Handler information
#[derive(Clone)]
struct HandlerInfo {
    file_path: String,
    methods: HashMap<String, HandlerMethod>,
    component_name: Option<String>,
    props_type: Option<String>,
}

/// Single handler method
#[derive(Clone)]
struct HandlerMethod {
    params: Vec<Param>,
    body: Vec<Stmt>,
    is_async: bool,
}

/// Layout information
#[derive(Clone)]
struct LayoutInfo {
    file_path: String,
    name: String,
    pattern: String,
    params: Vec<Param>,
    body: Vec<Stmt>,
}

/// Middleware information
#[derive(Clone)]
struct MiddlewareInfo {
    file_path: String,
    params: Vec<Param>,
    body: Vec<Stmt>,
    is_async: bool,
    is_global: bool,
    pattern: Option<String>,
}

/// Island information
#[derive(Clone)]
struct IslandInfo {
    file_path: String,
    name: String,
    props_type: Option<String>,
    props_fields: Vec<ObjectMember>,
    params: Vec<Param>,
    body: Vec<Stmt>,
}

/// Evaluation context for a single render
#[derive(Debug, Clone)]
pub struct EvalContext {
    pub scope: HashMap<String, Value>,
    pub props: HashMap<String, Value>,
    pub params: HashMap<String, String>,
    pub url: String,
    pub island_props: Option<HashMap<String, Value>>,
    pub rendered_islands: Vec<String>,
    pub request: Option<RequestInfo>,
    pub state: HashMap<String, Value>,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self {
            scope: HashMap::new(),
            props: HashMap::new(),
            params: HashMap::new(),
            url: String::new(),
            island_props: None,
            rendered_islands: vec![],
            request: None,
            state: HashMap::new(),
        }
    }
}

/// Runtime value
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
    Function(String),
    VNode(VNodeValue),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Undefined, Value::Undefined) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for Value {}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Undefined => write!(f, "undefined"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Array(arr) => write!(
                f,
                "[{}]",
                arr.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Object(obj) => write!(
                f,
                "{{{}}}",
                obj.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
            Value::Function(name) => write!(f, "fn {}", name),
            Value::VNode(v) => write!(f, "<{} />", v.tag),
        }
    }
}

impl Value {
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::String(s) => !s.is_empty(),
            Value::Number(n) => *n != 0.0,
            Value::Null | Value::Undefined => false,
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(_) => true,
            Value::Function(_) => true,
            Value::VNode(_) => true,
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Bool(true) => 1.0,
            Value::Bool(false) => 0.0,
            Value::String(s) => s.parse().unwrap_or(0.0),
            Value::Null | Value::Undefined => 0.0,
            _ => 0.0,
        }
    }

    pub fn get_member(&self, key: &str) -> Option<Value> {
        match self {
            Value::Object(map) => map.get(key).cloned(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VNodeValue {
    pub tag: String,
    pub attrs: HashMap<String, Value>,
    pub children: Vec<Value>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::new())),
            components: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            layouts: Arc::new(RwLock::new(HashMap::new())),
            middleware: Arc::new(RwLock::new(Vec::new())),
            error_pages: Arc::new(RwLock::new(HashMap::new())),
            islands: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn load_module(&mut self, path: &Path, source: &str) -> Result<(), anyhow::Error> {
        let parser = crate::transpile::TsParser::new();
        let module = parser.parse_source(source)?;

        let path_str = path.to_string_lossy().to_string();
        self.modules
            .write()
            .insert(path_str.clone(), module.clone());

        for item in &module.items {
            match item {
                ModuleItem::Export(export) => {
                    if let Export::Default { expr } = export {
                        if let Expr::Function(decl) = expr {
                            if decl
                                .name
                                .chars()
                                .next()
                                .map(|c| c.is_uppercase())
                                .unwrap_or(false)
                            {
                                let component = ComponentDef {
                                    name: decl.name.clone(),
                                    file_path: path_str.clone(),
                                    params: decl.params.clone(),
                                    body: decl.body.clone().unwrap_or_default().0,
                                };
                                self.components.write().insert(decl.name.clone(), component);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub fn render_route(
        &self,
        _pattern: &str,
        params: HashMap<String, String>,
    ) -> Result<String, anyhow::Error> {
        let ctx = EvalContext {
            scope: HashMap::new(),
            props: HashMap::new(),
            params,
            url: String::new(),
            island_props: None,
            rendered_islands: vec![],
            request: None,
            state: HashMap::new(),
        };

        let components = self.components.read();
        if let Some(component) = components.get("Home") {
            self.render_component(component, &ctx)
        } else {
            Ok(String::new())
        }
    }

    fn render_component(
        &self,
        component: &ComponentDef,
        _ctx: &EvalContext,
    ) -> Result<String, anyhow::Error> {
        Ok(format!("<div>{}</div>", component.name))
    }

    /// Load a file into the interpreter
    pub fn load_file(&mut self, path: &str, source: &str) -> Result<(), anyhow::Error> {
        use crate::transpile::parser::TsParser;
        let parser = TsParser::new();
        let module = parser.parse_source(source)?;
        // Store the module for later use
        let mut modules = self.modules.write();
        modules.insert(path.to_string(), module);
        Ok(())
    }

    /// Execute a route by file path
    pub fn execute_route_by_file(
        &self,
        _path: &str,
        _params: HashMap<String, String>,
        _request: RequestInfo,
    ) -> Result<RenderResult, anyhow::Error> {
        Ok(RenderResult {
            status: 200,
            headers: HashMap::new(),
            body: String::new(),
            html: String::new(),
            page_data: None,
            islands: vec![],
        })
    }

    /// Execute a route
    pub fn execute_route(
        &self,
        path: &str,
        params: HashMap<String, String>,
        request: RequestInfo,
    ) -> Result<RenderResult, anyhow::Error> {
        self.execute_route_by_file(path, params, request)
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// Request information for rendering
#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub method: String,
    pub path: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub name: Option<String>,
    pub id: Option<String>,
}

/// Render result from a route
#[derive(Debug, Clone)]
pub struct RenderResult {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub html: String,
    pub page_data: Option<String>,
    pub islands: Vec<String>,
}
