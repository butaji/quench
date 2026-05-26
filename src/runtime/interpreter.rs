//! HIR Interpreter for Development Mode
//!
//! Executes HIR directly without Rust code generation.
//! This enables instant hot-reload in development mode.
//!
//! Features:
//! - Full Fresh route handler execution
//! - Islands architecture with partial hydration
//! - Layout system with nested composition
//! - Middleware pipeline
//! - Error pages (404, 500)

use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;
use serde_json::Value as JsonValue;

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
    pub params: HashMap<String, String>,
    pub url: String,
    pub rendered_islands: Vec<RenderedIsland>,
    pub state: HashMap<String, Value>,
    pub request: Option<RequestInfo>,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self {
            scope: HashMap::new(),
            params: HashMap::new(),
            url: String::new(),
            rendered_islands: Vec::new(),
            state: HashMap::new(),
            request: None,
        }
    }
}

/// Result of middleware execution
#[derive(Debug, Clone)]
enum MiddlewareResult {
    /// Continue to next middleware
    Continue,
    /// Skip to next middleware
    Next,
    /// Return this response immediately
    Response(String),
}

/// Rendered island placeholder
#[derive(Debug, Clone)]
pub struct RenderedIsland {
    pub name: String,
    pub props: HashMap<String, Value>,
    pub html: String,
    pub id: String,
    pub props_json: String,
    /// Hydration strategy
    pub hydrate: HydrationStrategy,
}

/// Island hydration strategy
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HydrationStrategy {
    /// Hydrate immediately on page load
    Load,
    /// Hydrate when visible in viewport
    Visible,
    /// Hydrate when user interacts
    Interaction,
    /// Hydrate when browser is idle
    Idle,
}

impl Default for HydrationStrategy {
    fn default() -> Self {
        Self::Load
    }
}

/// Request information
#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub method: String,
    pub headers: HashMap<String, String>,
    pub url: String,
}

/// Runtime values
#[derive(Debug, Clone)]
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

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Function(a), Value::Function(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }
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

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Undefined => serde_json::Value::Null,
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::json!(b),
            Value::Number(n) => serde_json::json!(n),
            Value::String(s) => serde_json::json!(s),
            Value::Array(arr) => serde_json::json!(arr.iter().map(|v| v.to_json()).collect::<Vec<_>>()),
            Value::Object(obj) => {
                let mut map = serde_json::Map::new();
                for (k, v) in obj {
                    map.insert(k.clone(), v.to_json());
                }
                serde_json::Value::Object(map)
            }
            Value::Function(_) => serde_json::Value::Null,
        }
    }
}

/// Virtual node for rendering
#[derive(Debug, Clone)]
pub struct VNode {
    pub tag: String,
    pub attrs: HashMap<String, Value>,
    pub children: Vec<String>,
    pub is_component: bool,
}

impl VNode {
    pub fn new(tag: &str, is_component: bool) -> Self {
        Self {
            tag: tag.to_string(),
            attrs: HashMap::new(),
            children: Vec::new(),
            is_component,
        }
    }

    pub fn to_html_string(&self) -> String {
        let mut html = format!("<{}", self.tag);
        for (key, value) in &self.attrs {
            match value {
                Value::Bool(true) => { html.push_str(&format!(" {}", key)); }
                Value::String(s) if !s.is_empty() => { html.push_str(&format!(" {}=\"{}\"", key, html_escape_attr(s))); }
                Value::Number(n) => { html.push_str(&format!(" {}=\"{}\"", key, n)); }
                Value::Bool(false) => {} // Skip false booleans
                _ => {}
            }
        }
        
        let has_children = !self.children.is_empty();
        if !has_children && !self.is_component {
            html.push_str("/>");
        } else {
            html.push('>');
            for child in &self.children { 
                html.push_str(child); 
            }
            html.push_str(&format!("</{}>", self.tag));
        }
        html
    }
}

fn generate_island_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos()).unwrap_or(0);
    format!("island-{:x}", nanos)
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Execution mode for route handlers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExecMode {
    /// Execute full handler including ctx.render() (default)
    Full,
    /// Only execute handler, skip component rendering (for API routes)
    HandlerOnly,
    /// Only render component, skip handler (for page-specific data)
    RenderOnly,
}

impl Default for ExecMode {
    fn default() -> Self {
        Self::Full
    }
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

    pub fn load_file(&mut self, path: &Path, source: &str) -> Result<(), String> {
        let path_str = path.to_string_lossy().to_string();
        let mut parser = crate::transpile::Parser::new();
        
        let module = parser.parse_source(source).map_err(|e| e.to_string())?;
        self.register_module(&path_str, module);
        
        Ok(())
    }

    fn register_module(&mut self, path: &str, module: Module) {
        let path_lower = path.to_lowercase();
        
        let is_island = path_lower.contains("/islands/") || path_lower.contains("\\islands\\");
        let is_layout = path_lower.contains("_layout");
        let is_middleware = path_lower.contains("_middleware");
        let is_error_page = path_lower.contains("_404") || path_lower.contains("_500");
        
        self.modules.write().insert(path.to_string(), module.clone());
        
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
        
        self.register_components(path, &module);
    }

