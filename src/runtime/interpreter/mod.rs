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
        }
    }

    pub fn eval_module(&self, module: &Module) -> String {
        let mut result = String::new();
        for item in &module.items {
            match item {
                ModuleItem::Decl(Decl::Variable(var)) => {
                    let name = var.name.clone();
                    if let Some(init) = &var.init {
                        if matches!(&*init, Expr::ArrowFunction { .. }) {
                            self.vars.write().insert(name.clone(), (*init).clone());
                            result = format!("{:?}", init);
                        } else if matches!(&*init, Expr::New { .. }) {
                            // Evaluate new expression and store result string
                            let instance_name = self.eval_expr(init);
                            self.vars.write().insert(name.clone(), Expr::Ident { name: instance_name.clone() });
                            result = instance_name;
                        } else {
                            self.vars.write().insert(name.clone(), (*init).clone());
                            result = self.eval_expr(init);
                        }
                    }
                }
                ModuleItem::Decl(Decl::Class(class)) => {
                    self.classes.write().insert(class.name.clone(), class.clone());
                }
                _ => {}
            }
        }
        result
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
            Expr::Cond {
                test,
                consequent,
                alternate,
            } => self.eval_cond(test, consequent, alternate),
            Expr::Call { callee, arguments } => self.eval_call(callee, arguments),
            Expr::New { callee, arguments } => self.eval_new(callee, arguments),
            Expr::Member { obj, property, computed } => self.eval_member(obj, property, *computed),
            Expr::StaticMember { obj, property } => self.eval_static_member(obj, property),
            Expr::Array { elems } => self.eval_array(elems),
            Expr::ArrowFunction { body, .. } => self.eval_expr(body),
            _ => format!("{:?}", expr),
        }
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
        let obj_str = self.eval_expr(obj);
        let prop_str = self.eval_expr(property);
        // Handle array length
        if obj_str.starts_with('[') {
            if !computed {
                // Static property access like arr.length
                if prop_str == "length" {
                    let count = obj_str.matches(',').count() + 1;
                    if obj_str == "[]" { return "0".to_string(); }
                    return format!("{}", count);
                }
            } else {
                // Computed index like arr[0]
                let idx = prop_str.parse::<usize>().unwrap_or(0);
                let elements: Vec<&str> = obj_str[1..obj_str.len()-1].split(", ").collect();
                if let Some(elem) = elements.get(idx) {
                    return elem.trim().to_string();
                }
            }
        }
        format!("{}[{}]", obj_str, prop_str)
    }

    fn eval_static_member(&self, obj: &Box<Expr>, property: &str) -> String {
        let obj_str = self.eval_expr(obj);
        if obj_str.starts_with('[') && property == "length" {
            let count = obj_str.matches(',').count() + 1;
            if obj_str == "[]" { return "0".to_string(); }
            return format!("{}", count);
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
        // Special handling for console.log
        if let Expr::StaticMember { obj, property } = callee {
            let obj_str = self.eval_expr(obj);
            if obj_str.contains("console") && property == "log" {
                let arg_strs: Vec<String> = args.iter().map(|a| self.eval_expr(a)).collect();
                println!("{}", arg_strs.join(" "));
                return arg_strs.join(" ");
            }
            // Handle instance method calls
            if obj_str.contains('@') {
                let method_name = property;
                if let Some(instance) = self.instances.read().get(&obj_str) {
                    if let Some(class) = self.classes.read().get(&instance.class_name) {
                        if let Some(method) = class.methods.iter().find(|m| m.name == *method_name) {
                            return self.eval_method(&method, args);
                        }
                    }
                }
            }
            return format!("{}.{}({:?})", obj_str, property, args);
        }
        let callee_expr = match callee {
            Expr::Ident { name } => {
                self.vars.read().get(name).cloned().unwrap_or_else(|| callee.clone())
            }
            Expr::Member { obj, property, .. } => {
                let obj_str = self.eval_expr(obj);
                let prop_str = self.eval_expr(property);
                return format!("{}[{}]({:?})", obj_str, prop_str, args);
            }
            _ => callee.clone(),
        };
        if let Expr::ArrowFunction { params, body, .. } = callee_expr {
            return self.eval_arrow_func(&params, &body, args);
        }
        format!("Call<{:?}>", callee_expr)
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
        if matches!(op, BinaryOp::Add)
            && (matches!(left, Expr::String(_)) || matches!(right, Expr::String(_)))
        {
            return format!("{}{}", self.eval_expr(left), self.eval_expr(right));
        }
        self.eval_num_bin_op(op, left, right)
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
