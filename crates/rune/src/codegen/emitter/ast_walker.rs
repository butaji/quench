//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.

use crate::codegen::emitter::type_collector::{EmissionTracker, TypeCollector};
use super::{CodeEmitter, RustType};
use crate::utils::to_snake_case;
use std::collections::{HashMap, HashSet};
use swc_ecma_ast::{Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem, Stmt};

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    /// Code emitter
    emitter: CodeEmitter,
    /// Type collector
    collector: TypeCollector,
    /// Module name (for imports)
    #[allow(dead_code)]
    module_name: String,
    /// Known imports (path → names)
    imports: HashMap<String, Vec<String>>,
    /// Native imports (module names that use native: prefix)
    native_imports: HashSet<String>,
    /// Tracks what's been emitted
    emission_tracker: EmissionTracker,
}

impl AstWalker {
    /// Create a new AST walker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            emitter: CodeEmitter::new(),
            collector: TypeCollector::new(),
            module_name: String::new(),
            imports: HashMap::new(),
            native_imports: HashSet::new(),
            emission_tracker: EmissionTracker::default(),
        }
    }

    /// Walk a module and emit Rust code.
    pub fn walk_module(&mut self, module: &Module) {
        self.collect_imports(module);
        self.collect_types(module);
        self.emit_imports();
        self.emit_named_types();
        self.emit_functions(module);
        self.emit_anonymous_structs();
    }

    /// Emit use statements for collected imports.
    fn emit_imports(&mut self) {
        // Protocol types (Task, AppState, Filter) are re-exported by mod.rs
        // No need to import them here
        // Native imports are handled by RustEmitter::write_native_imports in core.rs

        for (path, names) in &self.imports {
            let clean_path = path.trim_matches('"');

            // Skip native imports - they're handled separately in core.rs
            if clean_path.starts_with("native:") {
                continue;
            }

            // Skip imports of protocol types (Task, AppState, Filter) - they're in scope via mod.rs
            let protocol_types = ["Task", "AppState", "Filter"];
            let is_protocol_import = names.iter().all(|n| protocol_types.contains(&n.as_str()));
            if is_protocol_import {
                continue;
            }

            // Convert relative path to Rust module path
            let rust_path = self.convert_import_path(clean_path);
            let names_str = names.join(", ");
            self.emitter.push_line(&format!("use {}::{{{}}};", rust_path, names_str));
        }
    }

    /// Convert TypeScript import path to Rust module path.
    fn convert_import_path(&self, path: &str) -> String {
        // Handle relative imports
        if path.starts_with("./") {
            let path = path.trim_start_matches("./");
            let path = path.replace(".r.ts", "").replace(".r.tsx", "");
            let parts: Vec<&str> = path.split('/').collect();
            let rust_parts: Vec<String> = parts.iter().map(|p| to_snake_case(p)).collect();
            return format!("crate::generated::{}", rust_parts.join("::"));
        }

        // Handle absolute imports (just use as-is for now)
        path.to_string()
    }

    fn collect_imports(&mut self, module: &Module) {
        for item in &module.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                let path_str = format!("{:?}", import.src.value);
                if path_str.starts_with("\"native:") {
                    let module_name = path_str.trim_start_matches("\"native:").trim_end_matches('"');
                    self.native_imports.insert(module_name.to_string());
                }

                let names: Vec<String> = import.specifiers.iter()
                    .map(|spec| match spec {
                        swc_ecma_ast::ImportSpecifier::Named(named) => named.local.as_ref().to_string(),
                        swc_ecma_ast::ImportSpecifier::Default(_) => "default".to_string(),
                        swc_ecma_ast::ImportSpecifier::Namespace(ns) => format!("*{}", to_snake_case(ns.local.as_ref())),
                    })
                    .collect();
                self.imports.insert(path_str, names);
            }
        }
    }

    fn collect_types(&mut self, module: &Module) {
        for item in &module.body {
            self.collector.collect_item(item);
        }
    }

    fn emit_named_types(&mut self) {
        // Emit struct types - each struct only once
        for (name, info) in self.collector.structs() {
            if self.emission_tracker.struct_emitted(name) {
                continue;
            }
            self.emission_tracker.mark_struct_emitted(name);
            self.emitter.emit_struct(&info.rust_name, &info.fields);
        }

        // Emit enum types - each enum only once
        for (name, ed) in self.collector.enums() {
            if self.emission_tracker.enum_emitted(name) {
                continue;
            }
            self.emission_tracker.mark_enum_emitted(name);
            self.emitter.emit_enum(ed);
        }
    }

    fn emit_anonymous_structs(&mut self) {
        for (name, fields) in self.collector.take_pending_structs() {
            if !self.emission_tracker.struct_emitted(&name) {
                self.emission_tracker.mark_struct_emitted(&name);
                self.emitter.emit_struct(&name, &fields);
            }
        }
    }

    #[allow(clippy::collapsible_match)]
    fn emit_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::Fn(fn_decl))) => self.emit_function(fn_decl),
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl: Decl::Fn(fn_decl), .. })) => {
                self.emit_function(fn_decl);
            }
            _ => {}
        }
    }

    fn emit_functions(&mut self, module: &Module) {
        for item in &module.body {
            self.emit_item(item);
        }
    }

    fn emit_function(&mut self, fn_decl: &FnDecl) {
        let rust_name = to_snake_case(fn_decl.ident.sym.as_ref());
        let params = self.extract_function_params(fn_decl);
        let return_type = self.extract_return_type(fn_decl);
        let is_async = fn_decl.function.is_async;
        let body = fn_decl.function.body.as_ref().map(|block| Stmt::Block(block.clone()));

        self.emitter.set_expected_return(Some(return_type.to_string()));
        self.emitter.emit_function_with_body(&rust_name, &params, &return_type, is_async, body);
        self.emitter.set_expected_return(None);
    }

    /// Extract function parameters with types.
    fn extract_function_params(&mut self, fn_decl: &FnDecl) -> Vec<(String, RustType)> {
        fn_decl.function.params.iter()
            .filter_map(|p| {
                if let swc_ecma_ast::Pat::Ident(ident) = &p.pat {
                    let ty = ident.type_ann.as_ref()
                        .map_or(RustType::Unknown, |ann| self.collector.resolver_mut().resolve(&ann.type_ann));
                    Some((ident.id.sym.to_string(), ty))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Extract the return type of a function.
    fn extract_return_type(&mut self, fn_decl: &FnDecl) -> RustType {
        fn_decl.function.return_type.as_ref()
            .map_or(RustType::Unit, |ann| self.collector.resolver_mut().resolve(&ann.type_ann))
    }

    /// Get the generated output.
    #[must_use]
    pub fn output(&self) -> &str {
        self.emitter.output()
    }

    /// Consume walker and return output.
    #[must_use]
    pub fn into_output(self) -> String {
        self.emitter.into_output()
    }

    /// Get native imports.
    #[must_use]
    pub fn native_imports(&self) -> &HashSet<String> {
        &self.native_imports
    }

    /// Consume walker and return native imports.
    #[must_use]
    pub fn into_native_imports(self) -> HashSet<String> {
        self.native_imports
    }
}

impl Default for AstWalker {
    fn default() -> Self {
        Self::new()
    }
}
