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
    handlers: Arc<RwLock<HashMap<String, HandlerInfo>>>,
    /// Layouts registry (path pattern -> layout info)
    layouts: Arc<RwLock<HashMap<String, LayoutInfo>>>,
    /// Middleware registry
    middleware: Arc<RwLock<Vec<MiddlewareInfo>>>,
    /// Error pages
    error_pages: Arc<RwLock<HashMap<u16, String>>>,
    /// Island definitions (path -> island info)
    islands: Arc<RwLock<HashMap<String, IslandInfo>>>,
}

/// Component definition
#[derive(Clone)]
struct ComponentDef {
    name: String,
    file_path: String,
}

/// Handler information
#[derive(Clone)]
struct HandlerInfo {
    /// File path
    file_path: String,
    /// Handler methods (GET, POST, etc.)
    methods: HashMap<String, HandlerMethod>,
    /// Default export component name
    component_name: Option<String>,
}

/// Single handler method
#[derive(Clone)]
struct HandlerMethod {
    /// Function parameters
    params: Vec<Param>,
    /// Function body
    body: Vec<Stmt>,
    /// Is async
    is_async: bool,
}

/// Layout information
#[derive(Clone)]
struct LayoutInfo {
    /// File path
    file_path: String,
    /// Layout name (for matching)
    name: String,
    /// Layout route pattern
    pattern: String,
}

/// Middleware information
#[derive(Clone)]
struct MiddlewareInfo {
    /// File path
    file_path: String,
    /// Function parameters
    params: Vec<Param>,
    /// Function body
    body: Vec<Stmt>,
    /// Is async
    is_async: bool,
    /// Is global (routes/_middleware.ts)
    is_global: bool,
}

/// Island information
#[derive(Clone)]
struct IslandInfo {
    /// File path
    file_path: String,
    /// Island name
    name: String,
    /// Props type (interface name)
    props_type: Option<String>,
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
    /// Middleware state
    state: HashMap<String, Value>,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self {
            scope: HashMap::new(),
            params: HashMap::new(),
            url: String::new(),
            rendered_islands: Vec::new(),
            state: HashMap::new(),
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
            layouts: Arc::new(RwLock::new(HashMap::new())),
            middleware: Arc::new(RwLock::new(Vec::new())),
            error_pages: Arc::new(RwLock::new(HashMap::new())),
            islands: Arc::new(RwLock::new(HashMap::new())),
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

    /// Register a parsed module based on its path
    fn register_module(&mut self, path: &str, module: Module) {
        let path_lower = path.to_lowercase();
        
        // Determine module type by path
        let is_island = path_lower.contains("/islands/") || path_lower.contains("\\islands\\");
        let is_layout = path_lower.contains("_layout");
        let is_middleware = path_lower.contains("_middleware");
        let is_error_page = path_lower.contains("_404") || path_lower.contains("_500");
        
        // Store module
        self.modules.write().insert(path.to_string(), module.clone());
        
        // Register based on type
        if is_error_page {
            self.register_error_page(path, &module);
        } else if is_middleware {
            self.register_middleware(path, &module);
        } else if is_layout {
            self.register_layout(path, &module);
        } else if is_island {
            self.register_island(path, &module);
        } else {
            self.register_route(path, &module);
        }
        
        // Always register components
        self.register_components(path, &module);
    }

    /// Register an island component
    fn register_island(&mut self, path: &str, module: &Module) {
        let name = extract_file_name(path, "islands/");
        let island_info = IslandInfo {
            file_path: path.to_string(),
            name: name.clone(),
            props_type: None,
        };
        
        // Look for props interface
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Type(t)) = item {
                if t.name.ends_with("Props") {
                    let mut info = island_info.clone();
                    info.props_type = Some(t.name.clone());
                    self.islands.write().insert(name, info);
                    return;
                }
            }
        }
        
