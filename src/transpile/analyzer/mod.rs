//! Semantic analyzer for runts
//!

use super::hir::*;
use super::hir::{ForInit, ObjectProp};
use std::collections::{HashMap, HashSet};

pub mod helpers;

#[allow(dead_code)]
#[derive(Debug, Clone, thiserror::Error)]
pub enum AnalyzeError {
    #[error("Unsupported feature: {feature} at {location}")]
    UnsupportedFeature { feature: String, location: String },
    #[error("Type error: {message} at {location}")]
    TypeError { message: String, location: String },
    #[error("Import error: {message} at {location}")]
    ImportError { message: String, location: String },
}

#[allow(dead_code)]
pub struct Analyzer {
    pub hooks: HashSet<String>,
    pub components: HashSet<String>,
    pub signals: HashSet<String>,
    pub functions: HashSet<String>,
    pub types: HashSet<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<AnalyzeError>,
    pub is_island: bool,
    pub is_route: bool,
    pub route_pattern: Option<String>,
    pub is_layout: bool,
    pub is_app: bool,
    pub is_middleware: bool,
    pub current_file: String,
    /// Type environment for validation
    type_env: HashMap<String, Type>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TypeMismatch {
    expected: Type,
    actual: Type,
    location: String,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl Analyzer {
    pub fn new() -> Self {
        Self {
            hooks: HashSet::new(),
            components: HashSet::new(),
            signals: HashSet::new(),
            functions: HashSet::new(),
            types: HashSet::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            is_island: false,
            is_route: false,
            route_pattern: None,
            is_layout: false,
            is_app: false,
            is_middleware: false,
            current_file: String::new(),
            type_env: HashMap::new(),
        }
    }

    pub fn analyze_file_path(&mut self, path: &str) {
        self.is_island = path.contains("islands/") || path.contains("_island");
        self.is_route = path.contains("routes/") && !path.starts_with("routes/_");
        self.is_layout = path.contains("routes/_layout") || path.contains("layouts/");
        self.is_app = path.ends_with("_app.ts") || path.ends_with("_app.tsx");
        self.is_middleware = path.contains("_middleware");
        if self.is_route {
            self.route_pattern = Some(self.extract_route_pattern(path));
        }
    }

    pub fn analyze(&mut self, module: &Module) -> Result<(), Vec<AnalyzeError>> {
        self.hooks.clear();
        self.components.clear();
        self.signals.clear();
        self.functions.clear();
        self.types.clear();
        self.warnings.clear();
        self.errors.clear();
        self.type_env.clear();

        // Populate type environment from module.types
        for (name, type_def) in &module.types {
            self.type_env.insert(name.clone(), type_def.type_.clone());
            self.types.insert(name.clone());
        }

        for item in &module.items {
            match item {
                ModuleItem::Import(imp) => self.analyze_import(imp),
                ModuleItem::Decl(decl) => self.analyze_decl(decl),
                ModuleItem::Stmt(stmt) => self.analyze_stmt(stmt),
                ModuleItem::Export(_) => {}
            }
        }
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    fn analyze_import(&mut self, imp: &Import) {
        if imp.source.contains("preact") || imp.source.contains("signals") {
            for spec in &imp.specifiers {
                if let ImportSpecifier::Named { name, .. } = spec {
                    self.analyze_import_spec(name);
                }
            }
        }
    }

    fn analyze_import_spec(&mut self, name: &str) {
        if name.starts_with("use") {
            self.hooks.insert(name.to_string());
        }
        if name == "signal" || name == "computed" || name == "effect" {
            self.signals.insert(name.to_string());
        }
    }

    fn analyze_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function(f) => {
                self.functions.insert(f.name.clone());
                self.validate_function_signature(f);
                self.analyze_function_body(f);
            }
            Decl::Type(t) => {
                self.types.insert(t.name.clone());
                self.type_env.insert(t.name.clone(), t.type_.clone());
                self.validate_type_compatibility(&t.type_, &t.name);
            }
            Decl::Class(c) => {
                self.components.insert(c.name.clone());
                self.validate_class_members(c);
            }
            Decl::Variable(v) => {
                self.validate_variable_decl(v);
            }
        }
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDecl(func) => self.analyze_function_body(func),
            Stmt::For { .. } | Stmt::ForIn { .. } | Stmt::ForOf { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => {
                self.analyze_stmt_iteration(stmt)
            }
            Stmt::If { test, consequent, alternate } => self.analyze_if(test, consequent, alternate),
            Stmt::Switch { discriminant, cases } => self.analyze_switch(discriminant, cases),
            Stmt::Return { .. } | Stmt::Throw { .. } => self.analyze_control_transfer(stmt),
            Stmt::Try { .. } | Stmt::Block { .. } | Stmt::Expr { .. } | Stmt::With { .. } => {
                self.analyze_stmt_block_like(stmt)
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Empty | Stmt::Labeled { .. } => {}
            _ => self.analyze_stmt_remaining(stmt),
        }
    }

    fn analyze_control_transfer(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return { arg } => self.analyze_return(arg),
            Stmt::Throw { arg } => self.analyze_expr(arg),
            _ => {}
        }
    }

    fn analyze_stmt_iteration(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::For { init, test, update, body } => self.analyze_for_loop(init, test, update, body),
            Stmt::ForIn { left, right, body } => self.analyze_for_in(left, right, body),
            Stmt::ForOf { left, right, body, .. } => self.analyze_for_of(left, right, body),
            Stmt::While { test, body } => self.analyze_while(test, body),
            Stmt::DoWhile { body, test } => self.analyze_do_while(body, test),
            _ => {}
        }
    }

