//! # Type Collector - Result Pattern
//!
//! Handles Result<T, E> type pattern detection and collection.

use crate::codegen::emitter::TypeResolver;

/// Result pattern collection implementation.
pub(super) struct TypeCollectorResult {
    #[allow(dead_code)]
    dummy: bool,
}

impl TypeCollectorResult {
    #[must_use]
    pub(super) fn new() -> Self {
        Self { dummy: false }
    }

    /// Check if this is a Result pattern: {ok: true, value: T} | {ok: false, error: E}
    pub(super) fn is_result_pattern(&self, type_ann: &swc_ecma_ast::TsType) -> bool {
        let swc_ecma_ast::TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = type_ann
        else {
            return false;
        };

        if u.types.len() != 2 {
            return false;
        }

        let ok_variant = u.types.iter().any(|t| self.is_ok_variant(t));
        let err_variant = u.types.iter().any(|t| self.is_error_variant(t));

        ok_variant && err_variant
    }

    fn is_ok_variant(&self, ts_type: &swc_ecma_ast::TsType) -> bool {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = ts_type else {
            return false;
        };
        let mut has_ok_true = false;
        let mut has_value = false;

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some(type_ann) = &prop.type_ann {
                    let field_name =
                        if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
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

    fn is_error_variant(&self, ts_type: &swc_ecma_ast::TsType) -> bool {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = ts_type else {
            return false;
        };
        let mut has_ok_false = false;
        let mut has_error = false;

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                if let Some(type_ann) = &prop.type_ann {
                    let field_name =
                        if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
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

    fn is_true_literal(&self, ty: &swc_ecma_ast::TsType) -> bool {
        if let swc_ecma_ast::TsType::TsLitType(lit) = ty {
            return matches!(
                lit.lit,
                swc_ecma_ast::TsLit::Bool(swc_ecma_ast::Bool { value: true, .. })
            );
        }
        false
    }

    fn is_false_literal(&self, ty: &swc_ecma_ast::TsType) -> bool {
        if let swc_ecma_ast::TsType::TsLitType(lit) = ty {
            return matches!(
                lit.lit,
                swc_ecma_ast::TsLit::Bool(swc_ecma_ast::Bool { value: false, .. })
            );
        }
        false
    }

    /// Collect a Result type and register it for emission
    pub(super) fn collect_result_type(
        &self,
        name: &str,
        union: &swc_ecma_ast::TsType,
        resolver: &mut TypeResolver,
    ) {
        let value_type = self.extract_result_value_type(union);
        resolver.register_result_type(name, value_type);
    }

    fn extract_result_value_type(&self, union: &swc_ecma_ast::TsType) -> String {
        let swc_ecma_ast::TsType::TsUnionOrIntersectionType(
            swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u),
        ) = union
        else {
            return String::from("()");
        };

        for ts_type in &u.types {
            if self.is_ok_variant(ts_type) {
                if let swc_ecma_ast::TsType::TsTypeLit(lit) = ts_type.as_ref() {
                    return self.extract_field_type(lit, "value");
                }
            }
        }
        String::from("()")
    }

    fn extract_field_type(&self, lit: &swc_ecma_ast::TsTypeLit, field_name: &str) -> String {
        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    ident.sym.as_ref()
                } else {
                    continue;
                };

                if name == field_name {
                    if let Some(type_ann) = &prop.type_ann {
                        return Self::resolve_type(&type_ann.type_ann);
                    }
                }
            }
        }
        String::from("()")
    }

    fn resolve_type(ty: &swc_ecma_ast::TsType) -> String {
        match ty {
            swc_ecma_ast::TsType::TsKeywordType(k) => match k.kind {
                swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => "f64".to_string(),
                swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => "String".to_string(),
                swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => "bool".to_string(),
                _ => "()".to_string(),
            },
            swc_ecma_ast::TsType::TsArrayType(arr) => {
                let inner = Self::resolve_type(&arr.elem_type);
                format!("Vec<{}>", inner)
            }
            swc_ecma_ast::TsType::TsTypeRef(type_ref) => {
                // TsEntityName is either Ident or TsQualifiedName
                match &type_ref.type_name {
                    swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::TsEntityName::TsQualifiedName(q) => {
                        // For qualified names like Namespace.Type
                        // left is another TsEntityName, recursively handle it
                        match &q.left {
                            swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                            swc_ecma_ast::TsEntityName::TsQualifiedName(inner) => {
                                match &inner.left {
                                    swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                                    swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
                                }
                            }
                        }
                    }
                }
            }
            _ => "()".to_string(),
        }
    }
}

impl Default for TypeCollectorResult {
    fn default() -> Self {
        Self::new()
    }
}
