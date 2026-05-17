//! # Type Collector
//!
//! Collects type definitions from SWC AST.

use super::types::{EnumDefinition, StructFields};
use super::{RustType, TypeResolver};
use crate::utils::to_pascal_case;
use std::collections::{HashMap, HashSet};
use swc_ecma_ast::TsType;

/// Information about a struct type.
#[derive(Debug, Clone)]
pub(super) struct StructInfo {
    /// Rust struct name
    pub(super) rust_name: String,
    /// Fields with types
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
    /// Type fields to emit (TS name → fields)
    type_fields: HashMap<String, Vec<(String, TsType)>>,
    /// Enums to emit
    enums: HashMap<String, EnumDefinition>,
    /// Struct type info (TS name → struct info)
    structs: HashMap<String, StructInfo>,
    /// Type resolver
    resolver: TypeResolver,
}

impl TypeCollector {
    /// Create a new collector.
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            type_fields: HashMap::new(),
            enums: HashMap::new(),
            structs: HashMap::new(),
            resolver: TypeResolver::new(),
        }
    }

    /// Collect interface (struct) definition.
    pub(super) fn collect_interface(&mut self, name: &str, body: &swc_ecma_ast::TsInterfaceBody) {
        let fields: Vec<(String, TsType)> = body.body.iter()
            .filter_map(|member| {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                    let field_name = match prop.key.as_ref() {
                        swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                        swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) => {
                            format!("{:?}", s.value)
                        }
                        _ => "_unknown".to_string(),
                    };
                    prop.type_ann.as_ref()
                        .map(|ann| (field_name, (*ann.type_ann).clone()))
                } else {
                    None
                }
            })
            .collect();

        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
            .collect();

        self.structs.insert(name.to_string(), StructInfo { rust_name, fields: resolved_fields });
    }

    /// Collect enum definition.
    pub(super) fn collect_enum(&mut self, name: &str, decl: &swc_ecma_ast::TsEnumDecl) {
        let variants: Vec<super::types::EnumVariant> = decl.members.iter()
            .map(|member| {
                let variant_name = match &member.id {
                    swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::TsEnumMemberId::Str(s) => format!("{:?}", s.value),
                };
                super::types::EnumVariant { name: variant_name, fields: Vec::new() }
            })
            .collect();

        self.enums.insert(name.to_string(), EnumDefinition { name: name.to_string(), variants });
    }

    /// Collect type alias.
    pub(super) fn collect_type_alias(&mut self, name: &str, type_ann: &TsType) {
        // Check if this is a struct type (object literal)
        if let TsType::TsTypeLit(lit) = type_ann {
            self.collect_struct_from_literal(name, lit);
            return;
        }

        // Check if this is a union type (tagged union)
        if let TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann {
            if u.types.iter().all(|t| self.is_tagged_variant(t)) {
                self.collect_tagged_union(name, &u.types);
                return;
            }
        }

        // For other type aliases, store the type reference
        self.type_fields.insert(name.to_string(), vec![("_type".to_string(), type_ann.clone())]);
    }

    /// Collect struct from type literal.
    fn collect_struct_from_literal(&mut self, name: &str, lit: &swc_ecma_ast::TsTypeLit) {
        let fields: Vec<(String, TsType)> = lit.members.iter()
            .filter_map(|member| {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                    let field_name = match prop.key.as_ref() {
                        swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
                        _ => return None,
                    };
                    prop.type_ann.as_ref()
                        .map(|ann| (field_name, (*ann.type_ann).clone()))
                } else {
                    None
                }
            })
            .collect();

        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
            .collect();

        self.structs.insert(name.to_string(), StructInfo { rust_name, fields: resolved_fields });
    }

    /// Collect namespace/module.
    pub(super) fn collect_ts_module(&mut self, decl: &swc_ecma_ast::TsModuleDecl) {
        if let Some(swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block)) = decl.body.as_ref() {
            for item in &block.body {
                self.collect_item(item);
            }
        }
    }

    /// Collect an item (interface, enum, type alias).
    #[allow(clippy::single_match)]
    pub(super) fn collect_item(&mut self, item: &swc_ecma_ast::ModuleItem) {
        match item {
            swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::TsInterface(d))) => {
                self.collect_interface(d.id.sym.as_ref(), &d.body);
            }
            swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::TsEnum(d))) => {
                self.collect_enum(d.id.sym.as_ref(), d);
            }
            swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::TsTypeAlias(d))) => {
                self.collect_type_alias(d.id.sym.as_ref(), &d.type_ann);
            }
            swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(swc_ecma_ast::Decl::TsModule(d))) => {
                self.collect_ts_module(d);
            }
            swc_ecma_ast::ModuleItem::ModuleDecl(swc_ecma_ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
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
            _ => {}
        }
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

    /// Check if a type is a string keyword.
    fn is_string_keyword(&self, ty: &TsType) -> bool {
        if let TsType::TsKeywordType(k) = ty {
            k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword
        } else {
            false
        }
    }

    /// Collect a tagged union (enum).
    pub(super) fn collect_tagged_union(&mut self, name: &str, types: &[Box<TsType>]) {
        let variants: Vec<super::types::EnumVariant> = types
            .iter()
            .filter_map(|ts_type| self.extract_variant(ts_type.as_ref(), name))
            .collect();

        if !variants.is_empty() {
            self.enums.insert(name.to_string(), EnumDefinition { name: name.to_string(), variants });
        }
    }

    /// Extract a single enum variant from a type literal.
    fn extract_variant(&mut self, ts_type: &TsType, enum_name: &str) -> Option<super::types::EnumVariant> {
        let TsType::TsTypeLit(lit) = ts_type else { return None };
        let (tag, fields) = self.extract_tag_and_fields(lit, enum_name)?;
        Some(super::types::EnumVariant { name: tag, fields })
    }

    /// Extract tag name and fields from a type literal.
    fn extract_tag_and_fields(&mut self, lit: &swc_ecma_ast::TsTypeLit, enum_name: &str) -> Option<(String, Vec<(String, RustType)>)> {
        let mut tag = String::new();
        let mut fields = Vec::new();

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some((field_name, ty)) = self.extract_property(prop, enum_name, &mut tag) {
                    fields.push((field_name, ty));
                }
            }
        }

        if tag.is_empty() { None } else { Some((tag, fields)) }
    }

    /// Extract a property as either tag or field.
    fn extract_property(&mut self, prop: &swc_ecma_ast::TsPropertySignature, enum_name: &str, tag_buffer: &mut String) -> Option<(String, RustType)> {
        let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() else { return None };
        let field_name = ident.sym.to_string();
        let type_ann = prop.type_ann.as_ref()?;
        let ty = self.resolver.resolve(&type_ann.type_ann);

        if field_name == "tag" && self.is_string_keyword(&type_ann.type_ann) {
            *tag_buffer = format!("{}{}", enum_name.chars().next()?.to_uppercase(), &enum_name[1..]);
            None
        } else {
            Some((field_name, ty))
        }
    }

    /// Get struct info.
    #[must_use]
    pub(super) fn structs(&self) -> &HashMap<String, StructInfo> {
        &self.structs
    }

    /// Get enums.
    #[must_use]
    pub(super) fn enums(&self) -> &HashMap<String, EnumDefinition> {
        &self.enums
    }

    /// Take pending structs from resolver.
    pub(super) fn take_pending_structs(&mut self) -> Vec<(String, StructFields)> {
        self.resolver.take_pending_structs()
    }

    /// Get mutable reference to resolver.
    pub(super) fn resolver_mut(&mut self) -> &mut TypeResolver {
        &mut self.resolver
    }
}

impl Default for TypeCollector {
    fn default() -> Self {
        Self::new()
    }
}
