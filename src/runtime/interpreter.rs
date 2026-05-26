//! HIR Interpreter for Development Mode
//!
//! Executes HIR directly without Rust code generation.
//! This enables instant hot-reload in development mode.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::transpile::hir::*;

/// Evaluation context for expression evaluation
#[derive(Debug, Clone)]
pub struct EvalContext {
    scope: HashMap<String, Value>,
    hooks: HookState,
    params: HashMap<String, String>,
    url: String,
    page_data: serde_json::Value,
}

impl EvalContext {
    pub fn new(params: HashMap<String, String>, url: String) -> Self {
        Self {
            scope: HashMap::new(),
            hooks: HookState::new(),
            params,
            url,
            page_data: serde_json::json!({}),
        }
    }

    pub fn set_page_data(&mut self, data: serde_json::Value) {
        self.page_data = data;
    }

    pub fn get_page_data(self) -> serde_json::Value {
        self.page_data
    }
}

/// Hook state management
#[derive(Debug, Clone, Default)]
pub struct HookState {
    state: Vec<Value>,
    refs: Vec<Value>,
}

impl HookState {
    pub fn new() -> Self {
        Self::default()
    }
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
    VNode(Box<VNode>),
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
            Value::VNode(vnode) => vnode.to_html_string(),
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Null | Value::Undefined => false,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(_) | Value::Function(_) | Value::VNode(_) => true,
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

    pub fn index(&self, index: f64) -> Option<Value> {
        match self {
            Value::Array(arr) => {
                let idx = index as usize;
                if idx < arr.len() { Some(arr[idx].clone()) } else { None }
            }
            Value::String(s) => {
                let idx = index as usize;
                s.chars().nth(idx).map(|c| Value::String(c.to_string()))
            }
            _ => None,
        }
    }
}

/// Virtual node for rendering
#[derive(Debug, Clone, PartialEq)]
pub enum VNode {
    Element {
        tag: String,
        attrs: HashMap<String, Value>,
        children: Vec<VNode>,
        key: Option<String>,
    },
    Text(String),
    Fragment(Vec<VNode>),
    Component {
        name: String,
        props: Value,
    },
    Empty,
}

