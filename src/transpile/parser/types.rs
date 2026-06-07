//! Type parsing utilities - converts oxc TypeAnnotation to HIR Type
//!

use super::super::hir::*;
use anyhow::Result;
use oxc_ast::ast::*;

// ============================================================================
// TypeParser - converts oxc TypeAnnotation to HIR Type
// ============================================================================

pub struct TypeParser;

impl TypeParser {
    pub fn new() -> Self {
        Self
    }

    /// Convert oxc TypeAnnotation to HIR Type
    pub fn convert_type(&self, type_annotation: &TSTypeAnnotation) -> Result<Type> {
        self.convert_ts_type(&type_annotation.type_annotation)
    }
}

// ============================================================================
// TSType conversion - main entry point
// ============================================================================

impl TypeParser {
    /// Convert a TSNode to HIR Type
    pub fn convert_ts_type(&self, ts_type: &TSType) -> Result<Type> {
        use TSType::*;
        // Simple scalar types
        if let TSArrayType(a) = ts_type { return self.convert_array_type(a); }
        if let TSConditionalType(c) = ts_type { return self.convert_multi(Some(c), None, None); }
        if let TSIntersectionType(i) = ts_type { return self.convert_multi(None, Some(i), None); }
        if let TSUnionType(u) = ts_type { return self.convert_multi(None, None, Some(u)); }
        if let TSConstructorType(c) = ts_type { return self.convert_fn(&c.params, &c.return_type); }
        if let TSFunctionType(f) = ts_type { return self.convert_fn(&f.params, &f.return_type); }
        // Complex types - delegate to helpers
        self.convert_ts_type_complex(ts_type)
    }

    fn convert_ts_type_complex(&self, ts_type: &TSType) -> Result<Type> {
        use TSType::*;
        if matches!(ts_type, TSIndexedAccessType(_) | TSMappedType(_)) {
            return self.convert_index_mapped(ts_type);
        }
        if let TSInferType(infer) = ts_type { return self.convert_query_like(Some(infer), None); }
        if let TSTypeQuery(query) = ts_type { return self.convert_query_like(None, Some(query)); }
        if matches!(ts_type, TSOptionalType(_) | TSParenthesizedType(_) | TSRestType(_)) {
            return self.convert_passthrough(ts_type);
        }
        if matches!(ts_type, TSRecordType(_) | TSTypeLiteralType(_) | TSTemplateLiteralType(_) | TSTypeOperatorType(_) | TSReferenceType(_)) {
            return self.convert_ref_or_obj(ts_type);
        }
        Ok(Type::Unknown)
    }

    fn convert_ref_or_obj(&self, ts_type: &TSType) -> Result<Type> {
        use TSType::*;
        match ts_type {
            TSRecordType(_) | TSTypeLiteralType(_) | TSTemplateLiteralType(_) | TSTypeOperatorType(_) => self.convert_object_like(ts_type),
            TSReferenceType(r) => self.convert_reference(r),
            _ => Ok(Type::Unknown),
        }
    }

    fn convert_multi(&self, cond: Option<&TSConditionalType>, inter: Option<&TSIntersectionType>, union: Option<&TSUnionType>) -> Result<Type> {
        if let Some(c) = cond {
            let check = self.convert_ts_type(&c.check_type)?;
            let extends = self.convert_ts_type(&c.extends_type)?;
            let true_type = self.convert_ts_type(&c.true_type)?;
            let false_type = self.convert_ts_type(&c.false_type)?;
            return Ok(Type::Conditional { check: Box::new(check), extends: Box::new(extends), true_type: Box::new(true_type), false_type: Box::new(false_type) });
        }
        if let Some(i) = inter {
            let types = i.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
            return Ok(Type::Intersection { types });
        }
        if let Some(u) = union {
            let types = u.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
            return Ok(Type::Union { types });
        }
        Ok(Type::Unknown)
    }

    fn convert_query_like(&self, infer: Option<&TSInferType>, query: Option<&TSTypeQuery>) -> Result<Type> {
        if let Some(i) = infer {
            Ok(Type::Infer { name: i.type_parameter.name.to_string() })
        } else if let Some(q) = query {
            Ok(Type::Query { expr: q.expr_name.to_string() })
        } else {
            Ok(Type::Unknown)
        }
    }

    fn convert_fn(&self, params: &[TSTypeParameter], ret: &TSType) -> Result<Type> {
        let params = params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(ret)?;
        Ok(Type::Function { params, ret: Box::new(ret) })
    }

    fn convert_index_mapped(&self, ts_type: &TSType) -> Result<Type> {
        match ts_type {
            TSType::TSIndexedAccessType(i) => self.convert_indexed_access(i),
            TSType::TSMappedType(m) => self.convert_mapped(m),
            _ => Ok(Type::Unknown),
        }
    }