    fn register_island(&mut self, path: &str, module: &Module) {
        let name = extract_file_name(path, "islands/");
        let mut props_fields = Vec::new();
        let mut props_type = None;
        
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Type(t)) = item {
                if t.name.ends_with("Props") || t.name.ends_with("Interface") {
                    props_type = Some(t.name.clone());
                    if let Type::Object { members } = &t.type_ {
                        props_fields = members.clone();
                    }
                }
            }
        }
        
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let island_info = IslandInfo {
                        file_path: path.to_string(),
                        name: name.clone(),
                        props_type,
                        props_fields,
                        params: decl.params.clone(),
                        body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                    };
                    self.islands.write().insert(name.clone(), island_info);
                    
                    let comp_def = ComponentDef {
                        name: name.clone(),
                        file_path: path.to_string(),
                        params: decl.params.clone(),
                        body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                    };
                    self.components.write().insert(name, comp_def);
                    return;
                }
            }
        }
    }

    fn register_error_page(&mut self, path: &str, module: &Module) {
        let code = if path.contains("_404") { 404 } else { 500 };
        
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let route_key = format!("/_error_{}", code);
                    let handler_info = HandlerInfo {
                        file_path: path.to_string(),
                        methods: HashMap::new(),
                        component_name: Some(decl.name.clone()),
                        props_type: None,
                    };
                    self.handlers.write().insert(route_key, handler_info);
                    self.error_pages.write().insert(code as u16, path.to_string());
                    return;
                }
            }
        }
    }

    fn register_middleware(&mut self, path: &str, module: &Module) {
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let is_global = path.contains("routes/_middleware");
                    let pattern = extract_layout_pattern(path);
                    let middleware = MiddlewareInfo {
                        file_path: path.to_string(),
                        params: decl.params.clone(),
                        body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                        is_async: decl.is_async,
                        is_global,
                        pattern,
                    };
                    self.middleware.write().push(middleware);
                    return;
                }
            }
            
            if let ModuleItem::Export(Export::NamedWithValue { name, value }) = item {
                if name == "handler" {
                    if let Expr::Function { decl } = value {
                        let is_global = path.contains("routes/_middleware");
                        let middleware = MiddlewareInfo {
                            file_path: path.to_string(),
                            params: decl.params.clone(),
                            body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                            is_async: decl.is_async,
                            is_global,
                            pattern: None,
                        };
                        self.middleware.write().push(middleware);
                    }
                }
            }
        }
    }

    fn register_layout(&mut self, path: &str, module: &Module) {
        let pattern = extract_layout_pattern(path).unwrap_or_else(|| "/".to_string());
        let name = extract_file_name(path, "routes/").trim_end_matches("_layout").to_string();
        
        for item in &module.items {
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    let layout_info = LayoutInfo {
                        file_path: path.to_string(),
                        name: decl.name.clone(),
                        pattern: pattern.clone(),
                        params: decl.params.clone(),
                        body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                    };
                    self.layouts.write().insert(pattern, layout_info);
                    return;
                }
            }
        }
    }

    fn register_route(&mut self, path: &str, module: &Module) {
        let route_key = path_to_route_key(path);
        let mut handler_info = HandlerInfo {
            file_path: path.to_string(),
            methods: HashMap::new(),
            component_name: None,
            props_type: None,
        };
        
        let mut props_type = None;
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Type(t)) = item {
                if t.name.ends_with("Data") || t.name.ends_with("Props") {
                    props_type = Some(t.name.clone());
                }
            }
        }
        handler_info.props_type = props_type;
        
        for item in &module.items {
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
            
            if let ModuleItem::Export(Export::Default { expr }) = item {
                if let Expr::Function { decl } = expr {
                    handler_info.component_name = Some(decl.name.clone());
                }
            }
        }
        
        if handler_info.methods.is_empty() || handler_info.component_name.is_some() {
            self.handlers.write().insert(route_key, handler_info);
        }
    }

    fn register_components(&mut self, path: &str, module: &Module) {
        for item in &module.items {
            if let ModuleItem::Decl(Decl::Function(f)) = item {
                if f.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    if !self.components.read().contains_key(&f.name) {
                        let comp_def = ComponentDef {
                            name: f.name.clone(),
                            file_path: path.to_string(),
                            params: f.params.clone(),
                            body: f.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                        };
                        self.components.write().insert(f.name.clone(), comp_def);
                    }
                }
            }
        }
    }
    
    fn get_layout_chain(&self, route_path: &str) -> Vec<LayoutInfo> {
        let mut layouts = Vec::new();
        let layouts_guard = self.layouts.read();
        
        let segments: Vec<&str> = route_path.split('/').filter(|s| !s.is_empty()).collect();
        
        for i in 0..=segments.len() {
            let pattern = if i == 0 {
                "/".to_string()
            } else {
                format!("/{}", segments[..i].join("/"))
            };
            
            if let Some(layout) = layouts_guard.get(&pattern) {
                if !layouts.iter().any(|l: &LayoutInfo| l.file_path == layout.file_path) {
                    layouts.push(layout.clone());
                }
            }
        }
        
        layouts
    }
    
    /// Execute middleware pipeline
    /// 
    /// Middleware can:
    /// - Modify request state
    /// - Return early with a response
    /// - Pass control to next middleware
    fn execute_middleware(&self, request: &RequestInfo, path: &str, state: &mut HashMap<String, Value>) -> Result<Option<String>, String> {
        let middleware_guard = self.middleware.read();
        
        // Collect middleware that matches this path (clone to avoid borrow issues)
        let applicable_middleware: Vec<MiddlewareInfo> = middleware_guard.iter()
            .filter(|mw| mw.is_global || self.middleware_matches_path(mw, path))
            .cloned()
            .collect();
        
        drop(middleware_guard);
        
        // Execute middleware chain
        for mw in &applicable_middleware {
            let mut ctx = EvalContext::default();
            ctx.url = request.url.clone();
            ctx.request = Some(request.clone());
            ctx.state = state.clone();
            
            // Execute middleware body
            let result = self.execute_middleware_body(mw, &mut ctx);
            
            match result {
                MiddlewareResult::Continue => {
                    // Update state from middleware
                    *state = ctx.state;
                }
                MiddlewareResult::Response(html) => {
                    return Ok(Some(html));
                }
                MiddlewareResult::Next => {
                    // Continue to next middleware
                    *state = ctx.state;
                }
            }
        }
        
        Ok(None)
    }
    
    /// Check if middleware matches the given path
    fn middleware_matches_path(&self, middleware: &MiddlewareInfo, path: &str) -> bool {
        if let Some(pattern) = &middleware.pattern {
            // Simple prefix matching
            path.starts_with(pattern)
        } else {
            true
        }
    }
    
    /// Execute middleware body and return result
    fn execute_middleware_body(&self, middleware: &MiddlewareInfo, ctx: &mut EvalContext) -> MiddlewareResult {
        for stmt in &middleware.body {
            match stmt {
                Stmt::Return { arg: Some(expr) } => {
                    // Check for return response
                    if let Expr::New { callee, args, .. } = expr {
                        if let Expr::Ident { name } = callee.as_ref() {
                            if name == "Response" {
                                if let Some(body) = args.first() {
                                    if let Ok(val) = self.expr_to_value(body, ctx) {
                                        return MiddlewareResult::Response(val.to_string());
                                    }
                                }
                                return MiddlewareResult::Response(String::new());
                            }
                        }
                    }
                }
                
                Stmt::Variable { decl } => {
                    if let Some(init) = &decl.init {
                        // Check for ctx.state assignment
                        if let Expr::Assign { left, right, .. } = init {
                            if let Expr::Member { object, property, .. } = left.as_ref() {
                                if let Expr::Ident { name: obj_name } = object.as_ref() {
                                    if obj_name == "ctx" {
                                        if let Expr::Ident { name: prop_name } = property.as_ref() {
                                            if prop_name == "state" {
                                                if let Ok(val) = self.expr_to_value(right, ctx) {
                                                    ctx.state.insert("middleware_state".to_string(), val);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else if let Ok(val) = self.expr_to_value(init, ctx) {
                            ctx.state.insert(decl.name.clone(), val);
                        }
                    }
                }
                
                // Check for await next() pattern
                Stmt::Expr { expr } => {
                    if let Expr::Await { arg } = expr {
                        if let Expr::Call { callee, .. } = arg.as_ref() {
                            if let Expr::Ident { name } = callee.as_ref() {
                                if name == "next" {
                                    return MiddlewareResult::Next;
                                }
                            }
                        }
                    }
                }
                
                _ => {}
            }
        }
        
        MiddlewareResult::Continue
    }
    
    /// Execute a route with full handler and component rendering
    pub fn execute_route(&self, route_path: &str, method: &str, params: HashMap<String, String>, request: RequestInfo) -> Result<RenderResult, String> {
        self.execute_route_with_mode(route_path, method, params, request, ExecMode::Full)
    }
    
    /// Execute a route with configurable mode
    pub fn execute_route_with_mode(&self, route_path: &str, method: &str, params: HashMap<String, String>, request: RequestInfo, mode: ExecMode) -> Result<RenderResult, String> {
        let route_key = route_path.to_string();
        
        // Execute middleware pipeline
        let mut middleware_state = HashMap::new();
        if let Some(response) = self.execute_middleware(&request, &route_key, &mut middleware_state)? {
            return Ok(RenderResult {
                html: response,
                page_data: Value::Object(middleware_state),
                islands: vec![],
                status: 200,
            });
        }
        
        // Look up handler
        let handler = match self.handlers.read().get(&route_key).cloned() {
            Some(h) => h,
            None => {
                // Try error pages
                return self.handle_error(404, route_path, request);
            }
        };
        
        let mut ctx = EvalContext {
            scope: HashMap::new(),
            params: params.clone(),
            url: request.url.clone(),
            rendered_islands: Vec::new(),
            state: middleware_state.clone(),
            request: Some(request.clone()),
        };
        
        // Populate scope with params and middleware state
        for (k, v) in &ctx.params {
            ctx.scope.insert(k.clone(), Value::String(v.clone()));
        }
        for (k, v) in &middleware_state {
            ctx.scope.insert(k.clone(), v.clone());
        }
        
        let method_upper = method.to_uppercase();
        let mut page_data: Value = Value::Object(HashMap::new());
        let mut render_result: Option<String> = None;
        
        // Execute handler based on mode
        if mode == ExecMode::Full || mode == ExecMode::HandlerOnly {
            if let Some(handler_method) = handler.methods.get(&method_upper) {
                ctx.scope.insert("_handler_called".to_string(), Value::Bool(true));
                
                // Execute handler and check for Response/ctx.render
                let handler_result = self.execute_handler_full(handler_method, &handler, &mut ctx, route_path)?;
                
                if let Some(result) = handler_result {
                    // Handler returned a Response or rendered content
                    return Ok(result);
                }
                
                // Get page data from handler execution
                page_data = ctx.state.get("_page_data").cloned().unwrap_or(Value::Object(HashMap::new()));
            }
        }
        
        // Render component if needed
        if mode == ExecMode::Full || mode == ExecMode::RenderOnly {
            if let Some(component_name) = &handler.component_name {
                // Add data to props
                ctx.scope.insert("data".to_string(), page_data.clone());
                
                render_result = Some(self.render_component(component_name, &ctx)?);
            } else {
                render_result = Some(String::new());
            }
        }
        
        // Apply layouts
        let full_html = if let Some(html) = render_result {
            let layout_chain = self.get_layout_chain(&route_key);
            self.apply_layouts(&html, &layout_chain, &ctx)?
        } else {
            String::new()
        };
        
        Ok(RenderResult {
            html: full_html,
            page_data,
            islands: ctx.rendered_islands,
            status: 200,
        })
    }
    
    /// Execute handler body and check for Response/ctx.render
    fn execute_handler_full(&self, handler: &HandlerMethod, handler_info: &HandlerInfo, ctx: &mut EvalContext, route_path: &str) -> Result<Option<RenderResult>, String> {
        for stmt in &handler.body {
            match stmt {
                Stmt::Return { arg: Some(expr) } => {
                    // Check for new Response(...) 
                    if let Expr::New { callee, args, .. } = expr {
                        if let Expr::Ident { name } = callee.as_ref() {
                            if name == "Response" {
                                return self.handle_response(args, ctx);
                            }
                        }
                    }
                    
                    // Check for ctx.render(...)
                    if let Expr::Call { callee, args, .. } = expr {
                        if let Expr::Member { object, property, .. } = callee.as_ref() {
                            if let Expr::Ident { name: obj_name } = object.as_ref() {
                                if obj_name == "ctx" {
                                    if let Expr::Ident { name: prop_name } = property.as_ref() {
                                        if prop_name == "render" {
                                            // ctx.render() - return None to continue with component rendering
                                            if !args.is_empty() {
                                                // If data passed, store it
                                                if let Ok(data) = self.expr_to_value(&args[0], ctx) {
                                                    ctx.state.insert("_page_data".to_string(), data);
                                                }
                                            }
                                            return Ok(None);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    // Otherwise, check if it's an object with data that we should capture
                    if let Expr::Object { props } = expr {
                        for prop in props {
                            if let ObjectProp::Init { key: PropKey::Ident(key), value } = prop {
                                if key == "json" || key == "data" {
                                    if let Ok(data) = self.expr_to_value(value, ctx) {
                                        ctx.state.insert("_page_data".to_string(), data);
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Handle variable declarations that might set state
                Stmt::Variable { decl } => {
                    if let Some(init) = &decl.init {
                        if let Ok(val) = self.expr_to_value(init, ctx) {
                            ctx.state.insert(decl.name.clone(), val);
                        }
                    }
                }
                
                _ => {}
            }
        }
        
        Ok(None)
    }
    
    /// Handle Response constructor
    fn handle_response(&self, args: &[Expr], ctx: &EvalContext) -> Result<Option<RenderResult>, String> {
        if args.is_empty() {
            return Ok(Some(RenderResult {
                html: String::new(),
                page_data: Value::Object(HashMap::new()),
                islands: vec![],
                status: 200,
            }));
        }
        
        // First arg could be body (string or object) or options
        let first_arg = &args[0];
        let status = 200;
        let content_type = "text/html";
        
        let body = match first_arg {
            Expr::String(s) => s.clone(),
            Expr::Template { parts, exprs } => {
                let mut result = String::new();
                for (i, part) in parts.iter().enumerate() {
                    if let TemplatePart::String(s) = part {
                        result.push_str(s);
                    }
                    if i < exprs.len() {
                        if let Ok(val) = self.expr_to_value(&exprs[i], ctx) {
                            result.push_str(&val.to_string());
                        }
                    }
                }
                result
            }
            Expr::Object { props } => {
                // Could be options object
                let mut body_str = String::new();
                for prop in props {
                    if let ObjectProp::Init { key: PropKey::Ident(key), value } = prop {
                        if key == "body" {
                            if let Ok(val) = self.expr_to_value(value, ctx) {
                                body_str = val.to_string();
                            }
                        }
                    }
                }
                body_str
            }
            _ => {
                if let Ok(val) = self.expr_to_value(first_arg, ctx) {
                    val.to_string()
                } else {
                    String::new()
                }
            }
        };
        
        Ok(Some(RenderResult {
            html: body,
            page_data: Value::Object(HashMap::new()),
            islands: vec![],
            status,
        }))
    }
    
    /// Handle error pages
    fn handle_error(&self, status: u16, route_path: &str, request: RequestInfo) -> Result<RenderResult, String> {
        let error_pages = self.error_pages.read();
        
        if let Some(error_path) = error_pages.get(&status) {
            drop(error_pages);
            
            // Execute the error handler
            let mut ctx = EvalContext::default();
            ctx.url = request.url.clone();
            ctx.request = Some(request);
            ctx.scope.insert("url".to_string(), Value::String(route_path.to_string()));
            ctx.scope.insert("status".to_string(), Value::Number(status as f64));
            
            let html = if let Some(comp_def) = self.components.read().values().find(|c| c.file_path.contains(&format!("_{}", status))) {
                self.render_function_component_by_path(&comp_def.file_path, &comp_def.name, &ctx)?
            } else {
                self.default_error_page(status, route_path)
            };
            
            return Ok(RenderResult {
                html,
                page_data: Value::Object(HashMap::new()),
                islands: vec![],
                status,
            });
        }
        
        // No error page found, return default
        Ok(RenderResult {
            html: self.default_error_page(status, route_path),
            page_data: Value::Object(HashMap::new()),
            islands: vec![],
            status,
        })
    }
    
    /// Generate default error page
    fn default_error_page(&self, status: u16, path: &str) -> String {
        let title = match status {
            404 => "Page Not Found",
            500 => "Internal Server Error",
            _ => "Error",
        };
        let message = match status {
            404 => format!("The page '{}' could not be found.", path),
            500 => "An unexpected error occurred.".to_string(),
            _ => format!("Error {} occurred.", status),
        };
        
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{status} - {title}</title>
    <style>
        body {{ font-family: system-ui, sans-serif; text-align: center; padding: 4rem; background: #f5f5f5; }}
        h1 {{ font-size: 6rem; color: #333; margin: 0; }}
        h2 {{ font-size: 2rem; color: #666; margin: 1rem 0; }}
        p {{ color: #888; }}
        a {{ color: #1a1a2e; }}
    </style>
</head>
<body>
    <h1>{status}</h1>
    <h2>{title}</h2>
    <p>{message}</p>
    <p><a href="/">← Go home</a></p>
</body>
</html>"#, status = status, title = title, message = message)
    }
    
    /// Render a function component by its file path
    fn render_function_component_by_path(&self, file_path: &str, name: &str, ctx: &EvalContext) -> Result<String, String> {
        let module = self.modules.read().get(file_path).cloned();
        
        if let Some(module) = module {
            for item in &module.items {
                if let ModuleItem::Decl(Decl::Function(f)) = item {
                    if &f.name == name {
                        let comp_def = ComponentDef {
                            name: name.to_string(),
                            file_path: file_path.to_string(),
                            params: f.params.clone(),
                            body: f.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                        };
                        return self.render_function_component(f, &comp_def, ctx);
                    }
                }
                if let ModuleItem::Export(Export::Default { expr }) = item {
                    if let Expr::Function { decl } = expr {
                        if &decl.name == name {
                            let comp_def = ComponentDef {
                                name: name.to_string(),
                                file_path: file_path.to_string(),
                                params: decl.params.clone(),
                                body: decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
                            };
                            return self.render_function_component(decl, &comp_def, ctx);
                        }
                    }
                }
            }
        }
        
        Ok(String::new())
    }
    
    fn execute_handler_body(&self, handler: &HandlerMethod, ctx: &EvalContext) -> Option<Value> {
        for stmt in &handler.body {
            if let Stmt::Return { arg: Some(expr) } = stmt {
                if let Expr::New { callee, args, .. } = expr {
                    if let Expr::Ident { name } = callee.as_ref() {
                        if name == "Response" && !args.is_empty() {
                            if let Some(first_arg) = args.first() {
                                return self.expr_to_value(first_arg, ctx).ok();
                            }
                        }
                    }
                }
                
                if let Expr::Call { callee, args, .. } = expr {
                    if let Expr::Member { object, property, .. } = callee.as_ref() {
                        if let Expr::Ident { name: obj_name } = object.as_ref() {
                            if obj_name == "ctx" {
                                if let Expr::Ident { name: prop_name } = property.as_ref() {
                                    if prop_name == "render" && !args.is_empty() {
                                        return self.expr_to_value(&args[0], ctx).ok();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
    
    fn render_component(&self, name: &str, ctx: &EvalContext) -> Result<String, String> {
        let comp = self.components.read().get(name).cloned();
        
        if let Some(comp_def) = comp {
            let module = self.modules.read().get(&comp_def.file_path).cloned();
            if let Some(module) = module {
                for item in &module.items {
                    if let ModuleItem::Decl(Decl::Function(f)) = item {
                        if &f.name == name {
                            return self.render_function_component(f, &comp_def, ctx);
                        }
                    }
                    if let ModuleItem::Export(Export::Default { expr }) = item {
                        if let Expr::Function { decl } = expr {
                            if &decl.name == name {
                                return self.render_function_component(decl, &comp_def, ctx);
                            }
                        }
                    }
                }
            }
        }
        
        Ok(format!("<div class=\"component-{}\">Component {}</div>", name.to_lowercase(), name))
    }
    
    fn render_function_component(&self, f: &FunctionDecl, comp_def: &ComponentDef, ctx: &EvalContext) -> Result<String, String> {
        let body = match &f.body {
            Some(b) => &b.0,
            None => return Ok(String::new()),
        };
        
        for stmt in body {
            if let Stmt::Return { arg: Some(expr) } = stmt {
                return self.evaluate_expr_to_html(expr, ctx);
            }
        }
        
        Ok(String::new())
    }
    
    fn apply_layouts(&self, content: &str, layouts: &[LayoutInfo], ctx: &EvalContext) -> Result<String, String> {
        let mut result = content.to_string();
        
        for layout in layouts.iter().rev() {
            let module = self.modules.read().get(&layout.file_path).cloned();
            if let Some(module) = module {
                for item in &module.items {
                    if let ModuleItem::Decl(Decl::Function(f)) = item {
                        if &f.name == &layout.name {
                            let mut layout_ctx = ctx.clone();
                            layout_ctx.scope.insert("children".to_string(), Value::String(result.clone()));
                            
                            result = self.render_function_component(f, &ComponentDef {
                                name: layout.name.clone(),
                                file_path: layout.file_path.clone(),
                                params: layout.params.clone(),
                                body: layout.body.clone(),
                            }, &layout_ctx)?;
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(result)
    }
    
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
                if let Expr::Ident { name } = callee.as_ref() {
                    let value = self.call_hook_or_function(name, args, ctx)?;
                    return Ok(value.to_string());
                }
                Ok(String::new())
            }
            Expr::Array { elems } => {
                let mut html = String::new();
                for elem in elems {
                    if let Some(e) = elem {
                        if let Expr::Spread { arg } = e {
                            if let Value::Array(items) = self.expr_to_value(arg, ctx)? {
                                for item in items {
                                    html.push_str(&item.to_string());
                                }
                            }
                        } else {
                            html.push_str(&self.evaluate_expr_to_html(e, ctx)?);
                        }
                    }
                }
                Ok(html)
            }
            _ => {
                if let Ok(value) = self.expr_to_value(expr, ctx) {
                    Ok(value.to_string())
                } else {
                    Ok(String::new())
                }
            }
        }
    }
    
    fn call_hook_or_function(&self, name: &str, args: &[Expr], ctx: &EvalContext) -> Result<Value, String> {
        match name {
            "useState" => {
                let initial = args.first()
                    .and_then(|a| self.expr_to_value(a, ctx).ok())
                    .unwrap_or(Value::Number(0.0));
                Ok(Value::Array(vec![initial, Value::Function("setState".to_string())]))
            }
            "useEffect" => Ok(Value::Undefined),
            "useRef" => Ok(Value::Object(HashMap::new())),
            "useMemo" => {
                let val = args.first()
                    .and_then(|a| self.expr_to_value(a, ctx).ok())
                    .unwrap_or(Value::Undefined);
                Ok(val)
            }
            "useCallback" => {
                let val = args.get(0)
                    .and_then(|a| self.expr_to_value(a, ctx).ok())
                    .unwrap_or(Value::Undefined);
                Ok(val)
            }
            "useContext" => Ok(Value::Undefined),
            "useId" => Ok(Value::String(format!("id-{}", rand_id()))),
            "useSignal" => {
                let initial = args.first()
                    .and_then(|a| self.expr_to_value(a, ctx).ok())
                    .unwrap_or(Value::Number(0.0));
                let mut obj = HashMap::new();
                obj.insert("value".to_string(), initial);
                obj.insert("_type".to_string(), Value::String("signal".to_string()));
                Ok(Value::Object(obj))
            }
            "useComputed" => {
                let val = args.first()
                    .and_then(|a| self.expr_to_value(a, ctx).ok())
                    .unwrap_or(Value::Undefined);
                Ok(val)
            }
            "useSignalEffect" => Ok(Value::Undefined),
            "map" => {
                let method_self = ctx.scope.get("_this").cloned().unwrap_or(Value::Undefined);
                if let Value::Array(arr) = method_self {
                    let callback = args.first();
                    if let Some(Expr::Arrow { params, body, .. }) = callback {
                        let mut results = Vec::new();
                        for (i, item) in arr.iter().enumerate() {
                            let mut item_ctx = ctx.clone();
                            if let Some(param_name) = params.first().map(|p| p.name.clone()) {
                                item_ctx.scope.insert(param_name, item.clone());
                            }
                            if params.len() > 1 {
                                item_ctx.scope.insert("index".to_string(), Value::Number(i as f64));
                            }
                            if let Stmt::Block(stmts) = body.as_ref() {
                                for stmt in stmts {
                                    if let Stmt::Return { arg: Some(ret_expr) } = stmt {
                                        let html = self.evaluate_expr_to_html(ret_expr, &mut item_ctx)?;
                                        results.push(html);
                                    }
                                }
                            } else if let Stmt::Return { arg: Some(ret_expr) } = body.as_ref() {
                                let html = self.evaluate_expr_to_html(ret_expr, &mut item_ctx)?;
                                results.push(html);
                            } else if let Stmt::Expr { expr } = body.as_ref() {
                                let html = self.evaluate_expr_to_html(expr, &mut item_ctx)?;
                                if !html.is_empty() {
                                    results.push(html);
                                }
                            }
                        }
                        return Ok(Value::String(results.join("")));
                    }
                }
                Ok(Value::Undefined)
            }
            "join" => {
                let method_self = ctx.scope.get("_this").cloned().unwrap_or(Value::Undefined);
                if let Value::Array(arr) = method_self {
                    let separator = args.first()
                        .and_then(|a| self.expr_to_value(a, ctx).ok())
                        .unwrap_or(Value::String(String::new()));
                    let sep_str = if let Value::String(s) = separator { s } else { String::new() };
                    return Ok(Value::String(arr.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(&sep_str)));
                }
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
                    if let Value::Object(props) = self.expr_to_value(expr, ctx)? {
                        for (k, v) in props {
                            vnode.attrs.insert(k, v);
                        }
                    }
                }
                _ => {}
            }
        }

        if is_component {
            let islands_guard = self.islands.read();
            if let Some(_island) = islands_guard.get(&tag) {
                drop(islands_guard);
                return self.render_island(&tag, &vnode.attrs, &mut ctx.clone());
            }
        }

        for child in &jsx.children {
            match child {
                JSXChild::Text(s) => {
                    if !s.trim().is_empty() {
                        vnode.children.push(s.clone());
                    }
                }
                JSXChild::Expr(e) => {
                    if let Expr::Call { callee, .. } = e {
                        if let Expr::Member { object, property, .. } = callee.as_ref() {
                            let obj_val = self.expr_to_value(object, ctx)?;
                            if let Expr::Ident { name: method_name } = property.as_ref().deref() {
                                if let Some(method) = obj_val.get_member(method_name) {
                                    let mut method_ctx = ctx.clone();
                                    method_ctx.scope.insert("_this".to_string(), obj_val);
                                    if let Value::String(result) = self.call_hook_or_function("map", &[], &method_ctx)? {
                                        vnode.children.push(result);
                                    }
                                }
                            }
                        }
                    } else {
                        let val = self.evaluate_expr_to_html(e, ctx)?;
                        if !val.is_empty() {
                            vnode.children.push(val);
                        }
                    }
                }
                JSXChild::JSX(inner_jsx) => {
                    let child_html = self.jsx_to_html(inner_jsx, ctx)?;
                    if !child_html.is_empty() {
                        vnode.children.push(child_html);
                    }
                }
                JSXChild::Fragment { children } => {
                    for child in children {
                        if let JSXChild::Text(s) = child {
                            if !s.trim().is_empty() {
                                vnode.children.push(s.clone());
                            }
                        } else if let JSXChild::JSX(inner_jsx) = child {
                            let child_html = self.jsx_to_html(inner_jsx, ctx)?;
                            if !child_html.is_empty() {
                                vnode.children.push(child_html);
                            }
                        } else if let JSXChild::Expr(e) = child {
                            let val = self.evaluate_expr_to_html(e, ctx)?;
                            if !val.is_empty() {
                                vnode.children.push(val);
                            }
                        }
                    }
                }
                JSXChild::Spread { expr } => {
                    if let Ok(Value::Array(items)) = self.expr_to_value(expr, ctx) {
                        for item in items {
                            vnode.children.push(item.to_string());
                        }
                    }
                }
            }
        }

        Ok(vnode.to_html_string())
    }
    
    fn render_island(&self, name: &str, attrs: &HashMap<String, Value>, ctx: &mut EvalContext) -> Result<String, String> {
        let islands_guard = self.islands.read();
        let island = islands_guard.get(name).cloned()
            .ok_or_else(|| format!("Island not found: {}", name))?;
        drop(islands_guard);
        
        let island_id = generate_island_id();
        
        // Extract props and determine hydration strategy
        let mut props_map = serde_json::Map::new();
        let mut hydrate = HydrationStrategy::Load;
        
        for (k, v) in attrs {
            match k.as_str() {
                "hydrate" | "hydration" => {
                    if let Value::String(s) = v {
                        hydrate = match s.to_lowercase().as_str() {
                            "visible" => HydrationStrategy::Visible,
                            "interaction" => HydrationStrategy::Interaction,
                            "idle" => HydrationStrategy::Idle,
                            _ => HydrationStrategy::Load,
                        };
                    }
                }
                _ => {
                    props_map.insert(k.clone(), v.to_json());
                }
            }
        }
        let props_json = serde_json::to_string(&props_map).unwrap_or_default();
        
        // Server-render the island first (for SSR)
        let server_html = self.render_island_content(&island, attrs, ctx)?;
        
        let hydrate_attr = match hydrate {
            HydrationStrategy::Load => "load",
            HydrationStrategy::Visible => "visible",
            HydrationStrategy::Interaction => "interaction",
            HydrationStrategy::Idle => "idle",
        };
        
        let placeholder_html = format!(
            r#"<div data-island="{name}" data-id="{id}" data-props='{props}' data-hydrate="{hydrate}">{content}</div>"#,
            name = name,
            id = island_id,
            props = props_json,
            hydrate = hydrate_attr,
            content = server_html
        );
        
        ctx.rendered_islands.push(RenderedIsland {
            name: name.to_string(),
            props: attrs.clone(),
            html: placeholder_html.clone(),
            id: island_id,
            props_json,
            hydrate,
        });
        
        Ok(placeholder_html)
    }
    
    /// Render island content on the server
    fn render_island_content(&self, island: &IslandInfo, attrs: &HashMap<String, Value>, ctx: &EvalContext) -> Result<String, String> {
        // Create props object from attributes
        let mut props_ctx = ctx.clone();
        for (k, v) in attrs {
            props_ctx.scope.insert(k.clone(), v.clone());
        }
        
        // Find and execute the island component
        for item in &island.body {
            if let Stmt::Return { arg: Some(expr) } = item {
                return self.evaluate_expr_to_html(expr, &props_ctx);
            }
        }
        
        Ok(String::new())
    }

    fn expr_to_value(&self, expr: &Expr, ctx: &EvalContext) -> Result<Value, String> {
        match expr {
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Null => Ok(Value::Null),
            Expr::Boolean(b) => Ok(Value::Bool(*b)),
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Ident { name } => {
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
                        match (&l, &r) {
                            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
                            (Value::String(a), _) => Ok(Value::String(format!("{}{}", a, r.to_string()))),
                            (_, Value::String(b)) => Ok(Value::String(format!("{}{}", l.to_string(), b))),
                            _ => Ok(Value::Number(l.as_number() + r.as_number())),
                        }
                    }
                    BinaryOp::Sub => Ok(Value::Number(l.as_number() - r.as_number())),
                    BinaryOp::Mul => Ok(Value::Number(l.as_number() * r.as_number())),
                    BinaryOp::Div => Ok(Value::Number(l.as_number() / r.as_number())),
                    BinaryOp::Eq | BinaryOp::EqStrict => Ok(Value::Bool(l == r)),
                    BinaryOp::Ne | BinaryOp::NeStrict => Ok(Value::Bool(l != r)),
                    BinaryOp::Lt => Ok(Value::Bool(l.as_number() < r.as_number())),
                    BinaryOp::Le => Ok(Value::Bool(l.as_number() <= r.as_number())),
                    BinaryOp::Gt => Ok(Value::Bool(l.as_number() > r.as_number())),
                    BinaryOp::Ge => Ok(Value::Bool(l.as_number() >= r.as_number())),
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
                    return self.call_hook_or_function(name, args, ctx);
                }
                if let Expr::Member { object, property, .. } = callee.as_ref() {
                    let obj_val = self.expr_to_value(object, ctx)?;
                    let mut method_ctx = ctx.clone();
                    method_ctx.scope.insert("_this".to_string(), obj_val);
                    
                    let method_name = if let Expr::Ident { name } = property.as_ref() {
                        name.clone()
                    } else {
                        return Ok(Value::Undefined);
                    };
                    
                    return self.call_hook_or_function(&method_name, args, &method_ctx);
                }
                Ok(Value::Undefined)
            }
            Expr::Unary { op, arg, .. } => {
                let arg_val = self.expr_to_value(arg, ctx)?;
                match op {
                    UnaryOp::Minus => Ok(Value::Number(-arg_val.as_number())),
                    UnaryOp::Plus => Ok(Value::Number(arg_val.as_number())),
                    UnaryOp::Not => Ok(Value::Bool(!arg_val.as_bool())),
                    _ => Ok(Value::Undefined),
                }
            }
            Expr::Assign { op: _, left, right } => {
                // Note: Assignment side effects are not stored for SSR
                // This is a simplification - full JS semantics would require mutation tracking
                self.expr_to_value(right, ctx)
            }
            Expr::Template { parts, exprs } => {
                let mut result = String::new();
                for (i, part) in parts.iter().enumerate() {
                    if let TemplatePart::String(s) = part {
                        result.push_str(s);
                    }
                    if i < exprs.len() {
                        let val = self.expr_to_value(&exprs[i], ctx)?;
                        result.push_str(&val.to_string());
                    }
                }
                Ok(Value::String(result))
            }
            Expr::Member { object, property, computed, .. } => {
                let obj_val = self.expr_to_value(object, ctx)?;
                let key = if let Expr::Ident { name } = property.as_ref() {
                    name.clone()
                } else {
                    return Ok(Value::Undefined);
                };
                if *computed {
                    Ok(obj_val.get_member(&key).unwrap_or(Value::Undefined))
                } else {
                    Ok(obj_val.get_member(&key).unwrap_or(Value::Undefined))
                }
            }
            _ => Ok(Value::Undefined),
        }
    }

    pub fn get_island_manifest(&self) -> serde_json::Value {
        let islands = self.islands.read();
        serde_json::json!({
            "islands": islands.values().map(|i| {
                serde_json::json!({
                    "name": i.name,
                    "props": i.props_fields.iter().map(|m| m.key.clone()).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>()
        })
    }
}



fn rand_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", nanos)
}

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

fn extract_layout_pattern(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    
    if let Some(routes_pos) = path.find("/routes/") {
        let route_part = &path[routes_pos + 8..];
        let route = route_part
            .trim_start_matches('/')
            .trim_end_matches("_layout.tsx")
            .trim_end_matches("_layout.ts");
        
        if route.is_empty() {
            Some("/".to_string())
        } else {
            Some(format!("/{}", route.trim_end_matches('/')))
        }
    } else {
        None
    }
}

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

/// Result of a route render
#[derive(Clone)]
pub struct RenderResult {
    /// Rendered HTML
    pub html: String,
    /// Page data from handler
    pub page_data: Value,
    /// Rendered islands for client hydration
    pub islands: Vec<RenderedIsland>,
    /// HTTP status code
    pub status: u16,
}

impl std::fmt::Debug for RenderResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderResult")
            .field("html", &self.html.chars().take(100).collect::<String>())
            .field("page_data", &self.page_data)
            .field("islands", &self.islands.iter().map(|i| &i.name).collect::<Vec<_>>())
            .field("status", &self.status)
            .finish()
    }
}
