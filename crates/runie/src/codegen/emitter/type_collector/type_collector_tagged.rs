//! # Type Collector - Tagged Unions
//!
//! Handles tagged union (enum) type detection and collection.

use crate::codegen::emitter::types::{EnumDefinition, EnumVariant, RustType};
use crate::codegen::emitter::TypeResolver;
use std::collections::HashMap;

/// Tagged union collection implementation.
pub(super) struct TypeCollectorTagged {
    #[allow(dead_code)]
    dummy: bool,
}

impl TypeCollectorTagged {
    #[must_use]
    pub(super) fn new() -> Self {
        Self { dummy: false }
    }

    /// Try to extract tagged union types from a union type.
    pub(super) fn try_extract_tagged_union(
        &self,
        type_ann: &swc_ecma_ast::TsType,
    ) -> Option<Vec<swc_ecma_ast::TsType>> {
        let swc_ecma_ast::TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann
        else {
            return None;
        };

        if u.types.iter().all(|t| self.is_tagged_variant(t)) {
            Some(u.types.iter().map(|b| b.as_ref().clone()).collect())
        } else {
            None
        }
    }

    fn is_tagged_variant(&self, ts_type: &swc_ecma_ast::TsType) -> bool {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = ts_type else {
            return false;
        };
        lit.members.iter().any(|m| self.has_string_tag(m))
    }

    fn has_string_tag(&self, member: &swc_ecma_ast::TsTypeElement) -> bool {
        let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member else {
            return false;
        };
        let Some(type_ann) = &prop.type_ann else {
            return false;
        };
        self.is_string_type(&type_ann.type_ann)
    }

    fn is_string_type(&self, ty: &swc_ecma_ast::TsType) -> bool {
        if let swc_ecma_ast::TsType::TsKeywordType(k) = ty {
            return k.kind == swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword;
        }
        if let swc_ecma_ast::TsType::TsLitType(lit) = ty {
            return matches!(lit.lit, swc_ecma_ast::TsLit::Str(_));
        }
        false
    }

    /// Collect a tagged union type.
    pub(super) fn collect_tagged_union(
        &self,
        name: &str,
        types: &[swc_ecma_ast::TsType],
        enums: &mut HashMap<String, EnumDefinition>,
        resolver: &mut TypeResolver,
    ) {
        let variants: Vec<EnumVariant> = types
            .iter()
            .filter_map(|ts_type| self.extract_variant(ts_type, resolver))
            .collect();

        if !variants.is_empty() {
            enums.insert(
                name.to_string(),
                EnumDefinition {
                    name: name.to_string(),
                    variants,
                },
            );
        }
    }

    fn extract_variant(
        &self,
        ts_type: &swc_ecma_ast::TsType,
        resolver: &mut TypeResolver,
    ) -> Option<EnumVariant> {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = ts_type else {
            return None;
        };
        let (tag, fields) = self.extract_tag_and_fields(lit, resolver)?;
        Some(EnumVariant { name: tag, fields })
    }

    fn extract_tag_and_fields(
        &self,
        lit: &swc_ecma_ast::TsTypeLit,
        resolver: &mut TypeResolver,
    ) -> Option<(String, Vec<(String, RustType)>)> {
        let mut tag = String::new();
        let mut fields = Vec::new();

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some((name, ty)) = self.extract_property(prop, resolver, &mut tag) {
                    fields.push((name, ty));
                }
            }
        }

        if tag.is_empty() {
            None
        } else {
            Some((tag, fields))
        }
    }

    fn extract_property(
        &self,
        prop: &swc_ecma_ast::TsPropertySignature,
        resolver: &mut TypeResolver,
        tag_buffer: &mut String,
    ) -> Option<(String, RustType)> {
        let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() else {
            return None;
        };
        let field_name = ident.sym.to_string();
        let type_ann = prop.type_ann.as_ref()?;
        let ty = resolver.resolve(&type_ann.type_ann);

        if field_name == "tag" && self.is_string_type(&type_ann.type_ann) {
            let tag_value = self.extract_string_value(&type_ann.type_ann);
            *tag_buffer = tag_value;
            None
        } else {
            Some((field_name, ty))
        }
    }

    fn extract_string_value(&self, ty: &swc_ecma_ast::TsType) -> String {
        if let swc_ecma_ast::TsType::TsLitType(lit) = ty {
            if let swc_ecma_ast::TsLit::Str(s) = &lit.lit {
                let raw = format!("{:?}", s.value);
                return raw.trim_matches('"').to_string();
            }
        }
        String::new()
    }
}

impl Default for TypeCollectorTagged {
    fn default() -> Self {
        Self::new()
    }
}
