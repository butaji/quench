//! # Type Collector
//!
//! Collects type definitions from SWC AST.

use crate::codegen::emitter::types::{EnumDefinition, StructFields};
use crate::codegen::emitter::{RustType, TypeResolver};
use std::collections::{HashMap, HashSet};

mod type_collector_enums;
mod type_collector_result;
mod type_collector_structs;
mod type_collector_tagged;

use type_collector_enums::TypeCollectorEnums;
use type_collector_result::TypeCollectorResult;
use type_collector_structs::TypeCollectorStructs;
use type_collector_tagged::TypeCollectorTagged;



/// Information about a struct type.
#[derive(Debug, Clone)]
pub(super) struct StructInfo {
    pub(super) rust_name: String,
    pub(super) fields: Vec<(String, RustType)>,
}

/// Tracks which types have been emitted to avoid duplicates.
#[derive(Default)]
pub(super) struct EmissionTracker {
    emitted_structs: HashSet<String>,
    emitted_enums: HashSet<String>,
}

impl EmissionTracker {
    pub(super) fn mark_struct_emitted(&mut self, name: &str) {
        self.emitted_structs.insert(name.to_string());
    }

    pub(super) fn mark_enum_emitted(&mut self, name: &str) {
        self.emitted_enums.insert(name.to_string());
    }

    #[must_use]
    pub(super) fn struct_emitted(&self, name: &str) -> bool {
        self.emitted_structs.contains(name)
    }

    #[must_use]
    pub(super) fn enum_emitted(&self, name: &str) -> bool {
        self.emitted_enums.contains(name)
    }
}

/// Collects types from AST.
pub(super) struct TypeCollector {
    enums: HashMap<String, EnumDefinition>,
    structs: HashMap<String, StructInfo>,
    resolver: TypeResolver,
    structs_impl: TypeCollectorStructs,
    enums_impl: TypeCollectorEnums,
    result_impl: TypeCollectorResult,
    tagged_impl: TypeCollectorTagged,
}

impl TypeCollector {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            enums: HashMap::new(),
            structs: HashMap::new(),
            resolver: TypeResolver::new(),
            structs_impl: TypeCollectorStructs::new(),
            enums_impl: TypeCollectorEnums::new(),
            result_impl: TypeCollectorResult::new(),
            tagged_impl: TypeCollectorTagged::new(),
        }
    }

    pub(super) fn collect_interface(
        &mut self,
        name: &str,
        body: &swc_ecma_ast::TsInterfaceBody,
    ) {
        self.structs_impl.collect_interface(
            name,
            body,
            &mut self.structs,
            &mut self.resolver,
        );
    }

    pub(super) fn collect_enum(
        &mut self,
        name: &str,
        decl: &swc_ecma_ast::TsEnumDecl,
    ) {
        self.enums_impl
            .collect_enum(name, decl, &mut self.enums);
    }

    pub(super) fn collect_type_alias(
        &mut self,
        name: &str,
        type_ann: &swc_ecma_ast::TsType,
    ) {
        // Check for Result pattern first (more specific than tagged union)
        if self.result_impl.is_result_pattern(type_ann) {
            self.result_impl
                .collect_result_type(name, type_ann, &mut self.resolver);
            return;
        }

        // Check for tagged union
        if let Some(types) = self.tagged_impl.try_extract_tagged_union(type_ann) {
            self.tagged_impl.collect_tagged_union(
                name,
                &types,
                &mut self.enums,
                &mut self.resolver,
            );
            return;
        }

        // Check for struct literal
        if let swc_ecma_ast::TsType::TsTypeLit(lit) = type_ann {
            self.structs_impl
                .collect_struct_from_literal(name, lit, &mut self.structs, &mut self.resolver);
        }

        // Unhandled type alias - not a struct, enum, result, or tagged union
    }

    pub(super) fn collect_ts_module(&mut self, decl: &swc_ecma_ast::TsModuleDecl) {
        if let Some(swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block)) = decl.body.as_ref()
        {
            for item in &block.body {
                self.collect_item(item);
            }
        }
    }

    pub(super) fn collect_item(&mut self, item: &swc_ecma_ast::ModuleItem) {
        match item {
            swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(d)) => {
                self.collect_decl(d);
            }
            swc_ecma_ast::ModuleItem::ModuleDecl(swc_ecma_ast::ModuleDecl::ExportDecl(e)) => {
                self.collect_export_decl(&e.decl);
            }
            _ => {}
        }
    }

    fn collect_decl(&mut self, decl: &swc_ecma_ast::Decl) {
        match decl {
            swc_ecma_ast::Decl::TsInterface(d) => {
                self.collect_interface(d.id.sym.as_ref(), &d.body);
            }
            swc_ecma_ast::Decl::TsEnum(d) => {
                self.collect_enum(d.id.sym.as_ref(), d);
            }
            swc_ecma_ast::Decl::TsTypeAlias(d) => {
                self.collect_type_alias(d.id.sym.as_ref(), &d.type_ann);
            }
            swc_ecma_ast::Decl::TsModule(d) => {
                self.collect_ts_module(d);
            }
            _ => {}
        }
    }

    fn collect_export_decl(&mut self, decl: &swc_ecma_ast::Decl) {
        match decl {
            swc_ecma_ast::Decl::TsInterface(d) => {
                self.collect_interface(d.id.sym.as_ref(), &d.body);
            }
            swc_ecma_ast::Decl::TsEnum(d) => {
                self.collect_enum(d.id.sym.as_ref(), d);
            }
            swc_ecma_ast::Decl::TsTypeAlias(d) => {
                self.collect_type_alias(d.id.sym.as_ref(), &d.type_ann);
            }
            _ => {}
        }
    }

    #[must_use]
    pub(super) fn structs(&self) -> &HashMap<String, StructInfo> {
        &self.structs
    }

    #[must_use]
    pub(super) fn enums(&self) -> &HashMap<String, EnumDefinition> {
        &self.enums
    }

    pub(super) fn take_pending_structs(&mut self) -> Vec<(String, StructFields)> {
        self.resolver.take_pending_structs()
    }

    pub(super) fn resolver_mut(&mut self) -> &mut TypeResolver {
        &mut self.resolver
    }
}

impl Default for TypeCollector {
    fn default() -> Self {
        Self::new()
    }
}
