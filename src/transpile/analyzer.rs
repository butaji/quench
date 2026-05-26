//! Semantic analyzer for runts
//!
//! Performs semantic analysis on the HIR to detect islands, routes,
//! hooks usage, and validate the TS/TSX subset.

use super::hir::*;
use anyhow::Result;
use std::collections::HashSet;

/// Semantic analysis errors
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    #[error("Unsupported feature: {feature} at {location}")]
    UnsupportedFeature { feature: String, location: String },

    #[error("Type error: {message} at {location}")]
    TypeError { message: String, location: String },

    #[error("Import error: {message} at {location}")]
    ImportError { message: String, location: String },
}

/// Semantic analyzer context
pub struct Analyzer {
    /// Detected hooks usage
    pub hooks: HashSet<String>,

    /// Detected components
    pub components: HashSet<String>,

    /// Detected signals usage
    pub signals: HashSet<String>,

    /// Top-level functions
    pub functions: HashSet<String>,

    /// Top-level types
    pub types: HashSet<String>,

    /// Warnings collected
    pub warnings: Vec<String>,

    /// Errors collected
    pub errors: Vec<AnalyzeError>,

    /// Is this an island file?
    pub is_island: bool,

    /// Is this a route file?
    pub is_route: bool,

    /// Route pattern (if applicable)
    pub route_pattern: Option<String>,

    /// Is this a layout?
    pub is_layout: bool,

    /// Is this app wrapper?
    pub is_app: bool,