    fn analyze_stmt_block_like(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Try { block, handler, finalizer } => self.analyze_try(block, handler, finalizer),
            Stmt::Block { stmts } => self.analyze_block(stmts),
            Stmt::Expr { expr } => self.analyze_expr(expr),
            Stmt::With { obj, body } => self.analyze_with(obj, body),
            _ => {}
        }
    }

    fn analyze_stmt_remaining(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Class(_) | Stmt::Variable(_) => {}
            Stmt::ExportNamed { .. } | Stmt::ExportDefault { .. } => {}
            Stmt::ImportNamed { .. } | Stmt::ImportDefault { .. } => {}
            _ => {}
        }
    }

    fn analyze_for_loop(&mut self, init: &Option<ForInit>, test: &Option<Expr>, update: &Option<Expr>, body: &Box<Stmt>) {
        self.analyze_for_init(init);
        if let Some(t) = test {
            self.validate_expr_type(t, &Type::Boolean);
        }
        if let Some(u) = update {
            self.validate_expr_type(u, &Type::Unknown);
        }
        self.analyze_stmt(body);
    }

    fn analyze_for_in(&mut self, left: &ForInit, right: &Expr, body: &Box<Stmt>) {
        self.analyze_for_init(&Some(left.clone()));
        self.analyze_expr(right);
        self.analyze_stmt(body);
    }

    fn analyze_for_of(&mut self, left: &ForInit, right: &Expr, body: &Box<Stmt>) {
        self.analyze_for_init(&Some(left.clone()));
        self.analyze_expr(right);
        self.analyze_stmt(body);
    }

    fn analyze_while(&mut self, test: &Expr, body: &Box<Stmt>) {
        self.validate_expr_type(test, &Type::Boolean);
        self.analyze_stmt(body);
    }

    fn analyze_do_while(&mut self, body: &Box<Stmt>, test: &Expr) {
        self.analyze_stmt(body);
        self.validate_expr_type(test, &Type::Boolean);
    }

    fn analyze_if(&mut self, test: &Expr, consequent: &Box<Stmt>, alternate: &Option<Box<Stmt>>) {
        self.validate_expr_type(test, &Type::Boolean);
        self.analyze_stmt(consequent);
        if let Some(a) = alternate {
            self.analyze_stmt(a);
        }
    }

    fn analyze_switch(&mut self, discriminant: &Expr, cases: &[SwitchCase]) {
        self.analyze_expr(discriminant);
        for case in cases {
            for s in &case.consequent {
                self.analyze_stmt(s);
            }
        }
    }

    fn analyze_return(&mut self, arg: &Option<Expr>) {
        if let Some(e) = arg {
            self.analyze_expr(e);
        }
    }

    fn analyze_try(&mut self, block: &Block, handler: &Option<CatchClause>, finalizer: &Option<Block>) {
        for s in &block.0 {
            self.analyze_stmt(s);
        }
        if let Some(h) = handler {
            for s in &h.body.0 {
                self.analyze_stmt(s);
            }
        }
        if let Some(f) = finalizer {
            for s in &f.0 {
                self.analyze_stmt(s);
            }
        }
    }

    fn analyze_block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.analyze_stmt(s);
        }
    }

    fn analyze_with(&mut self, obj: &Expr, body: &Box<Stmt>) {
        self.analyze_expr(obj);
        self.analyze_stmt(body);
    }

    fn analyze_for_init(&mut self, init: &Option<ForInit>) {
        match init {
            Some(ForInit::Variable(_kind, vars)) => {
                for (_name, init_expr) in vars {
                    if let Some(e) = init_expr {
                        self.analyze_expr(e);
                    }
                }
            }
            Some(ForInit::Expr(e)) => {
                self.analyze_expr(e);
            }
            None => {}
        }
    }

    fn analyze_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call { callee, arguments } => self.analyze_call(callee, arguments),
            Expr::New { callee, arguments } => self.analyze_new(callee, arguments),
            Expr::Bin { left, right, .. } | Expr::Logical { left, right, .. } | Expr::Assign { left, right, .. } | Expr::Seq { left, right } => {
                self.analyze_bin(left, right)
            }
            Expr::Cond { test, consequent, alternate } => self.analyze_cond(test, consequent, alternate),
            Expr::Update { arg, .. } | Expr::Unary { arg, .. } | Expr::Await { arg } | Expr::Spread { arg } => {
                self.analyze_expr(arg)
            }
            Expr::Array { elems } => self.analyze_array(elems),
            Expr::Object { members } => self.analyze_object(members),
            _ => {}
        }
    }

    fn analyze_call(&mut self, callee: &Box<Expr>, arguments: &[Expr]) {
        self.analyze_expr(callee);
        for a in arguments {
            self.analyze_expr(a);
        }
    }

    fn analyze_new(&mut self, callee: &Box<Expr>, arguments: &[Expr]) {
        self.analyze_expr(callee);
        for a in arguments {
            self.analyze_expr(a);
        }
    }

    fn analyze_bin(&mut self, left: &Box<Expr>, right: &Box<Expr>) {
        self.analyze_expr(left);
        self.analyze_expr(right);
    }

    fn analyze_cond(&mut self, test: &Box<Expr>, consequent: &Box<Expr>, alternate: &Box<Expr>) {
        self.analyze_expr(test);
        self.validate_expr_type(test, &Type::Boolean);
        self.analyze_expr(consequent);
        self.analyze_expr(alternate);
    }

    fn analyze_array(&mut self, elems: &[Option<Expr>]) {
        for e in elems {
            if let Some(x) = e {
                self.analyze_expr(x);
            }
        }
    }

    fn analyze_object(&mut self, members: &[ObjectMemberExpr]) {
        for m in members {
            if let ObjectProp::Init { value, .. } = &m.prop {
                self.analyze_expr(value);
            }
        }
    }

    fn validate_expr_type(&mut self, expr: &Expr, expected: &Type) {
        let actual = self.infer_type(expr);
        if !self.types_compatible(expected, &actual) {
            self.errors.push(AnalyzeError::TypeError {
                message: format!("Expected {:?} but found {:?}", expected, actual),
                location: format!("expr"),
            });
        }
    }

    fn infer_type(&self, expr: &Expr) -> Type {
        match expr {
            Expr::Number(_) => Type::Number,
            Expr::String(_) => Type::String,
            Expr::Boolean(_) => Type::Boolean,
            Expr::Null | Expr::Undefined | Expr::BigInt(_) => Type::Unknown,
            Expr::Ident { name } => self.infer_ident_type(name),
            Expr::Array { .. } => Type::Array { elem: Box::new(Type::Unknown) },
            Expr::Object { .. } => Type::Object { members: vec![] },
            _ => Type::Unknown,
        }
    }

    fn infer_ident_type(&self, name: &str) -> Type {
        self.type_env.get(name).cloned().unwrap_or(Type::Unknown)
    }

    fn types_compatible(&self, expected: &Type, actual: &Type) -> bool {
        self.is_universal_type(expected) || self.is_universal_type(actual) || self.is_same_primitive(expected, actual)
    }

    fn is_universal_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Unknown | Type::Any)
    }

    fn is_same_primitive(&self, expected: &Type, actual: &Type) -> bool {
        matches!(
            (expected, actual),
            (Type::Number, Type::Number)
                | (Type::String, Type::String)
                | (Type::Boolean, Type::Boolean)
                | (Type::Null, Type::Undefined)
                | (Type::Undefined, Type::Null)
        )
    }
}

mod validate;

#[allow(dead_code)]
impl Analyzer {
    pub fn add_warning(&mut self, msg: String) {
        self.warnings.push(msg);
    }
    pub fn add_error(&mut self, err: AnalyzeError) {
        self.errors.push(err);
    }

    pub fn extract_route_pattern(&self, path: &str) -> String {
        let path = path.replace("routes/", "/").replace("routes", "/");
        let mut pattern = path
            .replace("/index.tsx", "")
            .replace("/index.ts", "")
            .replace(".tsx", "")
            .replace(".ts", "");
        pattern = pattern.replace("[", ":").replace("]", "");
        if pattern.is_empty() {
            "/".to_string()
        } else {
            pattern
        }
    }

    pub fn is_hook_name(&self, name: &str) -> bool {
        helpers::is_hook_name(name)
    }

    pub fn is_signal_name(&self, name: &str) -> bool {
        helpers::is_signal_name(name)
    }
}
