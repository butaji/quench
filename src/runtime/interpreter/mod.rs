//! HIR Interpreter for Development Mode

// allow:too_many_lines,complexity

pub mod eval;
#[cfg(test)]
mod eval_tests;
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
    vars: Arc<RwLock<HashMap<String, Expr>>>,
    classes: Arc<RwLock<HashMap<String, ClassDecl>>>,
    instances: Arc<RwLock<HashMap<String, ClassInstance>>>,
    funcs: Arc<RwLock<HashMap<String, FunctionDecl>>>,
}

#[derive(Clone, Debug)]
struct ClassInstance {
    class_name: String,
    id: usize,
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
            vars: Arc::new(RwLock::new(HashMap::new())),
            classes: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
            funcs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn eval_module(&self, module: &Module) -> String {
        let mut result = String::new();
        for item in &module.items {
            result = self.eval_module_item(item);
        }
        result
    }
    
    fn eval_module_item(&self, item: &ModuleItem) -> String {
        match item {
            ModuleItem::Decl(Decl::Variable(var)) => {
                let name = var.name.clone();
                if let Some(init) = &var.init {
                    if matches!(&*init, Expr::ArrowFunction { .. }) {
                        self.vars.write().insert(name.clone(), (*init).clone());
                        format!("{:?}", init)
                    } else if matches!(&*init, Expr::New { .. }) {
                        let instance_name = self.eval_expr(init);
                        self.vars.write().insert(name.clone(), Expr::Ident { name: instance_name.clone() });
                        instance_name
                    } else {
                        self.vars.write().insert(name.clone(), (*init).clone());
                        self.eval_expr(init)
                    }
                } else {
                    String::new()
                }
            }
            ModuleItem::Decl(Decl::Class(class)) => {
                self.classes.write().insert(class.name.clone(), class.clone());
                format!("class {}", class.name)
            }
            ModuleItem::Decl(Decl::Function(func)) => {
                self.funcs.write().insert(func.name.clone(), func.clone());
                format!("function {}(...) {{...}}", func.name)
            }
            ModuleItem::Decl(Decl::Type(_)) => String::new(),
            ModuleItem::Export(_) => String::new(),
            ModuleItem::Import(_) => String::new(),
            ModuleItem::Stmt(stmt) => self.eval_stmt(stmt),
        }
    }
    
    pub fn eval_module_stmts(&self, module: &Module) -> String {
        let mut result = String::new();
        for item in &module.items {
            result = self.eval_module_item(item);
        }
        if result.is_empty() { "undefined".to_string() } else { result }
    }
    
