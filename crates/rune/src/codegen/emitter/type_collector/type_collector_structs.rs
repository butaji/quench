//! # Type Collector - Structs
//!
//! Handles struct type collection from SWC AST.

use crate::codegen::emitter::{RustType, TypeResolver};
use super::StructInfo;
use crate::utils::to_pascal_case;
use std::collections::HashMap;

/// Struct collection implementation.
pub(super) struct TypeCollectorStructs {
    #[allow(dead_code)]
    dummy: bool,
}

impl TypeCollectorStructs {
    #[must_use]
    pub(super) fn new() -> Self {
        Self { dummy: false }
    }

    pub(super) fn collect_interface(
        &self,
        name: &str,
        body: &swc_ecma_ast::TsInterfaceBody,
        structs: &mut HashMap<String, StructInfo>,
        resolver: &mut TypeResolver,
    ) {
        let fields = self.extract_interface_fields(body);
        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), resolver.resolve(t)))
            .collect();
        structs.insert(
            name.to_string(),
            StructInfo {
                rust_name,
                fields: resolved_fields,
            },
        );
    }

    fn extract_interface_fields(
        &self,
        body: &swc_ecma_ast::TsInterfaceBody,
    ) -> Vec<(String, swc_ecma_ast::TsType)> {
        body.body
            .iter()
            .filter_map(|member| {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                    let field_name = self.extract_field_name(&prop.key);
                    prop.type_ann
                        .as_ref()
                        .map(|ann| (field_name, (*ann.type_ann).clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    fn extract_field_name(&self, key: &swc_ecma_ast::Expr) -> String {
        match key {
            swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::Expr::Lit(swc_ecma_ast::Lit::Str(s)) => {
                format!("{:?}", s.value)
            }
            _ => "_unknown".to_string(),
        }
    }

    pub(super) fn collect_struct_from_literal(
        &self,
        name: &str,
        lit: &swc_ecma_ast::TsTypeLit,
        structs: &mut HashMap<String, StructInfo>,
        resolver: &mut TypeResolver,
    ) {
        let fields = self.extract_literal_fields(lit);
        let rust_name = to_pascal_case(name);
        let resolved_fields: Vec<(String, RustType)> = fields
            .iter()
            .map(|(n, t)| (n.clone(), resolver.resolve(t)))
            .collect();
        structs.insert(
            name.to_string(),
            StructInfo {
                rust_name,
                fields: resolved_fields,
            },
        );
    }

    fn extract_literal_fields(
        &self,
        lit: &swc_ecma_ast::TsTypeLit,
    ) -> Vec<(String, swc_ecma_ast::TsType)> {
        lit.members
            .iter()
            .filter_map(|member| {
                if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                        let field_name = ident.sym.to_string();
                        return prop
                            .type_ann
                            .as_ref()
                            .map(|ann| (field_name, (*ann.type_ann).clone()));
                    }
                }
                None
            })
            .collect()
    }
}

impl Default for TypeCollectorStructs {
    fn default() -> Self {
        Self::new()
    }
}