    /// Is this middleware?
    pub is_middleware: bool,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

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
        }
    }

    /// Analyze a module for semantic information
    pub fn analyze(&mut self, module: &Module) -> Result<(), Vec<AnalyzeError>> {
        // Reset state
        self.hooks.clear();
        self.components.clear();
        self.signals.clear();
        self.functions.clear();
        self.types.clear();
        self.warnings.clear();
        self.errors.clear();

        // Extract semantic info from file path if available
        self.analyze_file_path(&module.source);

        // Analyze imports first
        for item in &module.items {
            if let ModuleItem::Import(import) = item {
                self.analyze_import(import);
            }
        }

        // Analyze declarations
        for item in &module.items {
            match item {
                ModuleItem::Decl(decl) => self.analyze_decl(decl),
                ModuleItem::Export(export) => self.analyze_export(export),
                _ => {}
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    /// Analyze file path to determine type (island, route, etc.)
    pub fn analyze_file_path(&mut self, path: &str) {
        let path_lower = path.to_lowercase();

        // Extract filename and directory
        if let Some(filename) = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
        {
            // Check for special files
            if filename == "_app.tsx" || filename == "_app.ts" {
                self.is_app = true;
            } else if filename == "_layout.tsx" || filename == "_layout.ts" {
                self.is_layout = true;
            } else if filename.starts_with("_middleware") {
                self.is_middleware = true;
            } else if path_lower.contains("islands/") || path_lower.contains("\\islands\\") || path_lower.starts_with("islands/") {
                self.is_island = true;
            } else if path_lower.contains("routes/") || path_lower.contains("\\routes\\") || path_lower.starts_with("routes/") {
                self.is_route = true;
                self.route_pattern = Some(self.extract_route_pattern(path));
            }
        }
    }

    /// Extract route pattern from file path
    pub fn extract_route_pattern(&self, path: &str) -> String {
        let path = std::path::Path::new(path);
        
        // Get relative path from routes/ or routes
        let components: Vec<&str> = path.components()
            .filter_map(|c| c.as_os_str().to_str())
            .filter(|c| !matches!(*c, "routes" | "."))
            .collect();

        let pattern: Vec<String> = components.iter()
            .map(|c| {
                // [slug].tsx -> :slug
                if c.starts_with('[') && c.ends_with(".tsx") {
                    let inner = &c[1..c.len() - 5]; // Remove [ and .tsx (5 chars)
                    format!(":{}", inner)
                } else if c.starts_with('[') && c.ends_with(".ts") {
                    let inner = &c[1..c.len() - 3]; // Remove [ and .ts (3 chars)
                    format!(":{}", inner)
                } else if c.ends_with(".tsx") {
                    c[..c.len() - 4].to_string() // Remove .tsx (4 chars)
                } else if c.ends_with(".ts") {
                    c[..c.len() - 3].to_string() // Remove .ts (3 chars)
                } else {
                    c.to_string()
                }
            })
            .collect();

        let result = if pattern.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", pattern.join("/"))
        };

        // Handle index routes: /index -> /
        if result.ends_with("/index") {
            let without_index = result[..result.len() - 6].to_string();
            if without_index.is_empty() { "/".to_string() } else { without_index }
        } else {
            result
        }
    }

    /// Analyze an import statement
    fn analyze_import(&mut self, import: &Import) {
        let source = &import.source;

        // Track imports from known modules
        match source.as_str() {
            "preact" | "preact/hooks" | "@preact/hooks" => {
                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named { name, .. } => {
                            if self.is_hook_name(name) {
                                self.hooks.insert(name.clone());
                            }
                            if self.is_signal_name(name) {
                                self.signals.insert(name.clone());
                            }
                        }
                        ImportSpecifier::Default { name } => {
                            // Default import might be component
                            if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                                self.components.insert(name.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            "@preact/signals" | "preact/signals" => {
                for spec in &import.specifiers {
                    if let ImportSpecifier::Named { name, .. } = spec {
                        self.signals.insert(name.clone());
                    }
                }
            }
            "fresh" | "fresh/runtime" => {
                // Track Fresh-specific imports
                for spec in &import.specifiers {
                    if let ImportSpecifier::Named { name, .. } = spec {
                        match name.as_str() {
                            "IS_BROWSER" => {} // Known constant
                            "PageProps" => { let _ = self.types.insert(name.clone()); }
                            "Handler" | "Handlers" | "HandlerContext" => {
                                let _ = self.types.insert(name.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                // External imports - track for potential issues
                if source.starts_with('$') {
                    // Fresh alias imports (e.g., $fresh/server)
                    for spec in &import.specifiers {
                        if let ImportSpecifier::Named { name, .. } = spec {
                            match name.as_str() {
                                "PageProps" | "Handlers" | "HandlerContext" => {
                                    let _ = self.types.insert(name.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check if name is a hook
    pub fn is_hook_name(&self, name: &str) -> bool {
        matches!(
            name,
            "useState" | "useEffect" | "useRef" | "useMemo" | "useCallback"
                | "useReducer" | "useContext" | "useLayoutEffect" | "useDebugValue"
                | "useImperativeHandle" | "useInsertionEffect"
                | "useTransition" | "useDeferredValue" | "useId"
                | "useSyncExternalStore" | "useFormInput" | "useFormAction"
        )
    }

    /// Check if name is a signal API
    pub fn is_signal_name(&self, name: &str) -> bool {
        matches!(
            name,
            "signal" | "useSignal" | "useSignalEffect" | "useComputed"
                | "signalEffect" | "batch" | "useBatch"
                | "Signal" | "ReadSignal" | "WriteSignal"
        )
    }

    /// Analyze a declaration
    fn analyze_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function(f) => self.analyze_function(f),
            Decl::Variable(v) => self.analyze_variable(v),
            Decl::Type(t) => {
                self.types.insert(t.name.clone());
                // Don't analyze type bodies for now
            }
            Decl::Class(c) => {
                self.errors.push(AnalyzeError::UnsupportedFeature {
                    feature: format!("class component '{}'", c.name),
                    location: format!("at {}", c.name),
                });
            }
        }
    }

    /// Analyze a function declaration
    fn analyze_function(&mut self, func: &FunctionDecl) {
        self.functions.insert(func.name.clone());

        // Check if this looks like a component
        if func.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            self.components.insert(func.name.clone());
        }

        // Analyze body if present
        // FunctionDecl.body is Option<Block> where Block is a wrapper around Vec<Stmt>
        if let Some(ref block) = func.body {
            for stmt in &block.0 {
                self.analyze_stmt(stmt);
            }
        }
    }

    /// Analyze a variable declaration
    fn analyze_variable(&mut self, var: &VariableDecl) {
        if let Some(init) = &var.init {
            self.analyze_expr(init);
        }
    }

    /// Analyze an export
    fn analyze_export(&mut self, export: &Export) {
        match export {
            Export::Default { expr } => {
                self.analyze_expr(expr);
            }
            Export::NamedWithValue { name, value } => {
                // Check if this is a route handler
                if name == "handler" {
                    self.is_route = true;
                }
                self.analyze_expr(value);
            }
            _ => {}
        }
    }

    /// Analyze a block of statements
    #[allow(dead_code)]
    fn analyze_block(&mut self, block: &Block) {
        for stmt in &block.0 {
            self.analyze_stmt(stmt);
        }
    }

    /// Analyze a statement
    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Empty => {}
            Stmt::Block(stmts) => {
                // Stmt::Block contains Vec<Stmt> directly
                for s in stmts {
                    self.analyze_stmt(s);
                }
            }
            Stmt::Expr { expr } => self.analyze_expr(expr),
            Stmt::Variable { decl } => {
                if let Some(init) = &decl.init {
                    self.analyze_expr(init);
                }
            }
            Stmt::Return { arg } => {
                if let Some(expr) = arg {
                    self.analyze_expr(expr);
                }
            }
            Stmt::If { test, consequent, alternate } => {
                self.analyze_expr(test);
                self.analyze_stmt(consequent);
                if let Some(alt) = alternate {
                    self.analyze_stmt(alt);
                }
            }
            Stmt::While { test, body } => {
                self.analyze_expr(test);
                self.analyze_stmt(body);
            }
            Stmt::For { init, test, update, body } => {
                if let Some(init) = init {
                    match init {
                        ForInit::Variable(v) => {
                            if let Some(e) = &v.init {
                                self.analyze_expr(e);
                            }
                        }
                        ForInit::Expr(e) => self.analyze_expr(e),
                    }
                }
                if let Some(test) = test {
                    self.analyze_expr(test);
                }
                if let Some(update) = update {
                    self.analyze_expr(update);
                }
                self.analyze_stmt(body);
            }
            Stmt::ForIn { left, right, body, .. } => {
                self.analyze_for_init(left);
                self.analyze_expr(right);
                self.analyze_stmt(body);
            }
            Stmt::ForOf { left, right, body, .. } => {
                self.analyze_for_init(left);
                self.analyze_expr(right);
                self.analyze_stmt(body);
            }
            Stmt::Switch { discriminant, cases } => {
                self.analyze_expr(discriminant);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.analyze_expr(test);
                    }
                    for s in &case.consequent {
                        self.analyze_stmt(s);
                    }
                }
            }
            Stmt::Throw { arg } => self.analyze_expr(arg),
            Stmt::Try { block, handler, finalizer } => {
                // block is Box<Stmt>, not Block
                self.analyze_stmt(block);
                if let Some(h) = handler {
                    self.analyze_stmt(h);
                }
                if let Some(f) = finalizer {
                    self.analyze_stmt(f);
                }
            }
            Stmt::Function { decl } => self.analyze_function(decl),
            Stmt::Class { .. } => {},
            _ => {}
        }
    }

    /// Analyze for loop initializer
    fn analyze_for_init(&mut self, init: &ForInit) {
        match init {
            ForInit::Variable(v) => {
                if let Some(e) = &v.init {
                    self.analyze_expr(e);
                }
            }
            ForInit::Expr(e) => self.analyze_expr(e),
        }
    }

    /// Analyze an expression
    fn analyze_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::String(_) | Expr::Number(_) | Expr::BigInt(_)
            | Expr::Boolean(_) | Expr::Null | Expr::Undefined
            | Expr::RegExp { .. } | Expr::Ident { .. } => {}

            Expr::Template { parts, exprs } => {
                for part in parts {
                    if let TemplatePart::Type(t) = part {
                        let _ = t; // Don't recurse into types
                    }
                }
                for e in exprs {
                    self.analyze_expr(e);
                }
            }

            Expr::JSX(jsx) => self.analyze_jsx(jsx),

            Expr::Bin { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            Expr::Unary { arg, .. } => self.analyze_expr(arg),
            Expr::Update { arg, .. } => self.analyze_expr(arg),
            Expr::Logical { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            Expr::Cond { test, consequent, alternate } => {
                self.analyze_expr(test);
                self.analyze_expr(consequent);
                self.analyze_expr(alternate);
            }

            Expr::Call { callee, args, .. } => {
                self.analyze_expr(callee);
                for arg in args {
                    self.analyze_expr(arg);
                }
            }
            Expr::New { callee, args, .. } => {
                self.analyze_expr(callee);
                for arg in args {
                    self.analyze_expr(arg);
                }
            }
            Expr::TaggedTemplate { tag, template } => {
                self.analyze_expr(tag);
                self.analyze_expr(template);
            }

            Expr::Member { object, property, .. } => {
                self.analyze_expr(object);
                self.analyze_expr(property);
            }

            Expr::Object { props } => {
                for prop in props {
                    match prop {
                        ObjectProp::Init { value, .. } => self.analyze_expr(value),
                        ObjectProp::Spread { value } => self.analyze_expr(value),
                        _ => {}
                    }
                }
            }
            Expr::Array { elems } => {
                for elem in elems {
                    if let Some(e) = elem {
                        self.analyze_expr(e);
                    }
                }
            }

            Expr::Arrow { body, .. } => {
                // Arrow body is Box<Stmt> - analyze the inner statement
                self.analyze_stmt(body);
            },
            Expr::Function { decl } => {
                // Functions are analyzed at declaration level
                let _ = decl;
            }

            Expr::Await { arg } => self.analyze_expr(arg),
            Expr::Yield { arg, .. } => {
                if let Some(a) = arg {
                    self.analyze_expr(a);
                }
            }

            Expr::Class { .. } => {
                // Class expressions not supported
            }

            Expr::TSAs { expr, .. } => self.analyze_expr(expr),

            Expr::Seq { exprs } => {
                for e in exprs {
                    self.analyze_expr(e);
                }
            }

            Expr::Spread { arg } => self.analyze_expr(arg),

            Expr::Assign { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }

            Expr::MetaProp { .. } => {}
        }
    }

    /// Recursively analyze JSX
    fn analyze_jsx(&mut self, jsx: &JSXExpr) {
        // Analyze attributes
        for attr in &jsx.opening.attrs {
            match attr {
                JSXAttr::Expr { expr, .. } => self.analyze_expr(expr),
                JSXAttr::Spread { expr } => self.analyze_expr(expr),
                _ => {}
            }
        }

        // Analyze children
        for child in &jsx.children {
            match child {
                JSXChild::Expr(e) => self.analyze_expr(e),
                JSXChild::JSX(j) => self.analyze_jsx(j),
                JSXChild::Fragment { children } => {
                    for c in children {
                        if let JSXChild::JSX(j) = c {
                            self.analyze_jsx(j);
                        }
                    }
                }
                JSXChild::Spread { expr } => self.analyze_expr(expr),
                JSXChild::Text(_) => {}
            }
        }
    }

    /// Get semantic info summary
    #[allow(dead_code)]
    pub fn get_semantic_info(&self) -> SemanticInfo {
        SemanticInfo {
            is_island: self.is_island,
            is_route: self.is_route,
            route_pattern: self.route_pattern.clone(),
            is_app: self.is_app,
            is_layout: self.is_layout,
            is_middleware: self.is_middleware,
            hooks: self.hooks.iter().cloned().collect(),
            components: self.components.iter().cloned().collect(),
            functions: self.functions.iter().cloned().collect(),
        }
    }
}

/// Validation rules for supported subset
#[allow(dead_code)]
pub struct Validator;

#[allow(dead_code)]
impl Validator {
    /// Validate that a module only uses supported features
    pub fn validate_module(module: &Module) -> Vec<AnalyzeError> {
        let mut errors = Vec::new();

        for item in &module.items {
            if let ModuleItem::Decl(Decl::Class(c)) = item {
                errors.push(AnalyzeError::UnsupportedFeature {
                    feature: format!("class component '{}'", c.name),
                    location: format!("routes/{}", c.name),
                });
            }
        }

        errors
    }
}
