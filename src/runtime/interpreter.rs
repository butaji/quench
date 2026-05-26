//! HIR Interpreter for Development Mode
//!
//! Executes HIR directly without Rust code generation.
//! This enables instant hot-reload in development mode.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::transpile::hir::*;

/// Global interpreter state
pub struct Interpreter {
    /// Loaded modules (file path -> module)
    modules: Arc<RwLock<HashMap<String, Module>>>,
    /// Components registry (name -> ComponentDef)
    components: Arc<RwLock<HashMap<String, ComponentDef>>>,
    /// Handlers registry (route pattern -> HandlerDef)
    handlers: Arc<RwLock<HashMap<String, HandlerDef>>>,
}

struct ComponentDef {
    name: String,
    file_path: String,
}

#[derive(Clone)]
struct HandlerDef {
    name: String,
    file_path: String,
}

/// Evaluation context for a single render
#[derive(Debug, Clone)]
pub struct EvalContext {
    /// Local variables
    scope: HashMap<String, Value>,
    /// Route parameters
    params: HashMap<String, String>,
    /// Current URL
    url: String,
    /// Rendered islands
    rendered_islands: Vec<RenderedIsland>,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self {
            scope: HashMap::new(),
            params: HashMap::new(),
            url: String::new(),
            rendered_islands: Vec::new(),
        }
    }
}

/// Rendered island placeholder
#[derive(Debug, Clone)]
pub struct RenderedIsland {
    pub name: String,
    pub props: HashMap<String, Value>,
    pub html: String,
    pub id: String,
}

/// Runtime values
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
    Function(String),
}

