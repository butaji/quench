//! HIR Interpreter for Development Mode

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

#[derive(Clone)]
struct ComponentDef {
    name: String,
    file_path: String,
    params: Vec<Param>,
    body: Vec<Stmt>,
}

#[derive(Clone)]
struct HandlerInfo {
    file_path: String,
    methods: HashMap<String, HandlerMethod>,
    component_name: Option<String>,
    props_type: Option<String>,
}

#[derive(Clone)]
struct HandlerMethod {
    params: Vec<Param>,
    body: Vec<Stmt>,
    is_async: bool,
}

#[derive(Clone)]
struct LayoutInfo {
    file_path: String,
    name: String,
    pattern: String,
    params: Vec<Param>,
    body: Vec<Stmt>,
}

#[derive(Clone)]
struct MiddlewareInfo {
    file_path: String,
    params: Vec<Param>,
    body: Vec<Stmt>,
    is_async: bool,
    is_global: bool,
    pattern: Option<String>,
}

#[derive(Clone)]
struct IslandInfo {
    file_path: String,
    name: String,
    props_type: Option<String>,
    props_fields: Vec<ObjectMember>,
    params: Vec<Param>,
    body: Vec<Stmt>,
}

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

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Undefined => write!(f, "undefined"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Array(arr) => write!(f, "{:?}", arr),
            Value::Object(obj) => write!(f, "{:?}", obj),
            Value::Function(name) => write!(f, "function {}() {{}}", name),
            Value::VNode(vnode) => write!(f, "<{} />", vnode.tag),
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

    pub fn eval_module(&self, module: &Module) -> String {
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Variable(var)) = item {
                if let Some(init) = &var.init {
                    return format!("{:?}", init);
                }
            }
        }
        String::new()
    }

    pub fn load_module(&mut self, path: &Path, source: &str) -> Result<(), anyhow::Error> {
        let parser = crate::transpile::TsParser::new();
        let module = parser.parse_source(source)?;
        let path_str = path.to_string_lossy().to_string();
        self.modules.write().insert(path_str.clone(), module.clone());
        for item in &module.items {
            if let ModuleItem::Export(export) = item {
                if let Export::Default { expr } = export {
                    if let Expr::Function(decl) = expr {
                        if decl.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
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
        }
        Ok(())
    }

    pub fn render_route(&self, _pattern: &str, params: HashMap<String, String>) -> Result<String, anyhow::Error> {
        let ctx = EvalContext { params, ..Default::default() };
        let components = self.components.read();
        if let Some(component) = components.get("Home") {
            self.render_component(component, &ctx)
        } else {
            Ok(String::new())
        }
    }

    fn render_component(&self, component: &ComponentDef, ctx: &EvalContext) -> Result<String, anyhow::Error> {
        let mut html = String::new();
        html.push_str(&format!("<div data-component=\"{}\">", component.name));
        for stmt in &component.body {
            html.push_str(&self.render_stmt(stmt, ctx)?);
        }
        html.push_str("</div>");
        Ok(html)
    }

    fn render_stmt(&self, stmt: &Stmt, ctx: &EvalContext) -> Result<String, anyhow::Error> {
        match stmt {
            Stmt::Return { arg: Some(expr) } => Ok(format!("{{{{{}}}}}", self.render_expr(expr, ctx)?)),
            Stmt::Block(stmts) => {
                let mut html = String::new();
                for s in stmts { html.push_str(&self.render_stmt(s, ctx)?); }
                Ok(html)
            }
            _ => Ok(String::new()),
        }
    }

    fn render_expr(&self, expr: &Expr, ctx: &EvalContext) -> Result<String, anyhow::Error> {
        match expr {
            Expr::String(s) => Ok(s.clone()),
            Expr::Ident { name } => Ok(ctx.scope.get(name).map(|v| format!("{}", v)).unwrap_or_else(|| format!("{{{}}}", name))),
            _ => Ok(String::new()),
        }
    }

    pub fn load_file(&mut self, path: &str, source: &str) -> Result<(), anyhow::Error> {
        let parser = crate::transpile::TsParser::new();
        let module = parser.parse_source(source)?;
        self.modules.write().insert(path.to_string(), module);
        Ok(())
    }

    pub fn execute_route_by_file(&self, file: &str) -> Result<String, anyhow::Error> {
        let modules = self.modules.read();
        if let Some(module) = modules.get(file) {
            let mut html = String::new();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(func)) = item {
                    if func.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        html.push_str(&format!("<div data-component=\"{}\">", func.name));
                        if let Some(body) = &func.body {
                            for stmt in &body.0 { html.push_str(&self.render_stmt(stmt, &EvalContext::default())?); }
                        }
                        html.push_str("</div>");
                    }
                }
            }
            Ok(html)
        } else {
            Ok(String::new())
        }
    }

    pub fn execute_route(&self, path: &str, params: HashMap<String, String>) -> Result<String, anyhow::Error> {
        let ctx = EvalContext { params, ..Default::default() };
        let modules = self.modules.read();
        if let Some(module) = modules.get(path) {
            let mut html = String::new();
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(func)) = item {
                    if func.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        html.push_str(&format!("<div data-component=\"{}\">", func.name));
                        if let Some(body) = &func.body {
                            for stmt in &body.0 { html.push_str(&self.render_stmt(stmt, &ctx)?); }
                        }
                        html.push_str("</div>");
                    }
                }
            }
            Ok(html)
        } else {
            Ok(String::new())
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}
