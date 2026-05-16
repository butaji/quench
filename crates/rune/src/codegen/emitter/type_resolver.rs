//! # Type Resolver
//!
//! Resolves TypeScript types to Rust types.

use super::types::StructFields;
use super::RustType;

/// Type resolver for SWC types.
pub struct TypeResolver {
    /// Counter for anonymous structs
    anonymous_struct_counter: usize,
    /// Pending anonymous structs
    pending_anonymous_structs: Vec<(String, StructFields)>,
}

impl TypeResolver {
    /// Create a new type resolver.
    #[must_use]
    pub fn new() -> Self {
        Self {
            anonymous_struct_counter: 0,
            pending_anonymous_structs: Vec::new(),
        }
    }

    /// Resolve a SWC type to Rust type.
    #[must_use]
    pub fn resolve(&mut self, ts_type: &swc_ecma_ast::TsType) -> RustType {
        match ts_type {
            swc_ecma_ast::TsType::TsKeywordType(k) => self.resolve_keyword(k.kind),
            swc_ecma_ast::TsType::TsArrayType(arr) => {
                RustType::Vec(Box::new(self.resolve(&arr.elem_type)))
            }
            swc_ecma_ast::TsType::TsTypeRef(type_ref) => self.resolve_type_ref(type_ref),
            swc_ecma_ast::TsType::TsUnionOrIntersectionType(union) => {
                self.resolve_union(union)
            }
            swc_ecma_ast::TsType::TsParenthesizedType(paren) => {
                self.resolve(&paren.type_ann)
            }
            swc_ecma_ast::TsType::TsTupleType(_) => RustType::Unknown,
            swc_ecma_ast::TsType::TsTypeLit(lit) => self.resolve_type_literal(lit),
            _ => RustType::Unknown,
        }
    }

    /// Resolve a keyword type.
    #[must_use]
    fn resolve_keyword(&self, kind: swc_ecma_ast::TsKeywordTypeKind) -> RustType {
        match kind {
            swc_ecma_ast::TsKeywordTypeKind::TsNumberKeyword => RustType::F64,
            swc_ecma_ast::TsKeywordTypeKind::TsStringKeyword => RustType::String,
            swc_ecma_ast::TsKeywordTypeKind::TsBooleanKeyword => RustType::Bool,
            swc_ecma_ast::TsKeywordTypeKind::TsVoidKeyword
            | swc_ecma_ast::TsKeywordTypeKind::TsUndefinedKeyword => RustType::Unit,
            swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword => RustType::Unknown,
            _ => RustType::Unknown,
        }
    }

    /// Resolve a type reference.
    #[must_use]
    fn resolve_type_ref(&mut self, type_ref: &swc_ecma_ast::TsTypeRef) -> RustType {
        let name = match &type_ref.type_name {
            swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
        };

        if name == "null" {
            return RustType::Unknown;
        }

        if let Some(params) = &type_ref.type_params {
            if !params.params.is_empty() {
                if name == "Array" {
                    let inner = self.resolve(&params.params[0]);
                    return RustType::Vec(Box::new(inner));
                }
                if name == "Result" && !params.params.is_empty() && params.params.len() >= 2 {
                    // Result<T, E> -> Result<T, String>
                    let inner = self.resolve(&params.params[0]);
                    return RustType::Result(Box::new(inner));
                }
                if name == "Option" && !params.params.is_empty() {
                    let inner = self.resolve(&params.params[0]);
                    return RustType::Option(Box::new(inner));
                }
                // For other generic types, just resolve the first param
                let inner = self.resolve(&params.params[0]);
                return RustType::Custom(format!("{name}<{inner}>"));
            }
        }

        // Handle common types
        match name.as_str() {
            "Widget" | "Task" | "Filter" => RustType::Custom(name.clone()),
            // AppState should always be passed by mutable borrow
            "AppState" => RustType::MutBorrow(Box::new(RustType::Custom(name))),
            "Result" => RustType::Result(Box::new(RustType::Unknown)),
            "Option" => RustType::Option(Box::new(RustType::Unknown)),
            _ => RustType::Custom(name),
        }
    }

    /// Resolve a union type (for Option<T | null>).
    #[must_use]
    fn resolve_union(&mut self, union: &swc_ecma_ast::TsUnionOrIntersectionType) -> RustType {
        let swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u) = union else {
            return RustType::Unknown;
        };

        if u.types.len() != 2 {
            return RustType::Unknown;
        }

        let has_null = u.types.iter().any(|t| {
            if let swc_ecma_ast::TsType::TsKeywordType(k) = t.as_ref() {
                k.kind == swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword
            } else {
                false
            }
        });

        if !has_null {
            return RustType::Unknown;
        }

        let non_null = u.types.iter().find(|t| {
            if let swc_ecma_ast::TsType::TsKeywordType(k) = t.as_ref() {
                k.kind != swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword
            } else {
                true
            }
        });

        non_null
            .map_or(RustType::Unknown, |t| RustType::Option(Box::new(self.resolve(t))))
    }

    /// Resolve a type literal (anonymous struct).
    #[must_use]
    fn resolve_type_literal(&mut self, lit: &swc_ecma_ast::TsTypeLit) -> RustType {
        let mut fields = Vec::new();
        let mut field_counter = 0;

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    ident.sym.to_string()
                } else {
                    field_counter += 1;
                    format!("_field{field_counter}")
                };

                let field_type = prop
                    .type_ann
                    .as_ref()
                    .map_or(RustType::Unknown, |ann| self.resolve(&ann.type_ann));

                fields.push((field_name, field_type));
            }
        }

        self.anonymous_struct_counter += 1;
        let struct_name = format!("__AnonymousStruct{}", self.anonymous_struct_counter);
        self.pending_anonymous_structs
            .push((struct_name.clone(), fields));
        RustType::Custom(struct_name)
    }

    /// Take pending anonymous structs.
    #[must_use]
    pub fn take_pending_structs(&mut self) -> Vec<(String, StructFields)> {
        std::mem::take(&mut self.pending_anonymous_structs)
    }
}

impl Default for TypeResolver {
    fn default() -> Self {
        Self::new()
    }
}
