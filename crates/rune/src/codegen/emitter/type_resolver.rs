//! # Type Resolver
//!
//! Resolves TypeScript types to Rust types.

use super::types::StructFields;
use super::RustType;

/// Type resolver for SWC types.
pub struct TypeResolver {
    anonymous_struct_counter: usize,
    pending_anonymous_structs: Vec<(String, StructFields)>,
    result_types: std::collections::HashMap<String, String>,
}

impl TypeResolver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            anonymous_struct_counter: 0,
            pending_anonymous_structs: Vec::new(),
            result_types: std::collections::HashMap::new(),
        }
    }

    /// Register a Result type with its value type
    pub fn register_result_type(&mut self, name: &str, value_type: String) {
        self.result_types.insert(name.to_string(), value_type);
    }

    #[allow(clippy::too_many_lines)]
    pub fn resolve(&mut self, ts_type: &swc_ecma_ast::TsType) -> RustType {
        match ts_type {
            swc_ecma_ast::TsType::TsKeywordType(k) => self.resolve_keyword(k.kind),
            swc_ecma_ast::TsType::TsArrayType(arr) => self.resolve_array(&arr.elem_type),
            swc_ecma_ast::TsType::TsTypeRef(type_ref) => self.resolve_type_ref(type_ref),
            swc_ecma_ast::TsType::TsUnionOrIntersectionType(union) => self.resolve_union(union),
            swc_ecma_ast::TsType::TsParenthesizedType(paren) => self.resolve(&paren.type_ann),
            swc_ecma_ast::TsType::TsTupleType(_) => RustType::Unknown,
            swc_ecma_ast::TsType::TsTypeLit(lit) => self.resolve_type_literal(lit),
            _ => RustType::Unknown,
        }
    }

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

    fn resolve_array(&mut self, elem_type: &swc_ecma_ast::TsType) -> RustType {
        RustType::Vec(Box::new(self.resolve(elem_type)))
    }

    fn resolve_type_ref(&mut self, type_ref: &swc_ecma_ast::TsTypeRef) -> RustType {
        let name = extract_type_name(&type_ref.type_name);
        self.resolve_named_type(&name, type_ref.type_params.as_deref())
    }

    fn resolve_named_type(
        &mut self,
        name: &str,
        params: Option<&swc_ecma_ast::TsTypeParamInstantiation>,
    ) -> RustType {
        if name == "null" {
            return RustType::Unknown;
        }

        if let Some(p) = params {
            if !p.params.is_empty() {
                return self.resolve_generic_type(name, p);
            }
        }

        self.resolve_simple_type(name)
    }

    fn resolve_generic_type(
        &mut self,
        name: &str,
        params: &swc_ecma_ast::TsTypeParamInstantiation,
    ) -> RustType {
        match name {
            "Array" => self.resolve_vec_param(&params.params[0]),
            "Record" => self.resolve_record_type(params),
            "Result" if params.params.len() >= 2 => {
                let inner = self.resolve(&params.params[0]);
                RustType::Result(Box::new(inner))
            }
            "Option" => {
                let inner = self.resolve(&params.params[0]);
                RustType::Option(Box::new(inner))
            }
            "Map" => self.resolve_map_type(params),
            "Set" => self.resolve_set_type(params),
            _ => {
                let inner = self.resolve(&params.params[0]);
                RustType::Custom(format!("{name}<{inner}>"))
            }
        }
    }

    fn resolve_record_type(&mut self, params: &swc_ecma_ast::TsTypeParamInstantiation) -> RustType {
        // Record<K, V> maps to std::collections::HashMap<K, V> or IndexMap
        if params.params.len() >= 2 {
            let key = self.resolve(&params.params[0]);
            let value = self.resolve(&params.params[1]);
            let key_str = key.to_rust_type_string();
            let value_str = value.to_rust_type_string();
            RustType::Custom(format!("std::collections::HashMap<{key_str}, {value_str}>"))
        } else {
            RustType::Custom("std::collections::HashMap<String, ()>".to_string())
        }
    }

    fn resolve_map_type(&mut self, params: &swc_ecma_ast::TsTypeParamInstantiation) -> RustType {
        if params.params.len() >= 2 {
            let key = self.resolve(&params.params[0]);
            let value = self.resolve(&params.params[1]);
            let key_str = key.to_rust_type_string();
            let value_str = value.to_rust_type_string();
            RustType::Custom(format!("std::collections::HashMap<{key_str}, {value_str}>"))
        } else {
            RustType::Custom("std::collections::HashMap<String, ()>".to_string())
        }
    }

    fn resolve_set_type(&mut self, params: &swc_ecma_ast::TsTypeParamInstantiation) -> RustType {
        if !params.params.is_empty() {
            let elem = self.resolve(&params.params[0]);
            let elem_str = elem.to_rust_type_string();
            RustType::Custom(format!("std::collections::HashSet<{elem_str}>"))
        } else {
            RustType::Custom("std::collections::HashSet<()>" .to_string())
        }
    }

    fn resolve_vec_param(&mut self, param: &swc_ecma_ast::TsType) -> RustType {
        RustType::Vec(Box::new(self.resolve(param)))
    }

    fn resolve_simple_type(&self, name: &str) -> RustType {
        // Check if this is a registered Result type
        if let Some(value_type) = self.result_types.get(name).cloned() {
            return RustType::Result(Box::new(self.resolve_type_string(&value_type)));
        }

        match name {
            "Result" => RustType::Result(Box::new(RustType::Unknown)),
            "Option" => RustType::Option(Box::new(RustType::Unknown)),
            _ => RustType::Custom(name.to_string()),
        }
    }

    /// Resolve a type from a string representation
    fn resolve_type_string(&self, type_str: &str) -> RustType {
        match type_str {
            "f64" | "F64" => RustType::F64,
            "i32" | "I32" => RustType::I32,
            "bool" | "Bool" => RustType::Bool,
            "String" => RustType::String,
            "()" => RustType::Unit,
            _ => RustType::Custom(type_str.to_string()),
        }
    }

    fn resolve_union(&mut self, union: &swc_ecma_ast::TsUnionOrIntersectionType) -> RustType {
        let swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u) = union else {
            return RustType::Unknown;
        };

        if u.types.len() != 2 {
            return RustType::Unknown;
        }

        // Check for Result pattern: { ok: true, value: T } | { ok: false, error: E }
        if let Some(result_type) = self.try_extract_result_type(&u.types) {
            return result_type;
        }

        // Check for Option pattern: T | null
        if self.has_null_type(&u.types) {
            return self.extract_option_from_union(&u.types);
        }

        RustType::Unknown
    }

    /// Try to extract Result<T, E> from a union type.
    /// Returns Some(Result) if the union is { ok: true, value: T } | { ok: false, error: E }.
    fn try_extract_result_type(
        &mut self,
        types: &[Box<swc_ecma_ast::TsType>],
    ) -> Option<RustType> {
        // Both variants must exist for a valid Result pattern
        let has_ok = types.iter().any(|t| self.is_ok_variant(t));
        let has_err = types.iter().any(|t| self.is_err_variant(t));
        if !has_ok || !has_err {
            return None;
        }
        let ok_variant = types.iter().find(|t| self.is_ok_variant(t))?;
        let value_type = self.extract_result_value_type(ok_variant)?;
        Some(RustType::Result(Box::new(value_type)))
    }

    fn is_ok_variant(&self, ts_type: &swc_ecma_ast::TsType) -> bool {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = &ts_type else {
            return false;
        };
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

    fn is_err_variant(&self, ts_type: &swc_ecma_ast::TsType) -> bool {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = &ts_type else {
            return false;
        };
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

    fn extract_result_value_type(&mut self, ok_variant: &swc_ecma_ast::TsType) -> Option<RustType> {
        let swc_ecma_ast::TsType::TsTypeLit(lit) = &ok_variant else {
            return None;
        };
        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
                    ident.sym.as_ref()
                } else {
                    continue;
                };
                if field_name == "value" {
                    return prop.type_ann.as_ref().map(|ann| self.resolve(&ann.type_ann));
                }
            }
        }
        Some(RustType::Unit)
    }

    fn has_null_type(&self, types: &[Box<swc_ecma_ast::TsType>]) -> bool {
        types.iter().any(|t| self.is_null_keyword(t.as_ref()))
    }

    fn is_null_keyword(&self, ts_type: &swc_ecma_ast::TsType) -> bool {
        if let swc_ecma_ast::TsType::TsKeywordType(k) = ts_type {
            k.kind == swc_ecma_ast::TsKeywordTypeKind::TsNullKeyword
        } else {
            false
        }
    }

    fn extract_option_from_union(&mut self, types: &[Box<swc_ecma_ast::TsType>]) -> RustType {
        let non_null = types.iter().find(|t| !self.is_null_keyword(t.as_ref()));
        non_null.map_or(RustType::Unknown, |t| {
            RustType::Option(Box::new(self.resolve(t)))
        })
    }

    fn resolve_type_literal(&mut self, lit: &swc_ecma_ast::TsTypeLit) -> RustType {
        let fields = self.extract_literal_fields(lit);
        self.create_anonymous_struct(fields)
    }

    fn extract_literal_fields(&mut self, lit: &swc_ecma_ast::TsTypeLit) -> Vec<(String, RustType)> {
        let mut fields = Vec::new();
        let mut field_counter = 0;

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let (name, ty) = self.extract_field(prop, &mut field_counter);
                if let Some((n, t)) = name.zip(ty) {
                    fields.push((n, t));
                }
            }
        }

        fields
    }

    fn extract_field(
        &mut self,
        prop: &swc_ecma_ast::TsPropertySignature,
        counter: &mut usize,
    ) -> (Option<String>, Option<RustType>) {
        let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
            Some(ident.sym.to_string())
        } else {
            *counter += 1;
            Some(format!("_field{counter}"))
        };

        let field_type = prop
            .type_ann
            .as_ref()
            .map(|ann| self.resolve(&ann.type_ann));

        (field_name, field_type)
    }

    fn create_anonymous_struct(&mut self, fields: Vec<(String, RustType)>) -> RustType {
        self.anonymous_struct_counter += 1;
        let struct_name = format!("__AnonymousStruct{}", self.anonymous_struct_counter);
        self.pending_anonymous_structs
            .push((struct_name.clone(), fields));
        RustType::Custom(struct_name)
    }

    #[must_use]
    pub fn take_pending_structs(&mut self) -> Vec<(String, StructFields)> {
        std::mem::take(&mut self.pending_anonymous_structs)
    }
}

fn extract_type_name(name: &swc_ecma_ast::TsEntityName) -> String {
    match name {
        swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
        swc_ecma_ast::TsEntityName::TsQualifiedName(_) => "Unknown".to_string(),
    }
}

impl Default for TypeResolver {
    fn default() -> Self {
        Self::new()
    }
}
