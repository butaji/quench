//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.

use super::type_collector::{EmissionTracker, TypeCollector};
use super::{CodeEmitter, RustType};
use crate::utils::to_snake_case;
use std::collections::{HashMap, HashSet};
use swc_ecma_ast::{Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem, Stmt};

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    emitter: CodeEmitter,
    collector: TypeCollector,
    #[allow(dead_code)]
    module_name: String,
    imports: HashMap<String, Vec<String>>,
    native_imports: HashSet<String>,
    emission_tracker: EmissionTracker,
}

impl AstWalker {
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

    pub fn walk_module(&mut self, module: &Module) {
        self.collect_imports(module);
        self.collect_types(module);
        self.emit_imports();
        self.emit_named_types();
        self.emit_functions(module);
        self.emit_anonymous_structs();
    }

    fn collect_imports(&mut self, module: &Module) {
        for item in &module.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                self.process_import(import);
            }
        }
    }

    fn process_import(&mut self, import: &swc_ecma_ast::ImportDecl) {
        let path_str = format!("{:?}", import.src.value);

        if path_str.starts_with("\"native:") {
            let module_name = path_str.trim_start_matches("\"native:").trim_end_matches('"');
            self.native_imports.insert(module_name.to_string());
        }

        let names: Vec<String> = import.specifiers.iter().map(|spec| self.extract_spec_name(spec)).collect();
        self.imports.insert(path_str, names);
    }

    fn extract_spec_name(&self, spec: &swc_ecma_ast::ImportSpecifier) -> String {
        match spec {
            swc_ecma_ast::ImportSpecifier::Named(named) => named.local.as_ref().to_string(),
            swc_ecma_ast::ImportSpecifier::Default(_) => "default".to_string(),
            swc_ecma_ast::ImportSpecifier::Namespace(ns) => {
                format!("*{}", to_snake_case(ns.local.as_ref()))
            }
        }
    }

    fn collect_types(&mut self, module: &Module) {
        for item in &module.body {
            self.collector.collect_item(item);
        }
    }

    fn emit_imports(&mut self) {
        let protocol_types = ["Task", "AppState", "Filter"];
        for (path, names) in &self.imports {
            let clean_path = path.trim_matches('"');
            if clean_path.starts_with("native:") {
                continue;
            }

            // Filter out types that are in protocol (already imported via protocol)
            let filtered_names: Vec<&String> = names
                .iter()
                .filter(|n| !protocol_types.contains(&n.as_str()))
                .collect();

            if filtered_names.is_empty() {
                continue;
            }

            let rust_path = self.convert_import_path(clean_path);
            let names_str = filtered_names
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.emitter.push_line(&format!("use {rust_path}::{{{names_str}}};"));
        }
    }

    fn convert_import_path(&self, path: &str) -> String {
        if path.starts_with("./") {
            let path = path.trim_start_matches("./");
            let path = path.replace(".r.ts", "").replace(".r.tsx", "");
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            let rust_parts: Vec<String> = parts.iter().map(|p| to_snake_case(p)).collect();
            return format!("crate::generated::{}", rust_parts.join("::"));
        }
        // Handle ../ paths (relative to current directory)
        if path.starts_with("../") {
            let path = path.trim_start_matches("../");
            let path = path.replace(".r.ts", "").replace(".r.tsx", "");
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            let rust_parts: Vec<String> = parts.iter().map(|p| to_snake_case(p)).collect();
            return format!("crate::generated::{}", rust_parts.join("::"));
        }
        path.to_string()
    }

    fn emit_named_types(&mut self) {
        for (name, info) in self.collector.structs() {
            if self.emission_tracker.struct_emitted(name) {
                continue;
            }
            self.emission_tracker.mark_struct_emitted(name);
            self.emitter.emit_struct(&info.rust_name, &info.fields);
        }

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

    fn emit_functions(&mut self, module: &Module) {
        for item in &module.body {
            self.emit_item(item);
        }
    }

    fn emit_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::Fn(fn_decl))) => self.emit_function(fn_decl),
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl: Decl::Fn(fn_decl), .. })) => {
                self.emit_function(fn_decl);
            }
            _ => {}
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

    fn extract_return_type(&mut self, fn_decl: &FnDecl) -> RustType {
        fn_decl.function.return_type.as_ref()
            .map_or(RustType::Unit, |ann| self.collector.resolver_mut().resolve(&ann.type_ann))
    }

    #[must_use]
    pub fn output(&self) -> &str {
        self.emitter.output()
    }

    #[must_use]
    pub fn into_output(self) -> String {
        self.emitter.into_output()
    }

    #[must_use]
    pub fn native_imports(&self) -> &HashSet<String> {
        &self.native_imports
    }

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
