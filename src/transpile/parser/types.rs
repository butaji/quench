//! Type parsing utilities - converts oxc TypeAnnotation to HIR Type
//!
//! allow:complexity,too_many_lines

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
    pub fn convert_type(&self, type_annotation: &TypeAnnotation) -> Result<Type> {
        self.convert_ts_type(&type_annotation.type_annotation)
    }
}

// ============================================================================
// TSType conversion
// ============================================================================

impl TypeParser {
    /// Convert a TSNode to HIR Type
    pub fn convert_ts_type(&self, ts_type: &TSType) -> Result<Type> {
        match ts_type {
            TSType::TSAnyKeyword(_) => Ok(Type::Any),
            TSType::TSBigIntKeyword(_) => Ok(Type::BigInt),
            TSType::TSBooleanKeyword(_) => Ok(Type::Boolean),
            TSType::TSNeverKeyword(_) => Ok(Type::Never),
            TSType::TSNullKeyword(_) => Ok(Type::Null),
            TSType::TSNumberKeyword(_) => Ok(Type::Number),
            TSType::TSObjectKeyword(_) => Ok(Type::Object { members: vec![] }),
            TSType::TSStringKeyword(_) => Ok(Type::String),
            TSType::TSSymbolKeyword(_) => Ok(Type::Symbol),
            TSType::TSUndefinedKeyword(_) => Ok(Type::Undefined),
            TSType::TSUnknownKeyword(_) => Ok(Type::Unknown),
            TSType::TSVoidKeyword(_) => Ok(Type::Void),
            TSType::TSThisType(_) => Ok(Type::This),
            TSType::TSArrayType(a) => self.convert_array_type(a),
            TSType::TSConditionalType(c) => self.convert_conditional_type(c),
            TSType::TSConstructorType(c) => self.convert_function_or_constructor_type(&c.params, &c.return_type),
            TSType::TSFunctionType(f) => self.convert_function_or_constructor_type(&f.params, &f.return_type),
            TSType::TSIndexedAccessType(i) => self.convert_indexed_access_type(i),
            TSType::TSInferType(i) => Ok(Type::Infer { name: i.type_parameter.name.to_string() }),
            TSType::TSIntersectionType(i) => self.convert_intersection_type(i),
            TSType::TSMappedType(m) => self.convert_mapped_type(m),
            TSType::TSNeverKeyword(_) => Ok(Type::Never),
            TSType::TSNullKeyword(_) => Ok(Type::Null),
            TSType::TSNumberKeyword(_) => Ok(Type::Number),
            TSType::TSObjectKeyword(_) => Ok(Type::Object { members: vec![] }),
            TSType::TSOptionalType(o) => self.convert_optional_type(o),
            TSType::TSParenthesizedType(p) => self.convert_ts_type(&p.type_annotation),
            TSType::TSQualifiedName(_) => Ok(Type::Unknown),
            TSType::TSRecordType(r) => self.convert_record_type(r),
            TSType::TSReferenceType(r) => self.convert_reference_type(r),
            TSType::TSRestType(r) => self.convert_ts_type(&r.type_annotation),
            TSType::TSStringKeyword(_) => Ok(Type::String),
            TSType::TSSymbolKeyword(_) => Ok(Type::Symbol),
            TSType::TSTemplateLiteralType(t) => self.convert_template_literal_type(t),
            TSType::TSTypeLiteralType(l) => self.convert_type_literal(l),
            TSType::TSTypeOperatorType(t) => self.convert_type_operator(t),
            TSType::TSTypeQueryType(q) => Ok(Type::Query { expr: q.expr_name.to_string() }),
            TSType::TSUndefinedKeyword(_) => Ok(Type::Undefined),
            TSType::TSUnionType(u) => self.convert_union_type(u),
            TSType::TSUnknownKeyword(_) => Ok(Type::Unknown),
            TSType::TSVoidKeyword(_) => Ok(Type::Void),
            // Fallback for types we don't handle yet
            _ => Ok(Type::Unknown),
        }
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

    fn convert_function_or_constructor_type(
        &self,
        params: &[TSTypeParameter],
        return_type: &TSType,
    ) -> Result<Type> {
        let params = params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(return_type)?;
        Ok(Type::Function { params, ret: Box::new(ret) })
    }

    fn convert_indexed_access_type(&self, i: &TSIndexedAccessType) -> Result<Type> {
        let obj = self.convert_ts_type(&i.object_type)?;
        let index = self.convert_ts_type(&i.index_type)?;
        Ok(Type::Index {
            obj: Box::new(obj),
            index: Box::new(index),
        })
    }

    fn convert_intersection_type(&self, i: &TSIntersectionType) -> Result<Type> {
        let types = i.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
        Ok(Type::Intersection { types })
    }

    fn convert_mapped_type(&self, m: &TSMappedType) -> Result<Type> {
        let from = self.convert_ts_type(&m.type_annotation)?;
        let to = self.convert_ts_type(&m.template_type)?;
        Ok(Type::Mapped {
            from: Box::new(from),
            to: Box::new(to),
        })
    }

    fn convert_optional_type(&self, o: &TSOptionalType) -> Result<Type> {
        self.convert_ts_type(&o.type_annotation)
    }

    fn convert_record_type(&self, r: &TSRecordType) -> Result<Type> {
        let key = self.convert_ts_type(&r.index_type)?;
        let value = self.convert_ts_type(&r.body_type)?;
        Ok(Type::Record {
            key: Box::new(key),
            value: Box::new(value),
        })
    }

    fn convert_reference_type(&self, r: &TSReferenceType) -> Result<Type> {
        let name = if let Some(id) = &r.type_name.get_identifier() {
            id.name.to_string()
        } else if let Some(qual) = r.type_name.getQualifiedName() {
            qual.to_string()
        } else {
            "Unknown".to_string()
        };
        let generics = r.type_parameters.as_ref().map_or(vec![], |tp| {
            tp.params.iter().filter_map(|p| self.convert_ts_type(p).ok()).collect()
        });
        Ok(Type::Ref { name, generics })
    }

    fn convert_template_literal_type(&self, t: &TSTemplateLiteralType) -> Result<Type> {
        let mut parts = Vec::new();
        let mut values = Vec::new();

        for quasi in &t.quasis {
            parts.push(TemplatePart::String { value: quasi.value.raw.to_string()) };
        }

        for type_ in &t.types {
            let ty = self.convert_ts_type(type_)?;
            values.push(ty);
        }

        Ok(Type::Template { parts, values })
    }

    fn convert_type_literal(&self, l: &TSTypeLiteralType) -> Result<Type> {
        let members = l.members.iter().filter_map(|m| self.convert_type_member(m)).collect();
        Ok(Type::Object { members })
    }

    fn convert_type_operator(&self, t: &TSTypeOperatorType) -> Result<Type> {
        match t.operator.kind {
            TSTypeOperatorKeyword::Keyof => {
                let inner = self.convert_ts_type(&t.type_annotation)?;
                Ok(Type::KeyOf { inner: Box::new(inner) })
            }
            TSTypeOperatorKeyword::Readonly => {
                let inner = self.convert_ts_type(&t.type_annotation)?;
                Ok(Type::Readonly { inner: Box::new(inner) })
            }
            TSTypeOperatorKeyword::Unique => Ok(Type::Symbol),
            _ => Ok(Type::Unknown),
        }
    }

    fn convert_union_type(&self, u: &TSUnionType) -> Result<Type> {
        let types = u.types.iter().map(|t| self.convert_ts_type(t)).collect::<Result<Vec<_>>>()?;
        Ok(Type::Union { types })
    }

    fn convert_type_member(&self, member: &TSTypeMember) -> Option<TypeMember> {
        match member {
            TSTypeMember::TSCallSignatureDeclaration(c) => self.convert_call_signature(c),
            TSTypeMember::TSConstructSignatureDeclaration(_) => self.convert_construct_signature(),
            TSTypeMember::TSPropertySignature(p) => self.convert_property_signature(p),
            TSTypeMember::TSMethodSignature(m) => self.convert_method_signature(m),
            TSTypeMember::TSIndexSignature(i) => self.convert_index_signature(i),
            _ => None,
        }
    }

    fn convert_call_signature(&self, c: &TSCallSignatureDeclaration) -> Option<TypeMember> {
        let params = c.params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(&c.return_type).ok()?;
        Some(TypeMember {
            key: "__call".to_string(),
            type_: Type::Function { params, ret: Box::new(ret) },
            optional: false,
            readonly: false,
        })
    }

    fn convert_construct_signature(&self) -> Option<TypeMember> {
        Some(TypeMember {
            key: "__construct".to_string(),
            type_: Type::Object { members: vec![] },
            optional: false,
            readonly: false,
        })
    }

    fn convert_property_signature(&self, p: &TSPropertySignature) -> Option<TypeMember> {
        let key = self.get_property_key(&p.key)?;
        let type_ = self.convert_ts_type(&p.type_annotation).ok()?;
        Some(TypeMember {
            key,
            type_,
            optional: p.optional,
            readonly: p.readonly,
        })
    }

    fn convert_method_signature(&self, m: &TSMethodSignature) -> Option<TypeMember> {
        let key = self.get_property_key(&m.key)?;
        let params = m.params.iter().filter_map(|p| self.convert_param_type(p)).collect();
        let ret = self.convert_ts_type(&m.return_type).ok()?;
        Some(TypeMember {
            key,
            type_: Type::Function { params, ret: Box::new(ret) },
            optional: m.optional,
            readonly: false,
        })
    }

    fn convert_index_signature(&self, i: &TSIndexSignature) -> Option<TypeMember> {
        let key_type = self.convert_ts_type(&i.key_type).ok()?;
        let value_type = self.convert_ts_type(&i.value_type).ok()?;
        Some(TypeMember {
            key: "__index".to_string(),
            type_: Type::Index {
                obj: Box::new(key_type),
                index: Box::new(value_type),
            },
            optional: false,
            readonly: false,
        })
    }

    fn get_property_key(&self, key: &PropertyKey) -> Option<String> {
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
            constraint: g.constraint.as_ref().and_then(|c| {
                type_parser.convert_ts_type(c).ok()
            }),
            default: g.default.as_ref().and_then(|d| {
                type_parser.convert_ts_type(d).ok()
            }),
        }
    }).collect();

    Ok(TypeDecl { name, generics, type_ })
}