        self.islands.write().insert(name, island_info);
    }

    /// Register an error page
    fn register_error_page(&mut self, path: &str, module: &Module) {
        let code = if path.contains("_404") { 404 } else { 500 };
        
        // Find the default export component
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let route_key = format!("/_error_{}", code);
                    let handler_info = HandlerInfo {
                        file_path: path.to_string(),
                        methods: HashMap::new(),
                        component_name: Some(decl.name.clone()),
                    };
                    self.handlers.write().insert(route_key, handler_info);
                    self.error_pages.write().insert(code as u16, path.to_string());
                    return;
                }
            }
        }
    }

    /// Register middleware
    fn register_middleware(&mut self, path: &str, module: &Module) {
        for item in &module.items {
            // Check for default export function
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let is_global = path.contains("routes/_middleware");
                    let middleware = MiddlewareInfo {
                        file_path: path.to_string(),
                        params: decl.params.clone(),
                        body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                        is_async: decl.is_async,
                        is_global,
                    };
                    self.middleware.write().push(middleware);
                    return;
                }
            }
            
            // Check for named handler export
            if let ModuleItem::Export(Export::NamedWithValue { name, value }) = item {
                if name == "handler" || name.ends_with("Handler") {
                    if let Expr::Function { decl } = value {
                        let is_global = path.contains("routes/_middleware");
                        let middleware = MiddlewareInfo {
                            file_path: path.to_string(),
                            params: decl.params.clone(),
                            body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                            is_async: decl.is_async,
                            is_global,
                        };
                        self.middleware.write().push(middleware);
                    }
                }
            }
        }
    }

    /// Register a layout
    fn register_layout(&mut self, path: &str, module: &Module) {
        let pattern = path_to_layout_pattern(path);
        let name = extract_file_name(path, "routes/");
        
        // Find the default export function
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let layout_info = LayoutInfo {
                        file_path: path.to_string(),
                        name: decl.name.clone(),
                        pattern: pattern.clone(),
                    };
                    self.layouts.write().insert(pattern, layout_info);
                    return;
                }
            }
        }
    }

    /// Register a route with handlers
    fn register_route(&mut self, path: &str, module: &Module) {
        let route_key = path_to_route_key(path);
        let mut handler_info = HandlerInfo {
            file_path: path.to_string(),
            methods: HashMap::new(),
            component_name: None,
        };
        
        for item in &module.items {
            // Check for handler export (export const handler = { GET, POST, ... })
            if let ModuleItem::Export(Export::NamedWithValue { name, value }) = item {
                if name == "handler" {
                    if let Expr::Object { props } = value {
                        for prop in props {
                            if let ObjectProp::Init { key: PropKey::Ident(method), value: handler_expr } = prop {
                                if let Expr::Arrow { params, body, is_async } = handler_expr {
                                    let handler_method = HandlerMethod {
                                        params: params.clone(),
                                        body: match body.as_ref() {
                                            Stmt::Block(stmts) => stmts.clone(),
                                            other => vec![other.clone()],
                                        },
                                        is_async: *is_async,
                                    };
                                    handler_info.methods.insert(method.clone(), handler_method);
                                }
                            }
                        }
                    }
                }
            }
            
            // Check for default export (page component)
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    handler_info.component_name = Some(decl.name.clone());
                }
            }
        }
        
        if !handler_info.methods.is_empty() || handler_info.component_name.is_some() {
            self.handlers.write().insert(route_key, handler_info);
        }
    }

    /// Register components from a module
    fn register_components(&mut self, path: &str, module: &Module) {
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                // PascalCase function names are components
                if f.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    let comp_def = ComponentDef {
                        name: f.name.clone(),
                        file_path: path.to_string(),
                    };
                    self.components.write().insert(f.name.clone(), comp_def);
                }
            }
        }
    }
    
    /// Get layout chain for a route
    fn get_layout_chain(&self, route_path: &str) -> Vec<LayoutInfo> {
        let mut layouts = Vec::new();
        let layouts_guard = self.layouts.read();
        
        // Find all matching layouts
        let mut current_path = route_path;
        while !current_path.is_empty() {
            // Find layout for current path
            for (pattern, layout) in layouts_guard.iter() {
                if pattern_matches(pattern, current_path) {
                    layouts.push(layout.clone());
                }
            }
            
            // Move up one directory
            if let Some(pos) = current_path.rfind('/') {
                current_path = &current_path[..pos];
                if current_path.is_empty() {
                    break;
                }
            } else {
                break;
            }
        }
        
        layouts
    }
    
    /// Execute middleware chain
    fn execute_middleware(&self, _request: &Request, _state: &mut MiddlewareState) -> Result<(), String> {
        let middleware_guard = self.middleware.read();
        
        for mw in middleware_guard.iter() {
            if mw.is_global {
                // Execute global middleware
                // For now, just simulate state updates
                // In real implementation, we'd evaluate the middleware body
                _state.set("middleware_executed", Value::Bool(true));
            }
        }
        
        Ok(())
    }
    
    /// Execute a route handler and render the page
    pub fn execute_route(&self, route_path: &str, method: &str, params: HashMap<String, String>) -> Result<RenderResult, String> {
        let route_key = route_path.to_string();
        
        // Create middleware state
        let mut middleware_state = MiddlewareState::new();
        
        // Execute middleware chain
        let dummy_request = Request::new(route_path.to_string());
        self.execute_middleware(&dummy_request, &mut middleware_state)?;
        
        // Get handler info
        let handler = self.handlers.read()
            .get(&route_key)
            .cloned()
            .ok_or_else(|| {
                // Try to find 404 handler
                if let Some(not_found) = self.error_pages.read().get(&404) {
                    format!("Route not found, would use 404 handler: {}", not_found)
                } else {
                    format!("No handler found for route: {}", route_path)
                }
            })?;
        
        // Create evaluation context
        let mut ctx = EvalContext {
            scope: HashMap::new(),
            params: params.clone(),
            url: format!("http://localhost{}", route_path),
            rendered_islands: Vec::new(),
            state: middleware_state.data.clone(),
        };
        
        // Add route params to scope
        for (k, v) in &ctx.params {
            ctx.scope.insert(k.clone(), Value::String(v.clone()));
        }
        
        // Check if we have a handler method for this HTTP method
        let method_upper = method.to_uppercase();
        if let Some(handler_method) = handler.methods.get(&method_upper) {
            // Execute handler to get page data
            // For now, we'll simulate handler execution
            ctx.scope.insert("_handler_called".to_string(), Value::Bool(true));
        }
        
        // Render the page component
        let page_html = if let Some(component_name) = &handler.component_name {
            self.render_component(component_name, &ctx, &handler.file_path)?
        } else {
            String::new()
        };
        
        // Apply layouts
        let layout_chain = self.get_layout_chain(&route_key);
        let full_html = self.apply_layouts(&page_html, &layout_chain, &ctx)?;
        
        Ok(RenderResult {
            html: full_html,
            page_data: Value::Object(ctx.scope),
            islands: ctx.rendered_islands,
        })
    }
    
    /// Render a component by name
    fn render_component(&self, name: &str, ctx: &EvalContext, _file_path: &str) -> Result<String, String> {
        // Look up component definition
        let comp = self.components.read().get(name).cloned();
        
        if let Some(comp_def) = comp {
            // Get the module
            let module = self.modules.read().get(&comp_def.file_path).cloned();
            if let Some(module) = module {
                // Find the component function
                for item in &module.items {
                    if let ModuleItem::Decl(Decl::Function(f)) = item {
                        if &f.name == name {
                            return self.render_function_component(f, ctx);
                        }
                    }
                }
            }
        }
        
        // Fallback: render a placeholder
        Ok(format!("<div class=\"component-{}\">Component {}</div>", name.to_lowercase(), name))
    }
    
    /// Render a function component
    fn render_function_component(&self, f: &FunctionDecl, ctx: &EvalContext) -> Result<String, String> {
        // Evaluate the function body to get JSX
        let body = match &f.body {
            Some(b) => &b.0,
            None => return Ok(String::new()),
        };
        
        // Find return statement with JSX
        for stmt in body {
            if let Stmt::Return { arg: Some(expr) } = stmt {
                return self.evaluate_expr_to_html(expr, ctx);
            }
        }
        
        Ok(String::new())
    }
    
    /// Apply layout chain to content
    fn apply_layouts(&self, content: &str, layouts: &[LayoutInfo], ctx: &EvalContext) -> Result<String, String> {
        let mut result = content.to_string();
        
        // Apply layouts in order (innermost to outermost)
        for layout in layouts.iter().rev() {
            let module = self.modules.read().get(&layout.file_path).cloned();
            if let Some(module) = module {
                // Find the layout function
                for item in &module.items {
                    if let ModuleItem::Decl(Decl::Function(f)) = item {
                        if &f.name == &layout.name {
                            // Create a context with children
                            let mut layout_ctx = ctx.clone();
                            layout_ctx.scope.insert("children".to_string(), Value::String(result.clone()));
                            
                            result = self.render_function_component(f, &layout_ctx)?;
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(result)
    }
    
    /// Evaluate an expression to HTML string
    fn evaluate_expr_to_html(&self, expr: &Expr, ctx: &EvalContext) -> Result<String, String> {
        match expr {
            Expr::JSX(jsx) => self.jsx_to_html(jsx, ctx),
            Expr::String(s) => Ok(s.clone()),
            Expr::Template { parts, exprs } => {
                let mut result = String::new();
                for (i, part) in parts.iter().enumerate() {
                    if let TemplatePart::String(s) = part {
                        result.push_str(s);
                    }
                    if i < exprs.len() {
                        let val = self.evaluate_expr_to_html(&exprs[i], ctx)?;
                        result.push_str(&val);
                    }
                }
                Ok(result)
            }
            Expr::Ident { name } => {
                // Look up variable in context
                if let Some(value) = ctx.scope.get(name) {
                    Ok(value.to_string())
                } else {
                    Ok(format!("{{{}}}", name))
                }
            }
            Expr::Cond { test, consequent, alternate } => {
                let test_val = self.expr_to_value(test, ctx)?;
                if test_val.as_bool() {
                    self.evaluate_expr_to_html(consequent, ctx)
                } else {
                    self.evaluate_expr_to_html(alternate, ctx)
                }
            }
            Expr::Logical { op: LogicalOp::And, left, right } => {
                let left_val = self.expr_to_value(left, ctx)?;
                if left_val.as_bool() {
                    self.evaluate_expr_to_html(right, ctx)
                } else {
                    Ok(String::new())
                }
            }
            Expr::Call { callee, args, .. } => {
                // Handle hook calls
                if let Expr::Ident { name } = callee.as_ref() {
                    let value = self.call_hook(name, args, ctx)?;
                    return Ok(value.to_string());
                }
                Ok(String::new())
            }
            _ => Ok(String::new()),
        }
    }
    
    /// Call a hook and return appropriate value
    fn call_hook(&self, name: &str, _args: &[Expr], _ctx: &EvalContext) -> Result<Value, String> {
        match name {
            "useState" => {
                // Return a mock setState value
                Ok(Value::Array(vec![Value::Number(0.0), Value::Function("setState".to_string())]))
            }
            "useEffect" => {
                // Effects run on client side
                Ok(Value::Undefined)
            }
            _ => Ok(Value::String(format!("[{}]", name))),
        }
    }
    
    fn jsx_to_html(&self, jsx: &JSXExpr, ctx: &EvalContext) -> Result<String, String> {
        let tag = match &jsx.opening.name {
            JSXName::Ident(s) => s.clone(),
            JSXName::Member { object, property } => format!("{}_{}", object, property),
            _ => return Ok(String::new()),
        };

        let is_component = tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        let mut vnode = VNode::new(&tag, is_component);
        
        for attr in &jsx.opening.attrs {
            match attr {
                JSXAttr::Attr { name, value } => {
                    let attr_value = match value.as_ref() {
                        Some(JSXAttrValue::String(s)) => Value::String(s.clone()),
                        Some(JSXAttrValue::Expr(e)) => self.expr_to_value(e, ctx)?,
                        None => Value::Bool(true),
                    };
                    vnode.attrs.insert(name.clone(), attr_value);
                }
                JSXAttr::Spread { expr } => {
                    // Handle spread attributes
                    if let Value::Object(props) = self.expr_to_value(expr, ctx)? {
                        for (k, v) in props {
                            vnode.attrs.insert(k, v);
                        }
                    }
                }
                _ => {}
            }
        }

        for child in &jsx.children {
            match child {
                JSXChild::Text(s) => {
                    if !s.trim().is_empty() {
                        let mut text_vnode = VNode::new("", false);
                        text_vnode.tag = s.clone();
                        vnode.children.push(text_vnode);
                    }
                }
                JSXChild::Expr(e) => {
                    let val = self.evaluate_expr_to_html(e, ctx)?;
                    if !val.is_empty() {
                        let mut expr_vnode = VNode::new("", false);
                        expr_vnode.tag = val;
                        vnode.children.push(expr_vnode);
                    }
                }
                JSXChild::JSX(inner_jsx) => {
                    let child_html = self.jsx_to_html(inner_jsx, ctx)?;
                    let mut child_vnode = VNode::new("", false);
                    child_vnode.tag = child_html;
                    vnode.children.push(child_vnode);
                }
                JSXChild::Fragment { children } => {
                    for child in children {
                        if let JSXChild::Text(s) = child {
                            if !s.trim().is_empty() {
                                let mut text_vnode = VNode::new("", false);
                                text_vnode.tag = s.clone();
                                vnode.children.push(text_vnode);
                            }
                        } else if let JSXChild::JSX(inner_jsx) = child {
                            let child_html = self.jsx_to_html(inner_jsx, ctx)?;
                            let mut child_vnode = VNode::new("", false);
                            child_vnode.tag = child_html;
                            vnode.children.push(child_vnode);
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(vnode.to_html_string())
    }

    fn expr_to_value(&self, expr: &Expr, ctx: &EvalContext) -> Result<Value, String> {
        match expr {
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Null => Ok(Value::Null),
            Expr::Boolean(b) => Ok(Value::Bool(*b)),
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Ident { name } => {
                // Look up in context
                if let Some(value) = ctx.scope.get(name) {
                    Ok(value.clone())
                } else {
                    Ok(Value::String(format!("{{{}}}", name)))
                }
            }
            Expr::Object { props } => {
                let mut obj = HashMap::new();
                for prop in props {
                    match prop {
                        ObjectProp::Init { key, value } => {
                            let k = match key {
                                PropKey::Ident(s) => s.clone(),
                                PropKey::String(s) => s.clone(),
                                PropKey::Number(n) => n.to_string(),
                                PropKey::Computed(_) => continue,
                            };
                            let v = self.expr_to_value(value, ctx)?;
                            obj.insert(k, v);
                        }
                        ObjectProp::Spread { value } => {
                            if let Value::Object(spread_obj) = self.expr_to_value(value, ctx)? {
                                obj.extend(spread_obj);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Value::Object(obj))
            }
            Expr::Array { elems } => {
                let mut arr = Vec::new();
                for elem in elems {
                    if let Some(e) = elem {
                        arr.push(self.expr_to_value(e, ctx)?);
                    }
                }
                Ok(Value::Array(arr))
            }
            Expr::Bin { op, left, right } => {
                let l = self.expr_to_value(left, ctx)?;
                let r = self.expr_to_value(right, ctx)?;
                
                match op {
                    BinaryOp::Add => {
                        // Check if both are strings for concatenation
                        match (&l, &r) {
                            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
                            _ => Ok(Value::Number(l.as_number() + r.as_number())),
                        }
                    }
                    BinaryOp::Sub => Ok(Value::Number(l.as_number() - r.as_number())),
                    BinaryOp::Mul => Ok(Value::Number(l.as_number() * r.as_number())),
                    BinaryOp::Div => Ok(Value::Number(l.as_number() / r.as_number())),
                    BinaryOp::Eq | BinaryOp::EqStrict => Ok(Value::Bool(l == r)),
                    BinaryOp::Ne | BinaryOp::NeStrict => Ok(Value::Bool(l != r)),
                    _ => Ok(Value::Undefined),
                }
            }
            Expr::Cond { test, consequent, alternate } => {
                let test_val = self.expr_to_value(test, ctx)?;
                if test_val.as_bool() {
                    self.expr_to_value(consequent, ctx)
                } else {
                    self.expr_to_value(alternate, ctx)
                }
            }
            Expr::Logical { op, left, right } => {
                let l = self.expr_to_value(left, ctx)?;
                match op {
                    LogicalOp::And => {
                        if l.as_bool() {
                            self.expr_to_value(right, ctx)
                        } else {
                            Ok(l)
                        }
                    }
                    LogicalOp::Or => {
                        if l.as_bool() {
                            Ok(l)
                        } else {
                            self.expr_to_value(right, ctx)
                        }
                    }
                    LogicalOp::NullishCoalesce => {
                        if matches!(l, Value::Null | Value::Undefined) {
                            self.expr_to_value(right, ctx)
                        } else {
                            Ok(l)
                        }
                    }
                }
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident { name } = callee.as_ref() {
                    return self.call_hook(name, args, ctx);
                }
                Ok(Value::Undefined)
            }
            _ => Ok(Value::Undefined),
        }
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
                    let result = self.evaluate_jsx_from_body(&decl.body, ctx)?;
                    return Ok(result);
                }
            }
        }
        
        Ok(String::new())
    }
    
    /// Evaluate JSX from a function body
    fn evaluate_jsx_from_body(&self, body: &Option<crate::transpile::hir::Block>, ctx: &EvalContext) -> Result<String, String> {
        let body = match body {
            Some(b) => b,
            None => return Ok(String::new()),
        };
        
        // Find return statement with JSX
        for stmt in &body.0 {
            if let Stmt::Return { arg } = stmt {
                if let Some(expr) = arg {
                    return self.evaluate_expr_to_html(expr, ctx);
                }
            }
        }
        
        Ok(String::new())
    }
}

impl Default for Interpreter {
    fn default() -> Self { Self::new() }
}

/// Middleware execution state
#[derive(Clone, Debug)]
struct MiddlewareState {
    data: HashMap<String, Value>,
}

impl MiddlewareState {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
    
    fn set(&mut self, key: &str, value: Value) {
        self.data.insert(key.to_string(), value);
    }
    
    fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }
}

/// Mock request for middleware
struct Request {
    url: String,
}

impl Request {
    fn new(url: String) -> Self {
        Self { url }
    }
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

/// Convert path to layout pattern
fn path_to_layout_pattern(path: &str) -> String {
    let path = path.replace('\\', "/");
    
    if let Some(routes_pos) = path.find("/routes/") {
        let route_part = &path[routes_pos + 8..];
        let route = route_part
            .trim_start_matches('/')
            .trim_end_matches("_layout.tsx")
            .trim_end_matches("_layout.ts");
        
        if route.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", route.trim_end_matches('/'))
        }
    } else {
        "/".to_string()
    }
}

/// Extract file name from path after a prefix
fn extract_file_name(path: &str, prefix: &str) -> String {
    if let Some(pos) = path.find(prefix) {
        let after_prefix = &path[pos + prefix.len()..];
        after_prefix
            .trim_start_matches('/')
            .trim_end_matches(".tsx")
            .trim_end_matches(".ts")
            .to_string()
    } else {
        path.to_string()
    }
}

/// Check if a pattern matches a path
fn pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern == "/" {
        return path == "/" || path.is_empty();
    }
    
    let pattern = pattern.trim_end_matches('/');
    let path = path.trim_end_matches('/');
    
    // Direct match
    if pattern == path {
        return true;
    }
    
    // Pattern is a prefix of path
    if path.starts_with(pattern) {
        return true;
    }
    
    false
}