impl VNode {
    pub fn to_html_string(&self) -> String {
        match self {
            VNode::Empty => String::new(),
            VNode::Text(s) => html_escape(s),
            VNode::Fragment(children) => children.iter().map(|c| c.to_html_string()).collect(),
            VNode::Element { tag, attrs, children, .. } => {
                let mut html = format!("<{}", tag);
                for (key, value) in attrs {
                    match value {
                        Value::Bool(true) => { html.push_str(&format!(" {}", key)); }
                        Value::String(s) if !s.is_empty() => { html.push_str(&format!(" {}=\"{}\"", key, html_escape_attr(s))); }
                        Value::Number(n) => { html.push_str(&format!(" {}=\"{}\"", key, n)); }
                        _ => {}
                    }
                }
                if children.is_empty() {
                    html.push_str("/>");
                } else {
                    html.push('>');
                    for child in children { html.push_str(&child.to_html_string()); }
                    html.push_str(&format!("</{}>", tag));
                }
                html
            }
            VNode::Component { name, props } => {
                let props_str = match props {
                    Value::Object(obj) => obj.iter().map(|(k, v)| format!("{}=\"{}\"", k, v.to_string())).collect::<Vec<_>>().join(" "),
                    _ => String::new(),
                };
                format!("<div data-component=\"{}\" data-props=\"{}\">Component {}</div>", name, html_escape_attr(&props_str), name)
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// HIR Interpreter
pub struct Interpreter {
    components: HashMap<String, ComponentDef>,
    handlers: HashMap<String, HandlerDef>,
}

struct ComponentDef {
    name: String,
    params: Vec<String>,
    body: Vec<Stmt>,
}

struct HandlerDef {
    name: String,
    methods: Vec<String>,
    body: Vec<Stmt>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            handlers: HashMap::new(),
        }
    }

    pub fn load_module(&mut self, module: &Module) {
        for item in &module.items {
            match item {
                ModuleItem::Decl(Decl::Function(f)) => {
                    if f.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        let params: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                        let body = f.body.as_ref().map(|b| b.0.clone()).unwrap_or_default();
                        self.components.insert(f.name.clone(), ComponentDef { name: f.name.clone(), params, body });
                    }
                }
                ModuleItem::Export(Export::Default { expr }) => {
                    if let Expr::Function { decl } = expr {
                        let params: Vec<String> = decl.params.iter().map(|p| p.name.clone()).collect();
                        let body = decl.body.as_ref().map(|b| b.0.clone()).unwrap_or_default();
                        self.handlers.insert("default".to_string(), HandlerDef { name: decl.name.clone(), methods: vec!["GET".to_string()], body });
                    }
                }
                _ => {}
            }
        }
    }

    pub fn load_file(&mut self, _path: &Path, source: &str) -> Result<(), String> {
        let mut parser = crate::transpile::Parser::new();
        let module = parser.parse_source(source).map_err(|e| e.to_string())?;
        self.load_module(&module);
        Ok(())
    }

    pub fn execute_handler(&self, path: &str, params: HashMap<String, String>) -> Result<serde_json::Value, String> {
        let url = format!("http://localhost{}", path);
        let mut ctx = EvalContext::new(params, url);

        if let Some(handler) = self.handlers.get("default") {
            let mut props = HashMap::new();
            for (key, value) in &ctx.params {
                props.insert(key.clone(), Value::String(value.clone()));
            }
            props.insert("url".to_string(), Value::String(ctx.url.clone()));

            let _ = self.execute_stmts(&handler.body, &mut ctx, Value::Object(props));
        }

        Ok(ctx.get_page_data())
    }

    pub fn render_component(&self, name: &str, _props: Value) -> String {
        format!("<div data-component=\"{}\">Component rendered</div>", name)
    }

    fn execute_stmts(&self, stmts: &[Stmt], ctx: &mut EvalContext, _this: Value) -> Option<Value> {
        let mut result = None;
        for stmt in stmts {
            result = self.execute_stmt(stmt, ctx);
            if result.is_some() { return result; }
        }
        result
    }

    fn execute_stmt(&self, stmt: &Stmt, ctx: &mut EvalContext) -> Option<Value> {
        match stmt {
            Stmt::Empty => None,
            Stmt::Block(stmts) => self.execute_stmts(stmts, ctx, Value::Null),
            Stmt::Expr { expr } => { let _ = self.evaluate_expr(expr, ctx); None }
            Stmt::Return { arg } => arg.as_ref().map(|e| self.evaluate_expr(e, ctx).ok()).flatten(),
            Stmt::Variable { decl } => {
                let init = match decl.init.as_ref() {
                    Some(e) => self.evaluate_expr(e, ctx).unwrap_or(Value::Undefined),
                    None => Value::Undefined,
                };
                ctx.scope.insert(decl.name.clone(), init);
                None
            }
            Stmt::If { test, consequent, alternate } => {
                if self.evaluate_expr(test, ctx).map(|v| v.as_bool()).unwrap_or(false) {
                    self.execute_stmt(consequent, ctx)
                } else if let Some(alt) = alternate {
                    self.execute_stmt(alt, ctx)
                } else { None }
            }
            Stmt::While { test, body } => {
                while self.evaluate_expr(test, ctx).map(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(v) = self.execute_stmt(body, ctx) { return Some(v); }
                }
                None
            }
            Stmt::For { init, test, update, body } => {
                if let Some(ForInit::Variable(v)) = init {
                    if let Some(init_expr) = v.init.as_ref() {
                        let value = self.evaluate_expr(init_expr, ctx).unwrap_or(Value::Undefined);
                        ctx.scope.insert(v.name.clone(), value);
                    }
                }
                while test.as_ref().map(|t| self.evaluate_expr(t, ctx).map(|v| v.as_bool()).unwrap_or(false)).unwrap_or(true) {
                    if let Some(v) = self.execute_stmt(body, ctx) { return Some(v); }
                    if let Some(update) = update { let _ = self.evaluate_expr(update, ctx); }
                }
                None
            }
            Stmt::Throw { arg } => Some(Value::String(format!("Throw: {}", self.evaluate_expr(arg, ctx).map(|v| v.to_string()).unwrap_or_else(|e| e)))),
            Stmt::Break { .. } => Some(Value::Undefined),
            Stmt::Continue { .. } => Some(Value::Undefined),
            Stmt::Try { block, handler, finalizer } => {
                let _ = self.execute_stmt(block, ctx);
                if let Some(h) = handler { let _ = self.execute_stmt(h, ctx); }
                if let Some(f) = finalizer { let _ = self.execute_stmt(f, ctx); }
                None
            }
            Stmt::Function { decl } => {
                ctx.scope.insert(decl.name.clone(), Value::Function(decl.name.clone()));
                None
            }
            _ => None,
        }
    }

    fn evaluate_expr(&self, expr: &Expr, ctx: &mut EvalContext) -> Result<Value, String> {
        match expr {
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Null => Ok(Value::Null),
            Expr::Boolean(b) => Ok(Value::Bool(*b)),
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::BigInt(n) => Ok(Value::Number(*n as f64)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::RegExp { .. } => Ok(Value::String("[RegExp]".to_string())),
            Expr::Template { parts, exprs } => {
                let mut result = String::new();
                for (i, part) in parts.iter().enumerate() {
                    if let TemplatePart::String(s) = part { result.push_str(s); }
                    if i < exprs.len() { result.push_str(&self.evaluate_expr(&exprs[i], ctx)?.to_string()); }
                }
                Ok(Value::String(result))
            }
            Expr::Ident { name } => ctx.scope.get(name).cloned().ok_or_else(|| format!("Variable not found: {}", name)),
            Expr::Bin { op, left, right } => {
                let left_val = self.evaluate_expr(left, ctx)?;
                let right_val = self.evaluate_expr(right, ctx)?;
                match op {
                    BinaryOp::Add => {
                        if let (Value::String(s1), v) = (&left_val, &right_val) { Ok(Value::String(format!("{}{}", s1, v.to_string()))) }
                        else if let (v, Value::String(s2)) = (&left_val, &right_val) { Ok(Value::String(format!("{}{}", v.to_string(), s2))) }
                        else { Ok(Value::Number(left_val.as_number() + right_val.as_number())) }
                    }
                    BinaryOp::Sub => Ok(Value::Number(left_val.as_number() - right_val.as_number())),
                    BinaryOp::Mul => Ok(Value::Number(left_val.as_number() * right_val.as_number())),
                    BinaryOp::Div => { let d = right_val.as_number(); if d == 0.0 { Ok(Value::Number(f64::INFINITY)) } else { Ok(Value::Number(left_val.as_number() / d)) } }
                    BinaryOp::Mod => { let d = right_val.as_number(); if d == 0.0 { Ok(Value::Number(f64::NAN)) } else { Ok(Value::Number(left_val.as_number() % d)) } }
                    BinaryOp::Eq | BinaryOp::EqStrict => Ok(Value::Bool(left_val == right_val)),
                    BinaryOp::Ne | BinaryOp::NeStrict => Ok(Value::Bool(left_val != right_val)),
                    BinaryOp::Lt => Ok(Value::Bool(left_val.as_number() < right_val.as_number())),
                    BinaryOp::Le => Ok(Value::Bool(left_val.as_number() <= right_val.as_number())),
                    BinaryOp::Gt => Ok(Value::Bool(left_val.as_number() > right_val.as_number())),
                    BinaryOp::Ge => Ok(Value::Bool(left_val.as_number() >= right_val.as_number())),
                    _ => Ok(Value::Undefined),
                }
            }
            Expr::Unary { op, arg, .. } => {
                let arg_val = self.evaluate_expr(arg, ctx)?;
                match op {
                    UnaryOp::Minus => Ok(Value::Number(-arg_val.as_number())),
                    UnaryOp::Plus => Ok(Value::Number(arg_val.as_number())),
                    UnaryOp::Not => Ok(Value::Bool(!arg_val.as_bool())),
                    UnaryOp::TypeOf => Ok(Value::String(match arg_val { Value::Undefined => "undefined", Value::Null => "object", Value::Bool(_) => "boolean", Value::Number(_) => "number", Value::String(_) => "string", Value::Function(_) => "function", _ => "object" }.to_string())),
                    UnaryOp::Void => Ok(Value::Undefined),
                    _ => Ok(Value::Undefined),
                }
            }
            Expr::Update { op, arg, .. } => {
                let name = match &**arg { Expr::Ident { name } => name, _ => return Ok(Value::Undefined) };
                let current = ctx.scope.get(name).cloned().unwrap_or(Value::Number(0.0));
                let new_value = match op { UpdateOp::Increment => Value::Number(current.as_number() + 1.0), UpdateOp::Decrement => Value::Number(current.as_number() - 1.0) };
                ctx.scope.insert(name.clone(), new_value.clone());
                Ok(new_value)
            }
            Expr::Logical { op, left, right } => {
                let left_val = self.evaluate_expr(left, ctx)?;
                match op {
                    LogicalOp::And => if left_val.as_bool() { self.evaluate_expr(right, ctx) } else { Ok(left_val) },
                    LogicalOp::Or => if left_val.as_bool() { Ok(left_val) } else { self.evaluate_expr(right, ctx) },
                    LogicalOp::NullishCoalesce => if matches!(left_val, Value::Null | Value::Undefined) { self.evaluate_expr(right, ctx) } else { Ok(left_val) },
                }
            }
            Expr::Cond { test, consequent, alternate } => {
                if self.evaluate_expr(test, ctx)?.as_bool() { self.evaluate_expr(consequent, ctx) } else { self.evaluate_expr(alternate, ctx) }
            }
            Expr::Call { callee, args, .. } => {
                let callee_val = self.evaluate_expr(callee, ctx)?;
                let arg_values: Result<Vec<Value>, String> = args.iter().map(|a| self.evaluate_expr(a, ctx)).collect();
                let arg_values = arg_values?;
                self.call_function(&callee_val, &arg_values, ctx)
            }
            Expr::Member { object, property, computed, .. } => {
                let obj_val = self.evaluate_expr(object, ctx)?;
                if *computed {
                    let prop_val = self.evaluate_expr(property, ctx)?;
                    Ok(obj_val.index(prop_val.as_number()).unwrap_or(Value::Undefined))
                } else {
                    let key = match &**property { Expr::Ident { name } => name.clone(), _ => self.evaluate_expr(property, ctx)?.to_string() };
                    Ok(obj_val.get_member(&key).unwrap_or(Value::Undefined))
                }
            }
            Expr::Object { props } => {
                let mut obj = HashMap::new();
                for prop in props {
                    match prop {
                        ObjectProp::Init { key, value } => {
                            let k = match key { PropKey::Ident(s) => s.clone(), PropKey::String(s) => s.clone(), PropKey::Number(n) => n.to_string(), PropKey::Computed(e) => self.evaluate_expr(e, ctx)?.to_string() };
                            let v = self.evaluate_expr(value, ctx)?;
                            obj.insert(k, v);
                        }
                        ObjectProp::Shorthand { name } => { if let Some(value) = ctx.scope.get(name).cloned() { obj.insert(name.clone(), value); } }
                        ObjectProp::Spread { value } => { if let Value::Object(spread_obj) = self.evaluate_expr(value, ctx)? { obj.extend(spread_obj); } }
                        _ => {}
                    }
                }
                Ok(Value::Object(obj))
            }
            Expr::Array { elems } => {
                let mut values = Vec::new();
                for elem in elems { if let Some(e) = elem { values.push(self.evaluate_expr(e, ctx)?); } }
                Ok(Value::Array(values))
            }
            Expr::Arrow { .. } => Ok(Value::Undefined),
            Expr::Function { decl } => { ctx.scope.insert(decl.name.clone(), Value::Function(decl.name.clone())); Ok(Value::Function(decl.name.clone())) }
            Expr::Await { arg } => self.evaluate_expr(arg, ctx),
            Expr::Assign { left, right, .. } => {
                let value = self.evaluate_expr(right, ctx)?;
                match &**left { Expr::Ident { name } => { ctx.scope.insert(name.clone(), value.clone()); } _ => {} }
                Ok(value)
            }
            Expr::Seq { exprs } => { let mut last = Value::Undefined; for e in exprs { last = self.evaluate_expr(e, ctx)?; } Ok(last) }
            Expr::Spread { arg } => self.evaluate_expr(arg, ctx),
            Expr::JSX(jsx) => self.evaluate_jsx(jsx, ctx),
            Expr::Class { .. } => Err("Class not supported".to_string()),
            Expr::TSAs { expr, .. } => self.evaluate_expr(expr, ctx),
            Expr::MetaProp { kind } => Ok(match kind { MetaPropKind::NewTarget => Value::Undefined, MetaPropKind::ImportMeta => Value::Object(HashMap::new()) }),
            Expr::TaggedTemplate { tag, template } => { let tag_val = self.evaluate_expr(tag, ctx)?; let template_val = self.evaluate_expr(template, ctx)?; self.call_function(&tag_val, &[template_val], ctx) }
            Expr::Yield { arg, .. } => Ok(arg.as_ref().map(|a| self.evaluate_expr(a, ctx)).transpose()?.unwrap_or(Value::Undefined)),
            _ => Ok(Value::Undefined),
        }
    }

    fn call_function(&self, callee: &Value, args: &[Value], ctx: &mut EvalContext) -> Result<Value, String> {
        let name = match callee { Value::Function(name) => name, _ => return Ok(Value::Undefined) };
        match name.as_str() {
            "console.log" | "console.error" => { let _ = std::io::stderr().write_all(format!("{}\n", args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ")).as_bytes()); Ok(Value::Undefined) }
            "useState" => {
                let initial = args.first().cloned().unwrap_or(Value::Undefined);
                let state_id = ctx.hooks.state.len();
                ctx.hooks.state.push(initial.clone());
                let mut setter_map = HashMap::new();
                setter_map.insert("__type".to_string(), Value::String("setter".to_string()));
                setter_map.insert("state_id".to_string(), Value::Number(state_id as f64));
                Ok(Value::Array(vec![initial, Value::Object(setter_map)]))
            }
            "useEffect" | "useLayoutEffect" => Ok(Value::Undefined),
            "useRef" => { let initial = args.first().cloned().unwrap_or(Value::Null); let mut ref_obj = HashMap::new(); ref_obj.insert("current".to_string(), initial); Ok(Value::Object(ref_obj)) }
            "useMemo" => { if let Some(f) = args.first() { self.call_function(f, &[], ctx) } else { Ok(Value::Undefined) } },
            "useCallback" => Ok(args.first().cloned().unwrap_or(Value::Undefined)),
            "JSON.stringify" => Ok(Value::String(args.first().map(|a| serde_json::to_string(&self.value_to_json(a.clone())).unwrap_or_else(|_| "undefined".to_string())).unwrap_or_else(|| "undefined".to_string()))),
            "JSON.parse" => { if let Some(Value::String(s)) = args.first() { let parsed: serde_json::Value = serde_json::from_str(s).unwrap_or(serde_json::Value::Null); Ok(self.json_to_value(&parsed)) } else { Ok(Value::Null) } }
            "Object.keys" => { if let Some(Value::Object(obj)) = args.first() { Ok(Value::Array(obj.keys().map(|k| Value::String(k.clone())).collect())) } else { Ok(Value::Array(Vec::new())) } }
            "Object.values" => { if let Some(Value::Object(obj)) = args.first() { Ok(Value::Array(obj.values().cloned().collect())) } else { Ok(Value::Array(Vec::new())) } }
            "Array.isArray" => Ok(Value::Bool(matches!(args.first(), Some(Value::Array(_))))),
            "Array.prototype.map" | "Array.prototype.filter" | "Array.prototype.reduce" | "Array.prototype.find" => { if let Some(Value::Array(arr)) = args.first() { Ok(Value::Array(arr.clone())) } else { Ok(Value::Array(Vec::new())) } }
            "String.prototype.split" | "String.prototype.trim" | "String.prototype.includes" | "String.prototype.startsWith" | "String.prototype.endsWith" | "String.prototype.toLowerCase" | "String.prototype.toUpperCase" => { if let Some(Value::String(s)) = args.first() { Ok(Value::String(s.clone())) } else { Ok(Value::String(String::new())) } }
            "Math.random" => { use std::time::{SystemTime, UNIX_EPOCH}; let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0); Ok(Value::Number((nanos as f64) / (u32::MAX as f64))) }
            "Math.floor" | "Math.ceil" | "Math.round" | "Math.abs" => { let n = args.first().map(|v| v.as_number()).unwrap_or(0.0); let result = match name.as_str() { "Math.floor" => n.floor(), "Math.ceil" => n.ceil(), "Math.round" => n.round(), "Math.abs" => n.abs(), _ => n }; Ok(Value::Number(result)) }
            "Math.min" | "Math.max" => { let result = if name == "Math.min" { args.iter().map(|v| v.as_number()).fold(f64::INFINITY, f64::min) } else { args.iter().map(|v| v.as_number()).fold(f64::NEG_INFINITY, f64::max) }; Ok(Value::Number(result)) }
            "parseInt" | "parseFloat" => { let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let n = if name == "parseInt" { s.trim().parse::<f64>().map(|v| v as i32 as f64).unwrap_or(f64::NAN) } else { s.trim().parse::<f64>().unwrap_or(f64::NAN) }; Ok(Value::Number(n)) }
            "isNaN" => Ok(Value::Bool(args.first().map(|v| v.as_number()).unwrap_or(0.0).is_nan())),
            "isFinite" => Ok(Value::Bool(args.first().map(|v| v.as_number()).unwrap_or(0.0).is_finite())),
            "encodeURIComponent" => Ok(Value::String(urlencoding::encode(&args.first().map(|v| v.to_string()).unwrap_or_default()).to_string())),
            "decodeURIComponent" => { let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let decoded = urlencoding::decode(&s).map(|s| s.to_string()).unwrap_or_default(); Ok(Value::String(decoded)) }
            _ => { if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) { Ok(Value::VNode(VNode::Component { name: name.clone(), props: args.first().cloned().unwrap_or(Value::Object(HashMap::new())) })) } else { Ok(Value::Undefined) } }
        }
    }

    fn evaluate_jsx(&self, jsx: &JSXExpr, ctx: &mut EvalContext) -> Result<Value, String> {
        let tag = match &jsx.opening.name {
            JSXName::Ident(s) => s.clone(),
            JSXName::Member { object, property } => format!("{}_{}", object, property),
            JSXName::Namespaced { ns, name } => format!("{}_{}", ns, name),
            JSXName::Dynamic(expr) => self.evaluate_expr(expr, ctx)?.to_string(),
        };

        let mut attrs = HashMap::new();
        for attr in &jsx.opening.attrs {
            match attr {
                JSXAttr::Attr { name, value } => {
                    let attr_value = value.as_ref().map(|v| match v { JSXAttrValue::String(s) => Value::String(s.clone()), JSXAttrValue::Expr(e) => self.evaluate_expr(e, ctx)? }).unwrap_or(Value::Bool(true));
                    attrs.insert(name.clone(), attr_value);
                }
                JSXAttr::Expr { name, expr } => { let value = self.evaluate_expr(expr, ctx)?; if let Some(n) = name { attrs.insert(n.clone(), value); } }
                JSXAttr::Spread { expr } => { if let Value::Object(obj) = self.evaluate_expr(expr, ctx)? { attrs.extend(obj); } }
                JSXAttr::Event { .. } => {}
                JSXAttr::Bool { name } => { attrs.insert(name.clone(), Value::Bool(true)); }
            }
        }

        let mut children = Vec::new();
        for child in &jsx.children {
            match child {
                JSXChild::Text(s) => children.push(VNode::Text(s.clone())),
                JSXChild::Expr(e) => {
                    let value = self.evaluate_expr(e, ctx)?;
                    match value {
                        Value::String(s) => children.push(VNode::Text(s)),
                        Value::Array(arr) => for v in arr { if let Value::VNode(vnode) = v { children.push(vnode); } else { children.push(VNode::Text(v.to_string())); } }
                        Value::VNode(vnode) => children.push(vnode),
                        Value::Undefined | Value::Null => {}
                        _ => children.push(VNode::Text(value.to_string())),
                    }
                }
                JSXChild::JSX(inner_jsx) => { if let Value::VNode(vnode) = self.evaluate_jsx(inner_jsx, ctx)? { children.push(vnode); } }
                JSXChild::Fragment { children: frag_children } => for fc in frag_children { if let JSXChild::JSX(inner_jsx) = fc { if let Value::VNode(vnode) = self.evaluate_jsx(inner_jsx, ctx)? { children.push(vnode); } } }
                JSXChild::Spread { expr } => { if let Value::Array(arr) = self.evaluate_expr(expr, ctx)? { for v in arr { if let Value::VNode(vnode) = v { children.push(vnode); } } } }
            }
        }

        let is_component = tag.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        if is_component {
            Ok(Value::VNode(VNode::Component { name: tag, props: Value::Object(attrs) }))
        } else {
            Ok(Value::VNode(VNode::Element { tag, attrs, children, key: None }))
        }
    }

    fn json_to_value(&self, json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(arr) => Value::Array(arr.iter().map(|v| self.json_to_value(v)).collect()),
            serde_json::Value::Object(obj) => Value::Object(obj.iter().map(|(k, v)| (k.clone(), self.json_to_value(v))).collect()),
        }
    }

    fn value_to_json(&self, value: Value) -> serde_json::Value {
        match value {
            Value::Undefined => serde_json::Value::Null,
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(b),
            Value::Number(n) => serde_json::json!(n),
            Value::String(s) => serde_json::Value::String(s),
            Value::Array(arr) => serde_json::Value::Array(arr.into_iter().map(|v| self.value_to_json(v)).collect()),
            Value::Object(obj) => serde_json::Value::Object(obj.into_iter().map(|(k, v)| (k, self.value_to_json(v))).collect()),
            Value::Function(name) => serde_json::json!(format!("[Function: {}]", name)),
            Value::VNode(vnode) => serde_json::json!(vnode.to_html_string()),
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self { Self::new() }
}