    fn convert_multi(&self, inter: Option<&TSIntersectionType>, union: Option<&TSUnionType>) -> Result<Type> {
        if let Some(i) = inter {
            let types = i.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
            Ok(Type::Intersection { types })
        } else if let Some(u) = union {
            let types = u.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
            Ok(Type::Union { types })
        } else {
            Ok(Type::Unknown)
        }
    }

    fn convert_passthrough(&self, ts_type: &TSType) -> Result<Type> {
        match ts_type {
            TSType::TSOptionalType(o) => self.convert_optional(o),
            TSType::TSParenthesizedType(p) => self.convert_ts_type(&p.type_annotation),
            TSType::TSRestType(r) => self.convert_ts_type(&r.type_annotation),
            _ => Ok(Type::Unknown),
        }
    }

    fn convert_object_like(&self, ts_type: &TSType) -> Result<Type> {
        match ts_type {
            TSType::TSRecordType(r) => self.convert_record(r),
            TSType::TSTypeLiteralType(l) => self.convert_literal(l),
            _ => Ok(Type::Unknown),
        }
    }

    fn convert_simple_prim(&self, ts_type: &TSType) -> Result<Type> {
        Ok(Self::prim_to_hir(ts_type))
    }

    fn prim_to_hir(ts_type: &TSType) -> Type {
        use TSType::*;
        match ts_type {
            TSAnyKeyword(_) | TSUnknownKeyword(_) => Type::Any,
            TSBigIntKeyword(_) | TSNumberKeyword(_) => Type::BigInt,
            TSBooleanKeyword(_) => Type::Boolean,
            TSNeverKeyword(_) | TSNullKeyword(_) | TSUndefinedKeyword(_) | TSQualifiedName(_) | TSVoidKeyword(_) | TSSymbolKeyword(_) => Type::Never,
            TSObjectKeyword(_) => Type::Object { members: vec![] },
            TSStringKeyword(_) => Type::String,
            TSThisType(_) => Type::This,
            _ => Type::Unknown,
        }
    }

    // --- Primitive type converters ---
    fn convert_fn_type(&self, params: &[TSTypeParameter], ret: &TSType) -> Result<Type> {
        let params = params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(ret)?;
        Ok(Type::Function { params, ret: Box::new(ret) })
    }

    fn convert_array_type(&self, a: &TSArrayType) -> Result<Type> {
        let elem = self.convert_ts_type(&a.element_type)?;
        Ok(Type::Array { elem: Box::new(elem) })
    }

    fn convert_conditional_type(&self, c: &TSConditionalType) -> Result<Type> {
        let check = self.convert_ts_type(&c.check_type)?;
        let extends = self.convert_ts_type(&c.extends_type)?;
        let true_type = self.convert_ts_type(&c.true_type)?;
        let false_type = self.convert_ts_type(&c.false_type)?;
        Ok(Type::Conditional {
            check: Box::new(check),
            extends: Box::new(extends),
            true_type: Box::new(true_type),
            false_type: Box::new(false_type),
        })
    }

    fn convert_indexed_access(&self, i: &TSIndexedAccessType) -> Result<Type> {
        let obj = self.convert_ts_type(&i.object_type)?;
        let index = self.convert_ts_type(&i.index_type)?;
        Ok(Type::Index { obj: Box::new(obj), index: Box::new(index) })
    }

    fn convert_intersection(&self, i: &TSIntersectionType) -> Result<Type> {
        let types = i.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
        Ok(Type::Intersection { types })
    }

    fn convert_mapped(&self, m: &TSMappedType) -> Result<Type> {
        let from = self.convert_ts_type(&m.type_annotation)?;
        let to = self.convert_ts_type(&m.template_type)?;
        Ok(Type::Mapped { from: Box::new(from), to: Box::new(to) })
    }

    fn convert_optional(&self, o: &TSOptionalType) -> Result<Type> {
        self.convert_ts_type(&o.type_annotation)
    }

    fn convert_record(&self, r: &TSRecordType) -> Result<Type> {
        let key = self.convert_ts_type(&r.index_type)?;
        let value = self.convert_ts_type(&r.body_type)?;
        Ok(Type::Record { key: Box::new(key), value: Box::new(value) })
    }

    fn convert_reference(&self, r: &TSReferenceType) -> Result<Type> {
        let name = Self::get_ref_name(&r.type_name);
        let generics = r.type_parameters.as_ref().map_or(vec![], |tp| {
            tp.params.iter().filter_map(|p| self.convert_ts_type(p).ok()).collect()
        });
        Ok(Type::Ref { name, generics })
    }

    fn get_ref_name(type_name: &TSTypeName) -> String {
        if let Some(id) = type_name.get_identifier() {
            id.name.to_string()
        } else if let Some(qual) = type_name.getQualifiedName() {
            qual.to_string()
        } else {
            "Unknown".to_string()
        }
    }

