//! # AST Walker
//!
//! Walks SWC AST and emits Rust code.

use super::types::EnumDefinition;
use super::{CodeEmitter, RustType, TypeResolver};
use crate::utils::{to_pascal_case, to_snake_case};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast::{Decl, ExportDecl, FnDecl, Module, ModuleDecl, ModuleItem, Stmt, TsType};

/// Raw field for deferred type resolution.
#[allow(dead_code)]
type RawField = (String, TsType);

/// Information about a struct type.
#[derive(Debug, Clone)]
struct StructInfo {
    /// Rust struct name
    rust_name: String,
    /// Fields with types
    fields: Vec<(String, RustType)>,
}

/// Tracks which types have been emitted to avoid duplicates.
#[derive(Default)]
struct EmissionTracker {
    emitted_structs: HashSet<String>,
    emitted_enums: HashSet<String>,
}

impl EmissionTracker {
    fn mark_struct_emitted(&mut self, name: &str) {
        self.emitted_structs.insert(name.to_string());
    }

    fn mark_enum_emitted(&mut self, name: &str) {
        self.emitted_enums.insert(name.to_string());
    }

    #[must_use]
    fn struct_emitted(&self, name: &str) -> bool {
        self.emitted_structs.contains(name)
    }

    #[must_use]
    fn enum_emitted(&self, name: &str) -> bool {
        self.emitted_enums.contains(name)
    }
}

