//! # Type Resolver
//!
//! Resolves TypeScript types to Rust types.

use super::types::StructFields;
use super::RustType;

/// Type resolver for SWC types.
pub struct TypeResolver {
    anonymous_struct_counter: usize,
    pending_anonymous_structs: Vec<(String, StructFields)>,
}

impl TypeResolver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            anonymous_struct_counter: 0,
            pending_anonymous_structs: Vec::new(),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn resolve(&mut self, ts_type: &swc_ecma_ast::TsType) -> RustType {
        match ts_type {
            swc_ecma_ast::TsType::TsKeywordType(k) => self.resolve_keyword(k.kind),
            swc_ecma_ast::TsType::TsArrayType(arr) => self.resolve_array(&arr.elem_type),
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

    fn resolve_named_type(&mut self, name: &str, params: Option<&swc_ecma_ast::TsTypeParamInstantiation>) -> RustType {
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

    fn resolve_generic_type(&mut self, name: &str, params: &swc_ecma_ast::TsTypeParamInstantiation) -> RustType {
        match name {
            "Array" => self.resolve_vec_param(&params.params[0]),
            "Result" if params.params.len() >= 2 => {
                let inner = self.resolve(&params.params[0]);
                RustType::Result(Box::new(inner))
            }
            "Option" => {
                let inner = self.resolve(&params.params[0]);
                RustType::Option(Box::new(inner))
            }
            _ => {
                let inner = self.resolve(&params.params[0]);
                RustType::Custom(format!("{name}<{inner}>"))
            }
        }
    }

    fn resolve_vec_param(&mut self, param: &swc_ecma_ast::TsType) -> RustType {
        RustType::Vec(Box::new(self.resolve(param)))
    }

    fn resolve_simple_type(&self, name: &str) -> RustType {
        match name {
            "Widget" | "Task" | "Filter" => RustType::Custom(name.to_string()),
            "AppState" => RustType::MutBorrow(Box::new(RustType::Custom(name.to_string()))),
            "Result" => RustType::Result(Box::new(RustType::Unknown)),
            "Option" => RustType::Option(Box::new(RustType::Unknown)),
            _ => RustType::Custom(name.to_string()),
        }
    }

    fn resolve_union(&mut self, union: &swc_ecma_ast::TsUnionOrIntersectionType) -> RustType {
        let swc_ecma_ast::TsUnionOrIntersectionType::TsUnionType(u) = union else {
            return RustType::Unknown;
        };

        if u.types.len() != 2 {
            return RustType::Unknown;
        }

        if !self.has_null_type(&u.types) {
            return RustType::Unknown;
        }

        self.extract_option_from_union(&u.types)
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

    fn extract_field(&mut self, prop: &swc_ecma_ast::TsPropertySignature, counter: &mut usize) -> (Option<String>, Option<RustType>) {
        let field_name = if let swc_ecma_ast::Expr::Ident(ident) = prop.key.as_ref() {
            Some(ident.sym.to_string())
        } else {
            *counter += 1;
            Some(format!("_field{counter}"))
        };

        let field_type = prop.type_ann.as_ref().map(|ann| self.resolve(&ann.type_ann));

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