    fn convert_template(&self, t: &TSTemplateLiteralType) -> Result<Type> {
        let parts = t.quasis.iter()
            .map(|q| TemplatePart::String { value: q.value.raw.to_string() })
            .collect();
        let values = t.types.iter().filter_map(|tp| self.convert_ts_type(tp).ok()).collect();
        Ok(Type::Template { parts, values })
    }

    fn convert_literal(&self, l: &TSTypeLiteralType) -> Result<Type> {
        let members = l.members.iter().filter_map(|m| self.convert_type_member(m)).collect();
        Ok(Type::Object { members })
    }

    fn convert_operator(&self, t: &TSTypeOperatorType) -> Result<Type> {
        match t.operator.kind {
            TSTypeOperatorKeyword::Keyof | TSTypeOperatorKeyword::Readonly => {
                let inner = self.convert_ts_type(&t.type_annotation)?;
                let type_name = match t.operator.kind {
                    TSTypeOperatorKeyword::Keyof => Type::KeyOf { inner: Box::new(inner) },
                    TSTypeOperatorKeyword::Readonly => Type::Readonly { inner: Box::new(inner) },
                    _ => return Ok(Type::Unknown),
                };
                Ok(type_name)
            }
            TSTypeOperatorKeyword::Unique => Ok(Type::Symbol),
            _ => Ok(Type::Unknown),
        }
    }

    fn convert_union(&self, u: &TSUnionType) -> Result<Type> {
        let types = u.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
        Ok(Type::Union { types })
    }

    // --- Type member converters ---
    fn convert_type_member(&self, member: &TSTypeMember) -> Option<TypeMember> {
        match member {
            TSTypeMember::TSCallSignatureDeclaration(c) => self.convert_call_sig(c),
            TSTypeMember::TSConstructSignatureDeclaration(_) => Some(self.new_member("__construct")),
            TSTypeMember::TSPropertySignature(p) => self.convert_prop(p),
            TSTypeMember::TSMethodSignature(m) => self.convert_method(m),
            TSTypeMember::TSIndexSignature(i) => self.convert_index(i),
            _ => None,
        }
    }

    fn new_member(key: &str) -> TypeMember {
        TypeMember { key: key.to_string(), type_: Type::Object { members: vec![] }, optional: false, readonly: false }
    }

    fn convert_call_sig(&self, c: &TSCallSignatureDeclaration) -> Option<TypeMember> {
        let params = c.params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(&c.return_type).ok()?;
        Some(TypeMember {
            key: "__call".to_string(),
            type_: Type::Function { params, ret: Box::new(ret) },
            optional: false,
            readonly: false,
        })
    }

    fn convert_prop(&self, p: &TSPropertySignature) -> Option<TypeMember> {
        let key = self.get_key(&p.key)?;
        let type_ = self.convert_ts_type(&p.type_annotation).ok()?;
        Some(TypeMember { key, type_, optional: p.optional, readonly: p.readonly })
    }

    fn convert_method(&self, m: &TSMethodSignature) -> Option<TypeMember> {
        let key = self.get_key(&m.key)?;
        let params = m.params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(&m.return_type).ok()?;
        Some(TypeMember {
            key,
            type_: Type::Function { params, ret: Box::new(ret) },
            optional: m.optional,
            readonly: false,
        })
    }

    fn convert_index(&self, i: &TSIndexSignature) -> Option<TypeMember> {
        let key_type = self.convert_ts_type(&i.key_type).ok()?;
        let value_type = self.convert_ts_type(&i.value_type).ok()?;
        Some(TypeMember {
            key: "__index".to_string(),
            type_: Type::Index { obj: Box::new(key_type), index: Box::new(value_type) },
            optional: false,
            readonly: false,
        })
    }

    fn get_key(&self, key: &PropertyKey) -> Option<String> {
        match key {
            PropertyKey::StaticIdentifier(i) => Some(i.name.to_string()),
            PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
            PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
            _ => None,
        }
    }

    fn convert_param_type(&self, param: &TSTypeParameter) -> Option<Type> {
        self.convert_ts_type(&param.type_annotation).ok()
    }
}

// ============================================================================
// Public API - parse type alias declaration
// ============================================================================

/// Parse a type alias declaration from a TSTypeAliasDeclaration
pub fn convert_type_alias(alias: &TSTypeAliasDeclaration) -> Result<TypeDecl> {
    let name = alias.id.name.to_string();
    let type_parser = TypeParser::new();
    let type_ = type_parser.convert_ts_type(&alias.type_annotation)?;

    let generics = alias.generics.iter().map(|g| {
        GenericParam {
            name: g.name.to_string(),
            constraint: g.constraint.as_ref().and_then(|c| type_parser.convert_ts_type(c).ok()),
            default: g.default.as_ref().and_then(|d| type_parser.convert_ts_type(d).ok()),
        }
    }).collect();

    Ok(TypeDecl { name, generics, type_ })
}
