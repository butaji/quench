//! Semantic analyzer for runts

use super::hir::*;
use std::collections::HashSet;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AnalyzeError {
    #[error("Unsupported feature: {feature} at {location}")]
    UnsupportedFeature { feature: String, location: String },
    #[error("Type error: {message} at {location}")]
    TypeError { message: String, location: String },
    #[error("Import error: {message} at {location}")]
    ImportError { message: String, location: String },
}

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
}

impl Default for Analyzer {
    fn default() -> Self { Self::new() }
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
            current_file: String::new(),
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
        
        for item in &module.items {
            match item {
                ModuleItem::Import(imp) => {
                    if imp.source.contains("preact") || imp.source.contains("signals") {
                        for spec in &imp.specifiers {
                            if let ImportSpecifier::Named { name, .. } = spec {
                                if name.starts_with("use") { self.hooks.insert(name.clone()); }
                                if name == "signal" || name == "computed" || name == "effect" { self.signals.insert(name.clone()); }
                            }
                        }
                    }
                }
                ModuleItem::Decl(decl) => {
                    match decl {
                        Decl::Function(f) => { self.functions.insert(f.name.clone()); }
                        Decl::Variable(v) => { drop(v.init.clone()); }
                        Decl::Type(t) => { self.types.insert(t.name.clone()); }
                        Decl::Class(c) => { self.components.insert(c.name.clone()); }
                    }
                }
                _ => {}
            }
        }
        
        if self.errors.is_empty() { Ok(()) } else { Err(self.errors.clone()) }
    }
    
    pub fn add_warning(&mut self, msg: String) { self.warnings.push(msg); }
    pub fn add_error(&mut self, err: AnalyzeError) { self.errors.push(err); }

    pub fn extract_route_pattern(&self, path: &str) -> String {
        let path = path.replace("routes/", "/").replace("routes", "/");
        let mut pattern = path.replace("/index.tsx", "").replace("/index.ts", "").replace(".tsx", "").replace(".ts", "");
        pattern = pattern.replace("[", ":").replace("]", "");
        if pattern.is_empty() { "/".to_string() } else { pattern }
    }

    pub fn is_hook_name(&self, name: &str) -> bool {
        name.starts_with("use") && name.len() > 3
    }

    pub fn is_signal_name(&self, name: &str) -> bool {
        name == "signal" || name.starts_with("signal") || name.starts_with("useSignal") || name.starts_with("useComputed")
    }
}