impl Value {
    pub fn to_string(&self) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            Value::Array(arr) => format!("[{}]", arr.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")),
            Value::Object(obj) => {
                let pairs: Vec<String> = obj.iter().map(|(k, v)| format!("{}: {}", k, v.to_string())).collect();
                format!("{{{}}}", pairs.join(", "))
            }
            Value::Function(name) => format!("[Function: {}]", name),
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Null | Value::Undefined => false,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(_) | Value::Function(_) => true,
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::String(s) => s.parse().unwrap_or(0.0),
            Value::Bool(b) => if *b { 1.0 } else { 0.0 },
            Value::Null | Value::Undefined => 0.0,
            _ => 0.0,
        }
    }

    pub fn get_member(&self, key: &str) -> Option<Value> {
        match self {
            Value::Object(obj) => obj.get(key).cloned(),
            Value::Array(arr) => match key {
                "length" => Some(Value::Number(arr.len() as f64)),
                _ => None,
            },
            Value::String(s) => match key {
                "length" => Some(Value::Number(s.len() as f64)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Virtual node for rendering
#[derive(Debug, Clone)]
pub struct VNode {
    pub tag: String,
    pub attrs: HashMap<String, Value>,
    pub children: Vec<VNode>,
    pub is_component: bool,
    pub key: Option<String>,
}

impl VNode {
    pub fn new(tag: &str, is_component: bool) -> Self {
        Self {
            tag: tag.to_string(),
            attrs: HashMap::new(),
            children: Vec::new(),
            is_component,
            key: None,
        }
    }

    pub fn to_html_string(&self) -> String {
        let mut html = format!("<{}", self.tag);
        for (key, value) in &self.attrs {
            match value {
                Value::Bool(true) => { html.push_str(&format!(" {}", key)); }
                Value::String(s) if !s.is_empty() => { html.push_str(&format!(" {}=\"{}\"", key, html_escape_attr(s))); }
                Value::Number(n) => { html.push_str(&format!(" {}=\"{}\"", key, n)); }
                _ => {}
            }
        }
        
        let has_children = !self.children.is_empty();
        if !has_children && !self.is_component {
            html.push_str("/>");
        } else {
            html.push('>');
            for child in &self.children { 
                html.push_str(&child.to_html_string()); 
            }
            html.push_str(&format!("</{}>", self.tag));
        }
        html
    }
}

fn generate_island_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos()).unwrap_or(0);
    format!("island-{:x}", nanos)
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::new())),
            components: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load a module from source code
    pub fn load_file(&mut self, path: &Path, source: &str) -> Result<(), String> {
        let path_str = path.to_string_lossy().to_string();
        let mut parser = crate::transpile::Parser::new();
        
        let module = parser.parse_source(source).map_err(|e| e.to_string())?;
        
        // Classify and register the module
        self.register_module(&path_str, module);
        
        Ok(())
    }

    /// Register a parsed module
    fn register_module(&mut self, path: &str, module: Module) {
        let path_lower = path.to_lowercase();
        let is_island = path_lower.contains("/islands/") || path_lower.contains("\\islands\\");
        let is_layout = path_lower.contains("_layout");
        
        if is_layout || is_island {
            // Skip for now
            return;
        }
        
        // Store module
        self.modules.write().insert(path.to_string(), module.clone());
        
        // Register components and handlers
        for item in &module.items {
            match item {
                ModuleItem::Decl(Decl::Function(f)) => {
                    let name = &f.name;
                    if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        // PascalCase = component
                        let comp_def = ComponentDef {
                            name: f.name.clone(),
                            file_path: path.to_string(),
                        };
                        self.components.write().insert(f.name.clone(), comp_def);
                    }
                }
                ModuleItem::Export(Export::Default { expr }) => {
                    if let Expr::Function { decl } = expr {
                        let handler_def = HandlerDef {
                            name: decl.name.clone(),
                            file_path: path.to_string(),
                        };
                        let route_key = path_to_route_key(path);
                        self.handlers.write().insert(route_key, handler_def);
                    }
                }
                _ => {}
            }
        }
    }

    /// Execute a route handler and render the page
    pub fn execute_route(&self, route_path: &str, _method: &str, params: HashMap<String, String>) -> Result<RenderResult, String> {
        // Find handler for this route
        let route_key = route_path.to_string();
        
        let _handler = self.handlers.read()
            .get(&route_key)
            .cloned()
            .ok_or_else(|| format!("No handler found for route: {}", route_path))?;
        
        // Create evaluation context
        let mut ctx = EvalContext {
            scope: HashMap::new(),
            params: params.clone(),
            url: format!("http://localhost{}", route_path),
            rendered_islands: Vec::new(),
        };
        
        // Add route params to scope
        for (k, v) in &ctx.params {
            ctx.scope.insert(k.clone(), Value::String(v.clone()));
        }
        
        // Render the default export component
        let component_html = self.render_default_component(&route_key, &ctx)?;
        
        Ok(RenderResult {
            html: component_html,
            page_data: Value::Object(ctx.scope),
            islands: ctx.rendered_islands,
        })
    }

    /// Render the default exported component
    fn render_default_component(&self, route_key: &str, ctx: &EvalContext) -> Result<String, String> {
        let module = self.modules.read()
            .get(route_key)
            .cloned()
            .ok_or_else(|| format!("Module not found: {}", route_key))?;
        
        // Find default export
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    // Evaluate the component to get JSX
                    let result = self.evaluate_jsx_from_body(&decl.body)?;
                    return Ok(result);
                }
            }
        }
        
        Ok(String::new())
    }

    /// Evaluate JSX from a function body
    fn evaluate_jsx_from_body(&self, body: &Option<crate::transpile::hir::Block>) -> Result<String, String> {
        let body = match body {
            Some(b) => b,
            None => return Ok(String::new()),
        };
        
        // Find the return statement with JSX
        for stmt in &body.0 {
            if let Stmt::Return { arg } = stmt {
                if let Some(expr) = arg {
                    return self.evaluate_expr_to_html(expr);
                }
            }
        }
        
        Ok(String::new())
    }

    /// Evaluate an expression to HTML string
    fn evaluate_expr_to_html(&self, expr: &Expr) -> Result<String, String> {
        match expr {
            Expr::JSX(jsx) => self.jsx_to_html(jsx),
            Expr::String(s) => Ok(s.clone()),
            Expr::Template { parts, exprs } => {
                let mut result = String::new();
                for (i, part) in parts.iter().enumerate() {
                    if let TemplatePart::String(s) = part {
                        result.push_str(s);
                    }
                    if i < exprs.len() {
                        let val = self.evaluate_expr_to_html(&exprs[i])?;
                        result.push_str(&val);
                    }
                }
                Ok(result)
            }
            Expr::Ident { name } => Ok(format!("{{{}}}", name)),
            _ => Ok(String::new()),
        }
    }

    fn jsx_to_html(&self, jsx: &JSXExpr) -> Result<String, String> {
        let tag = match &jsx.opening.name {
            JSXName::Ident(s) => s.clone(),
            JSXName::Member { object, property } => format!("{}_{}", object, property),
            _ => return Ok(String::new()),
        };

        let mut vnode = VNode::new(&tag, tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false));
        
        for attr in &jsx.opening.attrs {
            match attr {
                JSXAttr::Attr { name, value } => {
                    let attr_value = match value.as_ref() {
                        Some(JSXAttrValue::String(s)) => Value::String(s.clone()),
                        Some(JSXAttrValue::Expr(e)) => self.expr_to_value(e)?,
                        None => Value::Bool(true),
                    };
                    vnode.attrs.insert(name.clone(), attr_value);
                }
                _ => {}
            }
        }

        for child in &jsx.children {
            match child {
                JSXChild::Text(s) => {
                    if !s.trim().is_empty() {
                        let mut text_node = VNode::new("span", false);
                        text_node.children.push(VNode {
                            tag: String::new(),
                            attrs: HashMap::new(),
                            children: vec![],
                            is_component: false,
                            key: None,
                        });
                        // Add as text
                        let text_html = format!("{}", s);
                        vnode.children.push(VNode {
                            tag: String::new(),
                            attrs: HashMap::new(),
                            children: vec![],
                            is_component: false,
                            key: None,
                        });
                    }
                }
                JSXChild::Expr(e) => {
                    let val = self.expr_to_value(e)?;
                    // Simple placeholder for dynamic content
                    let mut child_node = VNode::new("span", false);
                    child_node.children.push(VNode {
                        tag: String::new(),
                        attrs: HashMap::new(),
                        children: vec![],
                        is_component: false,
                        key: None,
                    });
                    vnode.children.push(child_node);
                }
                JSXChild::JSX(inner_jsx) => {
                    let child_html = self.jsx_to_html(inner_jsx)?;
                    let mut child_node = VNode::new("span", false);
                    child_node.children.push(VNode {
                        tag: String::new(),
                        attrs: HashMap::new(),
                        children: vec![],
                        is_component: false,
                        key: None,
                    });
                    vnode.children.push(child_node);
                }
                _ => {}
            }
        }

        Ok(vnode.to_html_string())
    }

    fn expr_to_value(&self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Null => Ok(Value::Null),
            Expr::Boolean(b) => Ok(Value::Bool(*b)),
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Ident { name } => Ok(Value::String(format!("{{{}}}", name))),
            Expr::Object { props } => {
                let mut obj = HashMap::new();
                for prop in props {
                    if let ObjectProp::Init { key, value } = prop {
                        let k = match key {
                            PropKey::Ident(s) => s.clone(),
                            PropKey::String(s) => s.clone(),
                            _ => continue,
                        };
                        let v = self.expr_to_value(value)?;
                        obj.insert(k, v);
                    }
                }
                Ok(Value::Object(obj))
            }
            _ => Ok(Value::Undefined),
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self { Self::new() }
}

/// Result of rendering a route
#[derive(Debug)]
pub struct RenderResult {
    /// Rendered HTML
    pub html: String,
    /// Page data from handler
    pub page_data: Value,
    /// Rendered islands
    pub islands: Vec<RenderedIsland>,
}

/// Convert file path to route key
pub fn path_to_route_key(path: &str) -> String {
    let path = path.replace('\\', "/");
    
    if let Some(routes_pos) = path.find("/routes/") {
        let route_part = &path[routes_pos + 8..];
        let route = route_part
            .trim_start_matches('/')
            .trim_end_matches(".tsx")
            .trim_end_matches(".ts");
        
        if route.is_empty() || route == "index" {
            "/".to_string()
        } else if route.ends_with("/index") {
            format!("/{}", &route[..route.len() - 6])
        } else {
            let route = route
                .replace("[", ":")
                .replace("]", "");
            format!("/{}", route)
        }
    } else {
        "/".to_string()
    }
}