/// Walks the AST and emits Rust code.
pub struct AstWalker {
    /// Type fields to emit (TS name → fields)
    type_fields: HashMap<String, Vec<RawField>>,
    /// Enums to emit
    enums: HashMap<String, EnumDefinition>,
    /// Struct type info (TS name → struct info)
    structs: HashMap<String, StructInfo>,
    /// Code emitter
    emitter: CodeEmitter,
    /// Type resolver
    resolver: TypeResolver,
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
            type_fields: HashMap::new(),
            enums: HashMap::new(),
            structs: HashMap::new(),
            emitter: CodeEmitter::new(),
            resolver: TypeResolver::new(),
            module_name: String::new(),
            imports: HashMap::new(),
            native_imports: HashSet::new(),
            emission_tracker: EmissionTracker::default(),
        }
    }

    /// Walk a module and emit Rust code.
    pub fn walk_module(&mut self, module: &Module) {
        self.collect_imports(module);

        // Second pass: collect all type definitions and enums
        for item in &module.body {
            self.collect_item(item);
        }
        self.emit_named_types();

        // Third pass: emit functions
        for item in &module.body {
            self.emit_item(item);
        }
        self.emit_anonymous_structs();
    }

    fn collect_imports(&mut self, module: &Module) {
        for item in &module.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                let path_str = format!("{:?}", import.src.value);

                let is_native = path_str.starts_with("\"native:");
                if is_native {
                    let module_name = path_str
                        .trim_start_matches("\"native:")
                        .trim_end_matches('"');
                    self.native_imports.insert(module_name.to_string());
                }

                let names: Vec<String> = import
                    .specifiers
                    .iter()
                    .map(|spec| match spec {
                        swc_ecma_ast::ImportSpecifier::Named(named) => {
                            to_snake_case(named.local.as_ref())
                        }
                        swc_ecma_ast::ImportSpecifier::Default(_) => "default".to_string(),
                        swc_ecma_ast::ImportSpecifier::Namespace(ns) => {
                            format!("*{}", to_snake_case(ns.local.as_ref()))
                        }
                    })
                    .collect();
                self.imports.insert(path_str, names);
            }
        }
    }

    fn emit_named_types(&mut self) {
        // Emit struct types - each struct only once
        let struct_names: Vec<String> = self.structs.keys().cloned().collect();
        for name in struct_names {
            if self.emission_tracker.struct_emitted(&name) {
                continue;
            }
            self.emission_tracker.mark_struct_emitted(&name);

            if let Some(info) = self.structs.get(&name) {
                self.emitter.emit_struct(&info.rust_name, &info.fields);
            }
        }

        // Emit enum types - each enum only once
        let enum_names: Vec<String> = self.enums.keys().cloned().collect();
        for name in enum_names {
            if self.emission_tracker.enum_emitted(&name) {
                continue;
            }
            self.emission_tracker.mark_enum_emitted(&name);

            if let Some(ed) = self.enums.get(&name) {
                self.emitter.emit_enum(ed);
            }
        }
    }

    fn emit_anonymous_structs(&mut self) {
        let anon_structs = self.resolver.take_pending_structs();
        for (name, fields) in anon_structs {
            // Check if already emitted (in case of naming collision)
            if !self.emission_tracker.struct_emitted(&name) {
                self.emission_tracker.mark_struct_emitted(&name);
                self.emitter.emit_struct(&name, &fields);
            }
        }
    }

    #[allow(clippy::single_match)]
    fn collect_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::TsInterface(d))) => {
                self.collect_interface(d.id.sym.as_ref(), &d.body);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsEnum(d))) => {
                self.collect_enum(d.id.sym.as_ref(), d);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsTypeAlias(d))) => {
                self.collect_type_alias(d.id.sym.as_ref(), &d.type_ann);
            }
            ModuleItem::Stmt(Stmt::Decl(Decl::TsModule(d))) => {
                self.collect_ts_module(d);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: Decl::TsInterface(d),
                ..
            })) => {
                self.collect_interface(d.id.sym.as_ref(), &d.body);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: Decl::TsEnum(d),
                ..
            })) => {
                self.collect_enum(d.id.sym.as_ref(), d);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: Decl::TsTypeAlias(d),
                ..
            })) => {
                self.collect_type_alias(d.id.sym.as_ref(), &d.type_ann);
            }
            _ => {}
        }
    }

    fn collect_interface(&mut self, name: &str, body: &swc_ecma_ast::TsInterfaceBody) {
        let mut fields: Vec<(String, TsType)> = Vec::new();
        for member in &body.body {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = match prop.key.as_ref() {
                    swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) => {
                        format!("{:?}", s.value)
                    }
                    _ => "_unknown".to_string(),
                };

                if let Some(type_ann) = &prop.type_ann {
                    fields.push((field_name, (*type_ann.type_ann).clone()));
                }
            }
        }

        // Convert to RustType and store in structs map
        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
            .collect();

        self.structs.insert(
            name.to_string(),
            StructInfo {
                rust_name,
                fields: resolved_fields,
            },
        );
    }

    fn collect_enum(&mut self, name: &str, decl: &swc_ecma_ast::TsEnumDecl) {
        let variants: Vec<super::types::EnumVariant> = decl
            .members
            .iter()
            .map(|member| {
                let variant_name = match &member.id {
                    swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::TsEnumMemberId::Str(s) => {
                        format!("{:?}", s.value)
                    }
                };
                super::types::EnumVariant {
                    name: variant_name,
                    fields: Vec::new(),
                }
            })
            .collect();

        self.enums.insert(
            name.to_string(),
            EnumDefinition {
                name: name.to_string(),
                variants,
            },
        );
    }

    fn collect_type_alias(&mut self, name: &str, type_ann: &TsType) {
        // Check if this is a struct type (object literal)
        if let TsType::TsTypeLit(lit) = type_ann {
            let mut fields: Vec<(String, TsType)> = Vec::new();
            for member in &lit.members {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                    let field_name = match prop.key.as_ref() {
                        swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                        _ => continue,
                    };

                    if let Some(type_ann) = &prop.type_ann {
                        fields.push((field_name, (*type_ann.type_ann).clone()));
                    }
                }
            }

            // Store in structs map with resolved types
            let rust_name = to_pascal_case(name);
            let resolved_fields: Vec<(String, RustType)> = fields
                .iter()
                .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
                .collect();

            self.structs.insert(
                name.to_string(),
                StructInfo {
                    rust_name,
                    fields: resolved_fields,
                },
            );
            return;
        }

        // Check if this is a union type (tagged union)
        if let TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann
        {
            if u.types.iter().all(|t| self.is_tagged_variant(t)) {
                self.collect_tagged_union(name, &u.types);
                return;
            }
        }

        // For other type aliases, store the type reference
        self.type_fields.insert(
            name.to_string(),
            vec![("_type".to_string(), type_ann.clone())],
        );
    }

    /// Check if a type is a tagged variant.
    fn is_tagged_variant(&self, ts_type: &TsType) -> bool {
        if let TsType::TsTypeLit(lit) = ts_type {
            lit.members.iter().any(|m| {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = m {
                    if let Some(type_ann) = &prop.type_ann {
                        if let TsType::TsKeywordType(k) = type_ann.type_ann.as_ref() {
                            return k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword;
                        }
                    }
                }
                false
            })
        } else {
            false
        }
    }

    /// Collect a tagged union (enum).
    fn collect_tagged_union(&mut self, name: &str, types: &[Box<TsType>]) {
        let mut variants: Vec<super::types::EnumVariant> = Vec::new();

        for ts_type in types {
            if let TsType::TsTypeLit(lit) = ts_type.as_ref() {
                let mut tag = String::new();
                let mut fields: Vec<(String, RustType)> = Vec::new();

                for member in &lit.members {
                    if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                        let field_name = match prop.key.as_ref() {
                            swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                            _ => continue,
                        };

                        if let Some(type_ann) = &prop.type_ann {
                            let ty = self.resolver.resolve(&type_ann.type_ann);
                            if field_name == "tag" {
                                if let TsType::TsKeywordType(k) = type_ann.type_ann.as_ref() {
                                    if k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword {
                                        tag = format!(
                                            "{}{}",
                                            name.chars().next().unwrap().to_uppercase(),
                                            &name[1..]
                                        );
                                    }
                                }
                            } else {
                                fields.push((field_name, ty));
                            }
                        }
                    }
                }

                if !tag.is_empty() {
                    variants.push(super::types::EnumVariant { name: tag, fields });
                }
            }
        }

        if !variants.is_empty() {
            self.enums.insert(
                name.to_string(),
                EnumDefinition {
                    name: name.to_string(),
                    variants,
                },
            );
        }
    }

    fn collect_ts_module(&mut self, decl: &swc_ecma_ast::TsModuleDecl) {
        if let Some(swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block)) = decl.body.as_ref() {
            for item in &block.body {
                self.collect_item(item);
            }
        }
    }

    #[allow(clippy::collapsible_match)]
    fn emit_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(Stmt::Decl(Decl::Fn(fn_decl))) => {
                self.emit_function(fn_decl);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: Decl::Fn(fn_decl),
                ..
            })) => {
                self.emit_function(fn_decl);
            }
            _ => {}
        }
    }

    fn emit_function(&mut self, fn_decl: &FnDecl) {
        let rust_name = to_snake_case(fn_decl.ident.sym.as_ref());

        let params: Vec<(String, RustType)> = fn_decl
            .function
            .params
            .iter()
            .filter_map(|p| {
                if let swc_ecma_ast::Pat::Ident(ident) = &p.pat {
                    let ty = ident.type_ann.as_ref().map_or(RustType::Unknown, |ann| {
                        self.resolver.resolve(&ann.type_ann)
                    });
                    Some((ident.id.sym.to_string(), ty))
                } else {
                    None
                }
            })
            .collect();

        let return_type = fn_decl
            .function
            .return_type
            .as_ref()
            .map_or(RustType::Unit, |ann| self.resolver.resolve(&ann.type_ann));

        let is_async = fn_decl.function.is_async;

        let body = fn_decl
            .function
            .body
            .as_ref()
            .map(|block| Stmt::Block(block.clone()));

        self.emitter
            .set_expected_return(Some(return_type.to_string()));

        self.emitter
            .emit_function_with_body(&rust_name, &params, &return_type, is_async, body);

        self.emitter.set_expected_return(None);
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
