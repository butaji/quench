//! Semantic analyzer for runts
//!
//! allow:complexity,too_many_lines

use super::hir::*;
use super::hir::{ForInit, ObjectProp};
use std::collections::{HashMap, HashSet};

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

    // allow:complexity,too_many_lines
    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDecl(func) => {
                self.analyze_function_body(func);
            }
            Stmt::For { init, test, update, body } => {
                self.analyze_for_init(init);
                if let Some(t) = test {
                    self.validate_expr_type(t, &Type::Boolean);
                }
                if let Some(u) = update {
                    self.validate_expr_type(u, &Type::Unknown);
                }
                self.analyze_stmt(body);
            }
            Stmt::ForIn { left, right, body } => {
                self.analyze_for_init(left);
                self.analyze_expr(right);
                self.analyze_stmt(body);
            }
            Stmt::ForOf { left, right, body, .. } => {
                self.analyze_for_init(left);
                self.analyze_expr(right);
                self.analyze_stmt(body);
            }
            Stmt::While { test, body } => {
                self.validate_expr_type(test, &Type::Boolean);
                self.analyze_stmt(body);
            }
            Stmt::DoWhile { body, test } => {
                self.analyze_stmt(body);
                self.validate_expr_type(test, &Type::Boolean);
            }
            Stmt::If { test, consequent, alternate } => {
                self.validate_expr_type(test, &Type::Boolean);
                self.analyze_stmt(consequent);
                if let Some(a) = alternate {
                    self.analyze_stmt(a);
                }
            }
            Stmt::Switch { discriminant, cases } => {
                self.analyze_expr(discriminant);
                for case in cases {
                    for s in &case.consequent {
                        self.analyze_stmt(s);
                    }
                }
            }
            Stmt::Return { arg } => {
                if let Some(e) = arg {
                    self.analyze_expr(e);
                }
            }
            Stmt::Throw { arg } => {
                self.analyze_expr(arg);
            }
            Stmt::Try { block, handler, finalizer } => {
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
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.analyze_stmt(s);
                }
            }
            Stmt::Expr { expr } => {
                self.analyze_expr(expr);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Empty => {}
            Stmt::Labeled { body, .. } => self.analyze_stmt(body),
            Stmt::With { obj, body } => {
                self.analyze_expr(obj);
                self.analyze_stmt(body);
            }
            _ => {}
        }
    }

    fn analyze_for_init(&mut self, init: &Option<ForInit>) {
        match init {
            Some(ForInit::Variable(kind, vars)) => {
                for (name, init_expr) in vars {
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

    // allow:complexity,too_many_lines
    fn analyze_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call { callee, arguments } => {
                self.analyze_expr(callee);
                for a in arguments {
                    self.analyze_expr(a);
                }
            }
            Expr::New { callee, arguments } => {
                self.analyze_expr(callee);
                for a in arguments {
                    self.analyze_expr(a);
                }
            }
            Expr::Bin { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            Expr::Logical { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            Expr::Cond { test, consequent, alternate } => {
                self.analyze_expr(test);
                self.validate_expr_type(test, &Type::Boolean);
                self.analyze_expr(consequent);
                self.analyze_expr(alternate);
            }
            Expr::Assign { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            Expr::Update { arg, .. } => {
                self.analyze_expr(arg);
            }
            Expr::Unary { arg, .. } => {
                self.analyze_expr(arg);
            }
            Expr::Await { arg } => {
                self.analyze_expr(arg);
            }
            Expr::Array { elems } => {
                for e in elems {
                    if let Some(x) = e {
                        self.analyze_expr(x);
                    }
                }
            }
            Expr::Object { members } => {
                for m in members {
                    if let ObjectProp::Init { value, .. } = &m.prop {
                        self.analyze_expr(value);
                    }
                }
            }
            Expr::Seq { left, right } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            Expr::Spread { arg } => {
                self.analyze_expr(arg);
            }
            _ => {}
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

    // allow:complexity,too_many_lines
    fn infer_type(&self, expr: &Expr) -> Type {
        match expr {
            Expr::Number(_) => Type::Number,
            Expr::String(_) => Type::String,
            Expr::Boolean(_) => Type::Boolean,
            Expr::Null => Type::Null,
            Expr::Undefined => Type::Undefined,
            Expr::BigInt(_) => Type::BigInt,
            Expr::Ident { name } => {
                self.type_env.get(name).cloned().unwrap_or(Type::Unknown)
            }
            Expr::Array { .. } => Type::Array {
                elem: Box::new(Type::Unknown),
            },
            Expr::Object { .. } => Type::Object { members: vec![] },
            _ => Type::Unknown,
        }
    }

    // allow:complexity,too_many_lines
    fn types_compatible(&self, expected: &Type, actual: &Type) -> bool {
        match (expected, actual) {
            (Type::Unknown, _) => true,
            (_, Type::Unknown) => true,
            (Type::Any, _) => true,
            (_, Type::Any) => true,
            (Type::Number, Type::Number) => true,
            (Type::String, Type::String) => true,
            (Type::Boolean, Type::Boolean) => true,
            (Type::Null, Type::Undefined) => true,
            (Type::Undefined, Type::Null) => true,
            _ => false,
        }
    }

    fn validate_function_signature(&mut self, func: &FunctionDecl) {
        // Validate return type annotation
        if let Some(ret_type) = &func.return_type {
            self.validate_type_compatibility(ret_type, "return type");
        }
        // Validate parameter types
        for param in &func.params {
            if let Some(param_type) = &param.type_ {
                self.validate_type_compatibility(param_type, &format!("parameter '{}'", param.name));
            }
        }
        // Validate error type if function throws
        if func.throws {
            if let Some(err_type) = &func.error_type {
                self.validate_type_compatibility(err_type, "error type");
            }
        }
    }

    fn analyze_function_body(&mut self, func: &FunctionDecl) {
        if let Some(body) = &func.body {
            for stmt in &body.0 {
                self.analyze_stmt(stmt);
            }
        }
    }

    // allow:complexity,too_many_lines
    fn validate_type_compatibility(&mut self, ty: &Type, context: &str) {
        match ty {
            Type::Ref { name, generics } => {
                // Check if referenced type exists
                if !self.types.contains(name) && !self.functions.contains(name) {
                    // Could be external type, just warn
                    self.warnings.push(format!("Unknown type reference: {}", name));
                }
                // Validate generics
                for g in generics {
                    self.validate_type_compatibility(g, context);
                }
            }
            Type::Union { types } => {
                for t in types {
                    self.validate_type_compatibility(t, context);
                }
            }
            Type::Intersection { types } => {
                for t in types {
                    self.validate_type_compatibility(t, context);
                }
            }
            Type::Array { elem } => {
                self.validate_type_compatibility(elem, context);
            }
            Type::Function { params, ret } => {
                for p in params {
                    self.validate_type_compatibility(p, context);
                }
                self.validate_type_compatibility(ret, context);
            }
            Type::Object { members } => {
                for m in members {
                    self.validate_type_compatible(m);
                }
            }
            Type::Conditional { check, extends, true_type, false_type } => {
                self.validate_type_compatibility(check, context);
                self.validate_type_compatibility(extends, context);
                self.validate_type_compatibility(true_type, context);
                self.validate_type_compatibility(false_type, context);
            }
            Type::Mapped { from, to } => {
                self.validate_type_compatibility(from, context);
                self.validate_type_compatibility(to, context);
            }
            Type::Index { obj, index } => {
                self.validate_type_compatibility(obj, context);
                self.validate_type_compatibility(index, context);
            }
            _ => {}
        }
    }

    fn validate_type_compatible(&mut self, member: &TypeMember) {
        self.validate_type_compatibility(&member.type_, &format!("member '{}'", member.key));
    }

    fn validate_class_members(&mut self, class: &ClassDecl) {
        for member in &class.members {
            if let Some(ty) = &member.type_ {
                self.validate_type_compatibility(ty, &format!("class member '{}'", member.name));
            }
        }
    }

    fn validate_variable_decl(&mut self, var: &VariableDecl) {
        if let Some(ty) = &var.type_ {
            self.validate_type_compatibility(ty, &format!("variable '{}'", var.name));
        }
        if let Some(init) = &var.init {
            if let Some(expected) = &var.type_ {
                let actual = self.infer_type(init);
                if !self.types_compatible(expected, &actual) {
                    self.errors.push(AnalyzeError::TypeError {
                        message: format!("Variable '{}' has type {:?} but is initialized with {:?}", var.name, expected, actual),
                        location: format!("variable '{}'", var.name),
                    });
                }
            }
        }
    }

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
        name.starts_with("use") && name.len() > 3
    }

    pub fn is_signal_name(&self, name: &str) -> bool {
        name == "signal"
            || name.starts_with("signal")
            || name.starts_with("useSignal")
            || name.starts_with("useComputed")
    }
}
