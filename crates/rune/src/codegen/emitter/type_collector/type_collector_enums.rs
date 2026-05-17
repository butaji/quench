//! # Type Collector - Enums
//!
//! Handles enum type collection from SWC AST.

use crate::codegen::emitter::types::EnumDefinition;
use std::collections::HashMap;

/// Enum collection implementation.
pub(super) struct TypeCollectorEnums {
    #[allow(dead_code)]
    dummy: bool,
}

impl TypeCollectorEnums {
    #[must_use]
    pub(super) fn new() -> Self {
        Self { dummy: false }
    }

    pub(super) fn collect_enum(
        &self,
        name: &str,
        decl: &swc_ecma_ast::TsEnumDecl,
        enums: &mut HashMap<String, EnumDefinition>,
    ) {
        let variants: Vec<crate::codegen::emitter::types::EnumVariant> = decl
            .members
            .iter()
            .map(|m| crate::codegen::emitter::types::EnumVariant {
                name: self.extract_enum_member_name(m),
                fields: Vec::new(),
            })
            .collect();
        enums.insert(
            name.to_string(),
            EnumDefinition {
                name: name.to_string(),
                variants,
            },
        );
    }

    fn extract_enum_member_name(&self, member: &swc_ecma_ast::TsEnumMember) -> String {
        match &member.id {
            swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::TsEnumMemberId::Str(s) => format!("{:?}", s.value),
        }
    }
}

impl Default for TypeCollectorEnums {
    fn default() -> Self {
        Self::new()
    }
}
