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
    type_fields: HashMap<String, Vec<(String, TsType)>>,
    enums: HashMap<String, EnumDefinition>,
    structs: HashMap<String, StructInfo>,
    resolver: TypeResolver,
}

impl TypeCollector {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            type_fields: HashMap::new(),
            enums: HashMap::new(),
            structs: HashMap::new(),
            resolver: TypeResolver::new(),
        }
    }

    pub(super) fn collect_interface(&mut self, name: &str, body: &swc_ecma_ast::TsInterfaceBody) {
        let fields = self.extract_interface_fields(body);
        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
            .collect();
        self.structs.insert(
            name.to_string(),
            StructInfo { rust_name, fields: resolved_fields },
        );
    }

    fn extract_interface_fields(&self, body: &swc_ecma_ast::TsInterfaceBody) -> Vec<(String, TsType)> {
        body.body.iter().filter_map(|member| {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = self.extract_field_name(&prop.key);
                prop.type_ann.as_ref().map(|ann| (field_name, (*ann.type_ann).clone()))
            } else {
                None
            }
        }).collect()
    }

    fn extract_field_name(&self, key: &swc_ecma_ast::Expr) -> String {
        match key {
            swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) => format!("{:?}", s.value),
            _ => "_unknown".to_string(),
        }
    }

    pub(super) fn collect_enum(&mut self, name: &str, decl: &swc_ecma_ast::TsEnumDecl) {
        let variants: Vec<super::types::EnumVariant> = decl.members.iter()
            .map(|m| super::types::EnumVariant {
                name: self.extract_enum_member_name(m),
                fields: Vec::new(),
            })
            .collect();
        self.enums.insert(name.to_string(), EnumDefinition { name: name.to_string(), variants });
    }

    fn extract_enum_member_name(&self, member: &swc_ecma_ast::TsEnumMember) -> String {
        match &member.id {
            swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::TsEnumMemberId::Str(s) => format!("{:?}", s.value),
        }
    }

    pub(super) fn collect_type_alias(&mut self, name: &str, type_ann: &TsType) {
        if let TsType::TsTypeLit(lit) = type_ann {
            self.collect_struct_from_literal(name, lit);
            return;
        }

        // Check for Result pattern first (more specific than tagged union)
        if let TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann {
            if self.is_result_pattern(u) {
                self.collect_result_type(name, u);
                return;
            }
        }

        if let TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann {
            if u.types.iter().all(|t| self.is_tagged_variant(t)) {
                self.collect_tagged_union(name, &u.types);
                return;
            }
        }

        self.type_fields.insert(name.to_string(), vec![("_type".to_string(), type_ann.clone())]);
    }

    fn collect_struct_from_literal(&mut self, name: &str, lit: &swc_ecma_ast::TsTypeLit) {
        let fields = self.extract_literal_fields(lit);
        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), self.resolver.resolve(t)))
            .collect();
        self.structs.insert(name.to_string(), StructInfo { rust_name, fields: resolved_fields });
    }

    fn extract_literal_fields(&self, lit: &swc_ecma_ast::TsTypeLit) -> Vec<(String, TsType)> {
        lit.members.iter().filter_map(|member| {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    let field_name = ident.sym.to_string();
                    return prop.type_ann.as_ref().map(|ann| (field_name, (*ann.type_ann).clone()));
                }
            }
            None
        }).collect()
    }

    pub(super) fn collect_ts_module(&mut self, decl: &swc_ecma_ast::TsModuleDecl) {
        if let Some(swc_ecma_ast::TsNamespaceBody::TsModuleBlock(block)) = decl.body.as_ref() {
            for item in &block.body {
                self.collect_item(item);
            }
        }
    }

    pub(super) fn collect_item(&mut self, item: &swc_ecma_ast::ModuleItem) {
        match item {
            swc_ecma_ast::ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(d)) => self.collect_decl(d),
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

    /// Check if this is a Result pattern: {ok: true, value: T} | {ok: false, error: E}
    fn is_result_pattern(&self, union: &swc_ecma_ast::TsUnionType) -> bool {
        if union.types.len() != 2 {
            return false;
        }

        let ok_variant = union.types.iter().find(|t| self.is_ok_variant(t));
        let err_variant = union.types.iter().find(|t| self.is_error_variant(t));

        ok_variant.is_some() && err_variant.is_some()
    }

    fn is_ok_variant(&self, ts_type: &TsType) -> bool {
        let TsType::TsTypeLit(lit) = ts_type else { return false };
        let mut has_ok_true = false;
        let mut has_value = false;

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some(type_ann) = &prop.type_ann {
                    let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                        ident.sym.as_ref()
                    } else {
                        continue;
                    };

                    if field_name == "ok" && self.is_true_literal(&type_ann.type_ann) {
                        has_ok_true = true;
                    }
                    if field_name == "value" {
                        has_value = true;
                    }
                }
            }
        }

        has_ok_true && has_value
    }

    fn is_error_variant(&self, ts_type: &TsType) -> bool {
        let TsType::TsTypeLit(lit) = ts_type else { return false };
        let mut has_ok_false = false;
        let mut has_error = false;

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some(type_ann) = &prop.type_ann {
                    let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                        ident.sym.as_ref()
                    } else {
                        continue;
                    };

                    if field_name == "ok" && self.is_false_literal(&type_ann.type_ann) {
                        has_ok_false = true;
                    }
                    if field_name == "error" {
                        has_error = true;
                    }
                }
            }
        }

        has_ok_false && has_error
    }

    fn is_true_literal(&self, ty: &TsType) -> bool {
        if let TsType::TsLitType(lit) = ty {
            return matches!(lit.lit, swc_ecma_ast::TsLit::Bool(swc_ecma_ast::Bool { value: true, .. }));
        }
        false
    }

    fn is_false_literal(&self, ty: &TsType) -> bool {
        if let TsType::TsLitType(lit) = ty {
            return matches!(lit.lit, swc_ecma_ast::TsLit::Bool(swc_ecma_ast::Bool { value: false, .. }));
        }
        false
    }

    /// Collect a Result type and register it for emission
    fn collect_result_type(&mut self, name: &str, union: &swc_ecma_ast::TsUnionType) {
        // Extract value type from the ok variant
        let value_type = self.extract_result_value_type(union);

        // Store the Result type info for later use during type resolution
        // We use the resolver to handle this
        self.resolver.register_result_type(name, value_type);
    }

    fn extract_result_value_type(&mut self, union: &swc_ecma_ast::TsUnionType) -> String {
        for ts_type in &union.types {
            if self.is_ok_variant(ts_type) {
                if let TsType::TsTypeLit(lit) = ts_type.as_ref() {
                    return self.extract_field_type(lit, "value");
                }
            }
        }
        String::from("()")
    }

    #[allow(dead_code)]
    fn extract_result_error_type(&mut self, union: &swc_ecma_ast::TsUnionType) -> String {
        for ts_type in &union.types {
            if self.is_error_variant(ts_type) {
                if let TsType::TsTypeLit(lit) = ts_type.as_ref() {
                    return self.extract_field_type(lit, "error");
                }
            }
        }
        String::from("String")
    }

    fn extract_field_type(&mut self, lit: &swc_ecma_ast::TsTypeLit, field_name: &str) -> String {
        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    ident.sym.as_ref()
                } else {
                    continue;
                };

                if name == field_name {
                    if let Some(type_ann) = &prop.type_ann {
                        return self.resolver.resolve(&type_ann.type_ann).to_string();
                    }
                }
            }
        }
        String::from("()")
    }

    fn is_string_type(&self, ty: &TsType) -> bool {
        // Check for string keyword (string)
        if let TsType::TsKeywordType(k) = ty {
            return k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword;
        }
        // Check for string literal type ("Move")
        if let TsType::TsLitType(lit) = ty {
            return matches!(lit.lit, swc_ecma_ast::TsLit::Str(_));
        }
        false
    }

    fn is_tagged_variant(&self, ts_type: &TsType) -> bool {
        let TsType::TsTypeLit(lit) = ts_type else { return false };
        lit.members.iter().any(|m| self.has_string_tag(m))
    }

    fn has_string_tag(&self, member: &swc_ecma_ast::TsTypeElement) -> bool {
        let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member else { return false };
        let Some(type_ann) = &prop.type_ann else { return false };
        self.is_string_type(&type_ann.type_ann)
    }

    pub(super) fn collect_tagged_union(&mut self, name: &str, types: &[Box<TsType>]) {
        let variants: Vec<super::types::EnumVariant> = types
            .iter()
            .filter_map(|ts_type| self.extract_variant(ts_type.as_ref(), name))
            .collect();

        if !variants.is_empty() {
            self.enums.insert(name.to_string(), EnumDefinition { name: name.to_string(), variants });
        }
    }

    fn extract_variant(&mut self, ts_type: &TsType, enum_name: &str) -> Option<super::types::EnumVariant> {
        let TsType::TsTypeLit(lit) = ts_type else { return None };
        let (tag, fields) = self.extract_tag_and_fields(lit, enum_name)?;
        Some(super::types::EnumVariant { name: tag, fields })
    }

    fn extract_tag_and_fields(&mut self, lit: &swc_ecma_ast::TsTypeLit, enum_name: &str) -> Option<(String, Vec<(String, RustType)>)> {
        let mut tag = String::new();
        let mut fields = Vec::new();

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some((name, ty)) = self.extract_property(prop, enum_name, &mut tag) {
                    fields.push((name, ty));
                }
            }
        }

        if tag.is_empty() { None } else { Some((tag, fields)) }
    }

    fn extract_property(&mut self, prop: &swc_ecma_ast::TsPropertySignature, _enum_name: &str, tag_buffer: &mut String) -> Option<(String, RustType)> {
        let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() else { return None };
        let field_name = ident.sym.to_string();
        let type_ann = prop.type_ann.as_ref()?;
        let ty = self.resolver.resolve(&type_ann.type_ann);

        if field_name == "tag" && self.is_string_type(&type_ann.type_ann) {
            // Extract the actual tag value from string literal
            let tag_value = self.extract_string_value(&type_ann.type_ann);
            *tag_buffer = tag_value;
            None
        } else {
            Some((field_name, ty))
        }
    }

    fn extract_string_value(&self, ty: &TsType) -> String {
        if let TsType::TsLitType(lit) = ty {
            if let swc_ecma_ast::TsLit::Str(s) = &lit.lit {
                // Extract string value and strip surrounding quotes if present
                let raw = format!("{:?}", s.value);
                return raw.trim_matches('"').to_string();
            }
        }
        // Fallback for string keyword - use enum name
        String::new()
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