    pub fn eval_stmt(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Expr { expr } => self.eval_expr(expr),
            Stmt::If { test, consequent, alternate } => {
                let t = self.eval_expr(test);
                if t != "false" && t != "null" && t != "0" && !t.is_empty() && t != "undefined" {
                    self.eval_stmt(consequent)
                } else if let Some(alt) = alternate {
                    self.eval_stmt(alt)
                } else {
                    String::new()
                }
            }
            Stmt::Return { arg } => {
                if let Some(expr) = arg {
                    self.eval_expr(expr)
                } else {
                    String::new()
                }
            }
            Stmt::While { test, body } => {
                let mut result = String::new();
                loop {
                    let t = self.eval_expr(test);
                    if t == "false" || t == "null" || t == "0" || t.is_empty() || t == "undefined" {
                        break;
                    }
                    result = self.eval_stmt(body);
                }
                result
            }
            Stmt::For { init, test, update, body } => {
                let mut result = String::new();
                // Initialize
                if let Some(init) = init {
                    self.eval_for_init(init);
                }
                // Loop
                loop {
                    // Check test
                    if let Some(test) = test {
                        let t = self.eval_expr(test);
                        if t == "false" || t == "null" || t == "0" || t.is_empty() || t == "undefined" {
                            break;
                        }
                    }
                    // Execute body
                    result = self.eval_stmt(body);
                    // Update
                    if let Some(update) = update {
                        self.eval_expr(update);
                    }
                }
                result
            }
            Stmt::Block(stmts) => {
                let mut result = String::new();
                for s in stmts {
                    result = self.eval_stmt(s);
                }
                result
            }
            Stmt::Break { .. } => String::new(),
            Stmt::Continue { .. } => String::new(),
            Stmt::Empty => String::new(),
            _ => format!("{:?}", stmt),
        }
    }
    
    fn eval_for_init(&self, init: &ForInit) {
        match init {
            ForInit::Variable(kind, decls) => {
                for (name, init) in decls {
                    let val = init.as_ref().map(|e| self.eval_expr(e)).unwrap_or_default();
                    self.vars.write().insert(name.clone(), Expr::String(val));
                }
            }
            ForInit::Expr(expr) => {
                self.eval_expr(expr);
            }
        }
    }

    fn eval_expr(&self, expr: &Expr) -> String {
        let s = self.eval_simple(expr);
        if !s.is_empty() {
            return s;
        }
        self.eval_complex(expr)
    }

    fn eval_simple(&self, expr: &Expr) -> String {
        match expr {
            Expr::Number(n) => format!("{}", n),
            Expr::String(s) => s.clone(),
            Expr::Boolean(b) => b.to_string(),
            Expr::Null => "null".into(),
            Expr::Undefined => "undefined".into(),
            Expr::Ident { name } => {
                // Look up variable - if found, evaluate it
                if let Some(var_expr) = self.vars.read().get(name) {
                    self.eval_expr(var_expr)
                } else {
                    name.clone()
                }
            }
            _ => String::new(),
        }
    }

    fn eval_complex(&self, expr: &Expr) -> String {
        match expr {
            Expr::Bin { op, left, right } => self.eval_bin_op(op, left, right),
            Expr::Logical { op, left, right } => self.eval_logical(op, left, right),
            Expr::Cond { test, consequent, alternate } => self.eval_cond(test, consequent, alternate),
            Expr::Call { callee, arguments } => self.eval_call(callee, arguments),
            Expr::New { callee, arguments } => self.eval_new(callee, arguments),
            Expr::Member { obj, property, computed } => self.eval_member(obj, property, *computed),
            Expr::StaticMember { obj, property } => self.eval_static_member(obj, property),
            Expr::Array { elems } => self.eval_array(elems),
            Expr::Object { members } => self.eval_object(members),
            Expr::Template { parts, exprs } => self.eval_template(parts, exprs),
            Expr::ArrowFunction { body, .. } => self.eval_expr(body),
            Expr::Unary { op, arg, .. } => self.eval_unary(op, arg),
            Expr::Update { op, arg, prefix, .. } => self.eval_update(op, arg, *prefix),
            Expr::Assign { op, left, right } => self.eval_assign(op, left, right),
            _ => format!("{:?}", expr),
        }
    }
    
    fn eval_template(&self, parts: &[TemplatePart], exprs: &[Expr]) -> String {
        let mut result = String::new();
        let mut expr_iter = exprs.iter();
        for part in parts {
            match part {
                TemplatePart::String(s) => result.push_str(s),
                TemplatePart::Type(_) => {}
            }
            if let Some(expr) = expr_iter.next() {
                result.push_str(&self.eval_expr(expr));
            }
        }
        result
    }
    
    fn eval_object(&self, members: &[ObjectMemberExpr]) -> String {
        let items: Vec<String> = members.iter().map(|m| {
            match &m.prop {
                ObjectProp::Init { key, value, .. } => {
                    let k = match key {
                        PropKey::Str(s) => s.clone(),
                        PropKey::Num(n) => n.to_string(),
                        PropKey::Computed { expr } => format!("[{}]", self.eval_expr(expr)),
                    };
                    let v = self.eval_expr(value);
                    format!("{}: {}", k, v)
                }
                _ => String::new(),
            }
        }).collect();
        format!("{{{}}}", items.join(", "))
    }
    fn eval_unary(&self, op: &UnaryOp, arg: &Expr) -> String {
        let v = self.eval_expr(arg);
        match op {
            UnaryOp::Minus => { if let Ok(n) = v.parse::<f64>() { format!("{}", -n) } else { "NaN".into() } }
            UnaryOp::Plus => { if let Ok(n) = v.parse::<f64>() { format!("{}", n) } else { "NaN".into() } }
            UnaryOp::Not => { if v == "false" || v == "null" || v == "0" || v.is_empty() || v == "undefined" { "true".into() } else { "false".into() } }
            UnaryOp::BitNot => { if let Ok(n) = v.parse::<i64>() { format!("{}", !n) } else { "NaN".into() } }
            UnaryOp::Typeof => Self::typeof_val(&v).to_string(),
            UnaryOp::Void => "undefined".into(),
            UnaryOp::Delete => "true".into(),
        }
    }
    fn typeof_val(v: &str) -> &'static str {
        if v == "null" { return "object"; }
        if v == "undefined" { return "undefined"; }
        if v == "true" || v == "false" { return "boolean"; }
        if Self::is_string_val(v) { return "string"; }
        if v.parse::<f64>().is_ok() { return "number"; }
        if v.starts_with('[') { return "object"; }
        if v.starts_with('{') { return "object"; }
        "function"
    }
    fn eval_update(&self, op: &UpdateOp, arg: &Expr, _prefix: bool) -> String {
        self.eval_expr(arg)
    }
    fn eval_assign(&self, op: &AssignOp, left: &Expr, right: &Expr) -> String {
        let r = self.eval_expr(right);
        if let Expr::Ident { name } = left {
            self.vars.write().insert(name.clone(), Expr::String(r.clone()));
        }
        r
    }

    fn eval_array(&self, elems: &[Option<Expr>]) -> String {
        let items: Vec<String> = elems.iter().map(|e| {
            match e {
                Some(expr) => self.eval_expr(expr),
                None => "undefined".to_string(),
            }
        }).collect();
        format!("[{}]", items.join(", "))
    }

    fn eval_new(&self, callee: &Box<Expr>, arguments: &[Expr]) -> String {
        // Evaluate the callee to get the class name
        let class_name = match &**callee {
            Expr::Ident { name } => name.clone(),
            _ => format!("{:?}", callee),
        };
        if class_name == "Array" {
            return format!("[{}]", arguments.iter().map(|a| self.eval_expr(a)).collect::<Vec<_>>().join(", "));
        }
        // Create instance and store it
        let instance_id = self.instances.read().len();
        let instance_name = format!("{}@{}", class_name, instance_id);
        let instance = ClassInstance { class_name: class_name.clone(), id: instance_id };
        self.instances.write().insert(instance_name.clone(), instance);
        instance_name
    }

    fn eval_member(&self, obj: &Box<Expr>, property: &Box<Expr>, computed: bool) -> String {
        let obj_expr = self.eval_expr(obj);
        let prop_str = self.eval_expr(property);
        // Handle string indexing
        if Self::is_string_val(&obj_expr) {
            if computed {
                let idx = prop_str.parse::<usize>().unwrap_or(0);
                let chars: Vec<char> = obj_expr.chars().collect();
                if let Some(c) = chars.get(idx) {
                    return c.to_string();
                }
            }
            return format!("{}[{}]", obj_expr, prop_str);
        }
        // Handle array length
        if obj_expr.starts_with('[') {
            if !computed {
                if prop_str == "length" {
                    let count = obj_expr.matches(',').count() + 1;
                    if obj_expr == "[]" { return "0".to_string(); }
                    return format!("{}", count);
                }
            } else {
                let idx = prop_str.parse::<usize>().unwrap_or(0);
                let elements: Vec<&str> = obj_expr[1..obj_expr.len()-1].split(", ").collect();
                if let Some(elem) = elements.get(idx) {
                    return elem.trim().to_string();
                }
            }
        }
        // Handle object literal property access - look for property in object string
        if obj_expr.starts_with('{') && obj_expr.ends_with('}') {
            return self.eval_object_prop(&obj_expr, &prop_str);
        }
        format!("{}[{}]", obj_expr, prop_str)
    }
    
    fn eval_object_prop(&self, obj: &str, prop: &str) -> String {
        let inner = obj.trim_start_matches('{').trim_end_matches('}').trim();
        for part in inner.split(',') {
            let kv: Vec<&str> = part.splitn(2, ':').collect();
            if kv.len() == 2 {
                let key = kv[0].trim().trim_matches('"').trim_matches('\'');
                if key == prop {
                    return kv[1].trim().to_string();
                }
            }
        }
        "undefined".to_string()
    }

    fn eval_static_member(&self, obj: &Box<Expr>, property: &str) -> String {
        let obj_str = self.eval_expr(obj);
        // Handle array length
        if obj_str.starts_with('[') && property == "length" {
            let count = obj_str.matches(',').count() + 1;
            if obj_str == "[]" { return "0".to_string(); }
            return format!("{}", count);
        }
        // Handle string length
        if Self::is_string_val(&obj_str) && property == "length" {
            return format!("{}", obj_str.len());
        }
        // Handle object literal property access
        if obj_str.starts_with('{') && obj_str.ends_with('}') {
            return self.eval_object_prop(&obj_str, property);
        }
        format!("{}.{}", obj_str, property)
    }

    fn eval_logical(&self, op: &LogicalOp, left: &Expr, right: &Expr) -> String {
        let l = self.eval_expr(left);
        match op {
            LogicalOp::And => self.and_op(&l, right),
            LogicalOp::Or => self.or_op(&l, right),
            LogicalOp::NullishCoalescing => self.nullish_op(&l, right),
        }
    }

    fn and_op(&self, left: &str, right: &Expr) -> String {
        if left == "false" || left == "null" || left == "0" || left.is_empty() {
            left.to_string()
        } else {
            self.eval_expr(right)
        }
    }

    fn or_op(&self, left: &str, right: &Expr) -> String {
        if left == "false" || left == "null" || left == "0" || left.is_empty() {
            self.eval_expr(right)
        } else {
            left.to_string()
        }
    }

    fn nullish_op(&self, left: &str, right: &Expr) -> String {
        if left == "null" || left == "undefined" {
            self.eval_expr(right)
        } else {
            left.to_string()
        }
    }

    fn eval_cond(&self, test: &Expr, consequent: &Expr, alternate: &Expr) -> String {
        let t = self.eval_expr(test);
        if t != "false" && t != "null" && t != "0" && t != "" && t != "undefined" {
            self.eval_expr(consequent)
        } else {
            self.eval_expr(alternate)
        }
    }

    fn eval_call(&self, callee: &Expr, args: &[Expr]) -> String {
        // Evaluate all arguments first
        let arg_vals: Vec<String> = args.iter().map(|a| self.eval_expr(a)).collect();
        // Special handling for console.log
        if let Expr::StaticMember { obj, property } = callee {
            let obj_str = self.eval_expr(obj);
            if obj_str.contains("console") && property == "log" {
                println!("{}", arg_vals.join(" "));
                return arg_vals.join(" ");
            }
            // Handle Math static methods
            if obj_str == "Math" {
                return self.eval_math_method(property, &arg_vals);
            }
            // Handle JSON static methods
            if obj_str == "JSON" {
                return self.eval_json_method(property, &arg_vals);
            }
            // Handle Object static methods
            if obj_str == "Object" {
                return self.eval_object_method(property, &arg_vals);
            }
            // Handle instance method calls
            if obj_str.contains('@') {
                if let Some(instance) = self.instances.read().get(&obj_str) {
                    if let Some(class) = self.classes.read().get(&instance.class_name) {
                        if let Some(method) = class.methods.iter().find(|m| m.name == *property) {
                            return self.eval_method(&method, args);
                        }
                    }
                }
            }
            // Handle string instance methods
            if Self::is_string_val(&obj_str) {
                return self.eval_string_method(&obj_str, property, &arg_vals);
            }
            // Handle array instance methods
            if obj_str.starts_with('[') {
                return self.eval_array_method_call(&obj_str, property, &arg_vals);
            }
            return format!("{}.{}({:?})", obj_str, property, arg_vals);
        }
        let callee_expr = match callee {
            Expr::Ident { name } => {
                // Check for built-in constructors
                match name.as_str() {
                    "String" => return arg_vals.first().map(|s| s.clone()).unwrap_or_default(),
                    "Number" => return arg_vals.first().and_then(|s| s.parse::<f64>().ok()).map(|n| n.to_string()).unwrap_or_default(),
                    "Boolean" => return arg_vals.first().map(|s| if s == "false" || s == "0" || s.is_empty() { "false".to_string() } else { "true".to_string() }).unwrap_or_default(),
                    "Array" => return format!("[{}]", arg_vals.join(", ")),
                    "Object" => return "{}".to_string(),
                    "console" => return arg_vals.join(" "),
                    _ => {}
                }
                self.vars.read().get(name).cloned().unwrap_or_else(|| callee.clone())
            }
            Expr::Member { obj, property, computed } => {
                let obj_str = self.eval_expr(obj);
                if *computed {
                    let prop_str = self.eval_expr(property);
                    // Array index access
                    if obj_str.starts_with('[') {
                        if let Ok(idx) = prop_str.parse::<usize>() {
                            let elements = self.extract_array_elements(&obj_str);
                            return elements.get(idx).cloned().unwrap_or_else(|| "undefined".to_string());
                        }
                    }
                    return format!("{}[{}]", obj_str, prop_str);
                } else {
                    let prop_str = self.eval_expr(property);
                    // Property access on string
                    if Self::is_string_val(&obj_str) {
                        if prop_str == "length" {
                            return format!("{}", obj_str.len());
                        }
                        return "undefined".to_string();
                    }
                    // Property access on object
                    if obj_str.starts_with('{') {
                        return self.eval_object_prop(&obj_str, &prop_str);
                    }
                    // Property access on array
                    if obj_str.starts_with('[') {
                        if prop_str == "length" {
                            let elements = self.extract_array_elements(&obj_str);
                            return format!("{}", elements.len());
                        }
                    }
                    return format!("{}.{}", obj_str, prop_str);
                }
            }
            _ => callee.clone(),
        };
        if let Expr::ArrowFunction { params, body, .. } = callee_expr {
            return self.eval_arrow_func(&params, &body, args);
        }
        format!("Call<{:?}>", callee_expr)
    }
    
    fn is_string_val(s: &str) -> bool {
        // A string in our interpreter is plain text (identifier-like) that isn't a JS keyword
        if s.is_empty() { return false; }
        let js_vals = ["true", "false", "null", "undefined", "NaN", "Infinity"];
        if js_vals.contains(&s) { return false; }
        // If it parses as a number, it's not a string
        if s.parse::<f64>().is_ok() { return false; }
        // Check if it looks like a string (alphanumeric, spaces, punctuation)
        let chars: Vec<char> = s.chars().collect();
        if chars[0].is_alphabetic() || chars[0] == '_' || chars[0] == ' ' {
            return true;
        }
        false
    }
    
    fn get_string_val(s: &str) -> String {
        s.to_string()
    }
    
    fn eval_string_method(&self, s: &str, method: &str, args: &[String]) -> String {
        let inner = s.trim_matches('\"').trim_matches('\'');
        match method {
            "length" => format!("{}", inner.len()),
            "toUpperCase" => inner.to_uppercase(),
            "toLowerCase" => inner.to_lowercase(),
            "trim" => inner.trim().to_string(),
            "charAt" => args.first().and_then(|a| a.parse::<usize>().ok()).and_then(|i| inner.chars().nth(i)).map(|c| c.to_string()).unwrap_or_default(),
            "indexOf" => args.first().map(|a| inner.find(a.as_str()).map(|i| i.to_string()).unwrap_or_else(|| "-1".to_string())).unwrap_or_default(),
            "includes" => args.first().map(|a| inner.contains(a.as_str()).to_string()).unwrap_or_default(),
            "startsWith" => args.first().map(|a| inner.starts_with(a.as_str()).to_string()).unwrap_or_default(),
            "endsWith" => args.first().map(|a| inner.ends_with(a.as_str()).to_string()).unwrap_or_default(),
            "substring" | "substr" => {
                let start = args.get(0).and_then(|a| a.parse::<usize>().ok()).unwrap_or(0);
                let end = args.get(1).and_then(|a| a.parse::<usize>().ok()).unwrap_or(inner.len());
                inner.chars().skip(start).take(end - start).collect()
            }
            "split" => {
                let sep = args.first().map(|s| s.as_str()).unwrap_or(",");
                if sep.is_empty() {
                    inner.chars().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", ")
                } else {
                    inner.split(sep).map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ")
                }
            }
            "toString" => inner.to_string(),
            _ => format!("{}.{}({:?})", s, method, args),
        }
    }
    
    fn eval_object_method(&self, method: &str, args: &[String]) -> String {
        match method {
            "keys" => {
                if let Some(obj) = args.first() {
                    let inner = obj.trim_start_matches('{').trim_end_matches('}').trim();
                    let keys: Vec<String> = inner.split(',').filter_map(|part| {
                        let kv: Vec<&str> = part.splitn(2, ':').collect();
                        if kv.len() == 2 {
                            Some(format!("\"{}\"", kv[0].trim().trim_matches('\"').trim_matches('\'')))
                        } else { None }
                    }).collect();
                    format!("[{}]", keys.join(", "))
                } else { "[]".to_string() }
            }
            "values" => {
                if let Some(obj) = args.first() {
                    let inner = obj.trim_start_matches('{').trim_end_matches('}').trim();
                    let vals: Vec<String> = inner.split(',').filter_map(|part| {
                        let kv: Vec<&str> = part.splitn(2, ':').collect();
                        if kv.len() == 2 { Some(kv[1].trim().to_string()) } else { None }
                    }).collect();
                    format!("[{}]", vals.join(", "))
                } else { "[]".to_string() }
            }
            "entries" => {
                if let Some(obj) = args.first() {
                    let inner = obj.trim_start_matches('{').trim_end_matches('}').trim();
                    let entries: Vec<String> = inner.split(',').filter_map(|part| {
                        let kv: Vec<&str> = part.splitn(2, ':').collect();
                        if kv.len() == 2 {
                            let key = kv[0].trim().trim_matches('\"').trim_matches('\'');
                            Some(format!("[\"{}\", {}]", key, kv[1].trim()))
                        } else { None }
                    }).collect();
                    format!("[{}]", entries.join(", "))
                } else { "[]".to_string() }
            }
            "create" => "{}".to_string(),
            "assign" => args.get(1).cloned().unwrap_or_else(|| "{}".to_string()),
            _ => format!("Object.{}({:?})", method, args),
        }
    }
    
    fn eval_json_method(&self, method: &str, args: &[String]) -> String {
        match method {
            "parse" => {
                if let Some(s) = args.first() {
                    let inner = s.trim_matches('\"');
                    let inner = inner.trim();
                    if inner.starts_with('{') || inner.starts_with('[') {
                        inner.to_string()
                    } else if let Ok(n) = inner.parse::<f64>() {
                        n.to_string()
                    } else if inner == "true" { "true".to_string() }
                    else if inner == "false" { "false".to_string() }
                    else if inner == "null" { "null".to_string() }
                    else { inner.to_string() }
                } else { "undefined".to_string() }
            }
            "stringify" => {
                if let Some(v) = args.first() {
                    if v.starts_with('\"') { v.to_string() } else { format!("\"{}\"", v.trim_matches('\"')) }
                } else { "undefined".to_string() }
            }
            _ => format!("JSON.{}({:?})", method, args),
        }
    }
    
    fn eval_math_method(&self, method: &str, args: &[String]) -> String {
        match method {
            "PI" => "3.141592653589793".to_string(),
            "E" => "2.718281828459045".to_string(),
            "abs" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.abs().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "floor" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.floor().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "ceil" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.ceil().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "round" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.round().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "max" => { let nums: Vec<f64> = args.iter().filter_map(|a| a.parse().ok()).collect(); nums.iter().cloned().fold(std::f64::NEG_INFINITY, f64::max).to_string() }
            "min" => { let nums: Vec<f64> = args.iter().filter_map(|a| a.parse().ok()).collect(); nums.iter().cloned().fold(std::f64::INFINITY, f64::min).to_string() }
            "pow" => { let base: f64 = args.get(0).and_then(|a| a.parse().ok()).unwrap_or(0.0); let exp: f64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0.0); base.powf(exp).to_string() }
            "sqrt" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.sqrt().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "random" => Self::rand_simple().to_string(),
            "sin" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.sin().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "cos" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.cos().to_string()).unwrap_or_else(|| "NaN".to_string()),
            "log" => args.first().and_then(|a| a.parse::<f64>().ok()).map(|n| n.ln().to_string()).unwrap_or_else(|| "NaN".to_string()),
            _ => format!("Math.{}({:?})", method, args),
        }
    }
    
    fn rand_simple() -> f64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        (nanos as f64) / (u32::MAX as f64 + 1.0)
    }
    
    fn extract_array_elements(&self, arr: &str) -> Vec<String> {
        let inner = arr.trim_start_matches('[').trim_end_matches(']');
        if inner.trim().is_empty() { return vec![]; }
        let mut result = vec![];
        let mut depth = 0;
        let mut current = String::new();
        for ch in inner.chars() {
            match ch {
                '[' => { depth += 1; current.push(ch); }
                ']' => { depth -= 1; current.push(ch); }
                ',' if depth == 0 => { result.push(current.trim().to_string()); current.clear(); }
                _ => current.push(ch),
            }
        }
        if !current.trim().is_empty() { result.push(current.trim().to_string()); }
        result
    }
    
    fn eval_array_method_call(&self, arr: &str, method: &str, args: &[String]) -> String {
        let elements = self.extract_array_elements(arr);
        match method {
            "length" => format!("{}", elements.len()),
            "map" | "filter" | "reduce" => arr.to_string(), // Full implementation needs closures
            "push" => format!("{}", elements.len()),
            "pop" => elements.last().cloned().unwrap_or_else(|| "undefined".to_string()),
            "shift" => elements.get(1).cloned().unwrap_or_else(|| "undefined".to_string()),
            "indexOf" => { if let Some(target) = args.first() { elements.iter().position(|e| e.trim_matches('\"') == target.trim_matches('\"')).map(|i| i.to_string()).unwrap_or_else(|| "-1".to_string()) } else { "-1".to_string() } }
            "includes" => { if let Some(target) = args.first() { elements.iter().any(|e| e.trim_matches('\"') == target.trim_matches('\"')).to_string() } else { "false".to_string() } }
            "join" => { let sep = args.first().map(|s| s.as_str()).unwrap_or(","); elements.iter().map(|e| e.trim_matches('\"').trim_matches('\'')).collect::<Vec<_>>().join(sep) }
            "reverse" => format!("[{}]", elements.iter().rev().map(|e| e.as_str()).collect::<Vec<_>>().join(", ")),
            "slice" => { let start = args.get(0).and_then(|a| a.parse::<usize>().ok()).unwrap_or(0); let end = args.get(1).and_then(|a| a.parse::<usize>().ok()).unwrap_or(elements.len()); format!("[{}]", elements[start..end.min(elements.len())].iter().map(|e| e.as_str()).collect::<Vec<_>>().join(", ")) }
            "toString" => format!("[{}]", elements.iter().map(|e| e.trim_matches('\"').trim_matches('\'')).collect::<Vec<_>>().join(", ")),
            "flat" => arr.to_string(),
            "find" => "undefined".to_string(),
            _ => format!("{}.{}({:?})", arr, method, args),
        }
    }

    fn eval_method(&self, method: &ClassMethod, args: &[Expr]) -> String {
        let mut ctx = EvalContext::default();
        for (i, p) in method.params.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                ctx.scope.insert(p.name.clone(), self.expr_to_value(arg));
            }
        }
        self.eval_expr_with_ctx(&method.body, &ctx)
    }

    fn eval_arrow_func(&self, params: &[Param], body: &Box<Expr>, args: &[Expr]) -> String {
        let mut ctx = EvalContext::default();
        for (i, p) in params.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                ctx.scope.insert(p.name.clone(), self.expr_to_value(arg));
            }
        }
        self.eval_expr_with_ctx(body, &ctx)
    }

    fn expr_to_value(&self, expr: &Expr) -> Value {
        match expr {
            Expr::Number(n) => Value::Number(*n),
            Expr::String(s) => Value::String(s.clone()),
            Expr::Boolean(b) => Value::Bool(*b),
            Expr::Null => Value::Null,
            Expr::Undefined => Value::Undefined,
            _ => Value::Null,
        }
    }

    fn eval_expr_with_ctx(&self, expr: &Expr, ctx: &EvalContext) -> String {
        match expr {
            Expr::Ident { name } => {
                if let Some(v) = ctx.scope.get(name) {
                    match v {
                        Value::Number(n) => format!("{}", n),
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        Value::Null => "null".into(),
                        Value::Undefined => "undefined".into(),
                        _ => format!("{:?}", v),
                    }
                } else {
                    name.clone()
                }
            }
            Expr::Bin { op, left, right } => {
                let l = self.eval_expr_with_ctx(left, ctx);
                let r = self.eval_expr_with_ctx(right, ctx);
                self.eval_bin_op_str(&l, &r, op)
            }
            _ => self.eval_expr(expr),
        }
    }

    fn subst_expr(&self, expr: &Expr, var: &str, val: &str) -> Expr {
        match expr {
            Expr::Ident { name } if name == var => Expr::Number(val.parse().unwrap_or(0.0)),
            Expr::Bin { op, left, right } => Expr::Bin {
                op: op.clone(),
                left: Box::new(self.subst_expr(left, var, val)),
                right: Box::new(self.subst_expr(right, var, val)),
            },
            _ => expr.clone(),
        }
    }

    fn eval_bin_op(&self, op: &BinaryOp, left: &Expr, right: &Expr) -> String {
        let l = self.eval_expr(left);
        let r = self.eval_expr(right);
        match op {
            BinaryOp::Add => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(nl), Ok(nr)) => format!("{}", nl + nr), _ => format!("{}{}", l, r) } }
            BinaryOp::Sub => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(nl), Ok(nr)) => format!("{}", nl - nr), _ => "NaN".into() } }
            BinaryOp::Mul => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(nl), Ok(nr)) => format!("{}", nl * nr), _ => "NaN".into() } }
            BinaryOp::Div => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(nl), Ok(nr)) => format!("{}", nl / nr), _ => "NaN".into() } }
            BinaryOp::Mod => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(nl), Ok(nr)) => format!("{}", (nl as i64) % (nr as i64)), _ => "NaN".into() } }
            BinaryOp::Eq => (Self::loose_eq(&l, &r)).to_string(),
            BinaryOp::StrictEq => (Self::strict_eq(&l, &r)).to_string(),
            BinaryOp::Neq => (!Self::loose_eq(&l, &r)).to_string(),
            BinaryOp::StrictNeq => (!Self::strict_eq(&l, &r)).to_string(),
            BinaryOp::Lt => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(ln), Ok(rn)) => (ln < rn).to_string(), _ => "false".into() } }
            BinaryOp::Lte => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(ln), Ok(rn)) => (ln <= rn).to_string(), _ => "false".into() } }
            BinaryOp::Gt => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(ln), Ok(rn)) => (ln > rn).to_string(), _ => "false".into() } }
            BinaryOp::Gte => { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(ln), Ok(rn)) => (ln >= rn).to_string(), _ => "false".into() } }
            _ => "NaN".into(),
        }
    }
    fn loose_eq(l: &str, r: &str) -> bool {
        if l == "NaN" || r == "NaN" { false }
        else if l == "null" && r == "undefined" { true }
        else if l == "undefined" && r == "null" { true }
        else { let ln = l.parse::<f64>(); let rn = r.parse::<f64>(); match (ln, rn) { (Ok(nl), Ok(rn)) => nl == rn, _ => l == r } }
    }
    fn strict_eq(l: &str, r: &str) -> bool {
        let (ln, rn) = (l.parse::<f64>(), r.parse::<f64>());
        match (ln, rn) {
            (Ok(_), Ok(_)) => l == r,
            (Err(_), Err(_)) => l == r,
            _ => false,
        }
    }

    fn eval_bin_op_str(&self, left: &str, right: &str, op: &BinaryOp) -> String {
        if matches!(op, BinaryOp::Add) {
            // Check if either is a string
            if left.starts_with('"')
                || right.starts_with('"')
                || left.starts_with('\'')
                || right.starts_with('\'')
            {
                let ls = left.trim_matches('"').trim_matches('\'');
                let rs = right.trim_matches('"').trim_matches('\'');
                return format!(
                    "{}\"{}\"",
                    if left.starts_with('"') || left.starts_with('\'') {
                        String::new()
                    } else {
                        format!("{}", left)
                    },
                    rs
                );
            }
        }
        let l: f64 = left.parse().unwrap_or(0.0);
        let r: f64 = right.parse().unwrap_or(0.0);
        match op {
            BinaryOp::Add => format!("{}", l + r),
            BinaryOp::Sub => format!("{}", l - r),
            BinaryOp::Mul => format!("{}", l * r),
            BinaryOp::Div => format!("{}", l / r),
            BinaryOp::Mod => format!("{}", (l as i64) % (r as i64)),
            _ => format!("{:?}", op),
        }
    }

    fn eval_num_bin_op(&self, op: &BinaryOp, left: &Expr, right: &Expr) -> String {
        let l = self.eval_expr(left).parse::<f64>().unwrap_or(0.0);
        let r = self.eval_expr(right).parse::<f64>().unwrap_or(1.0);
        match op {
            BinaryOp::Add => format!("{}", l + r),
            BinaryOp::Sub => format!("{}", l - r),
            BinaryOp::Mul => format!("{}", l * r),
            BinaryOp::Div => format!("{}", l / r),
            BinaryOp::Mod => format!("{}", (l as i64) % (r as i64)),
            _ => format!("{:?}", op),
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
            if let ModuleItem::Export(export) = item {
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
        }
        Ok(())
    }

    pub fn render_route(
        &self,
        _pattern: &str,
        params: HashMap<String, String>,
    ) -> Result<String, anyhow::Error> {
        let ctx = EvalContext { params, ..Default::default() };
        let components = self.components.read();
        if let Some(component) = components.get("Home") {
            let html = format!("<div data-component=\"{}\">{}</div>",
                component.name,
                render::render_component_body(&component.body, &ctx)
            );
            Ok(html)
        } else {
            Ok(String::new())
        }
    }

    pub fn load_file(&mut self, path: &str, source: &str) -> Result<(), anyhow::Error> {
        let parser = crate::transpile::TsParser::new();
        let module = parser.parse_source(source)?;
        self.modules.write().insert(path.to_string(), module);
        Ok(())
    }

    pub fn execute_route(&self, path: &str, params: HashMap<String, String>) -> Result<String, anyhow::Error> {
        let ctx = EvalContext { params, ..Default::default() };
        let modules = self.modules.read();
        if let Some(module) = modules.get(path) {
            Ok(render::execute_module_items(&module.items, &ctx))
        } else {
            Ok(String::new())
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}
