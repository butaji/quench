//! # TypeScript Type Inference
//!
//! Infers Rust types from TypeScript type annotations.

use swc_ecma_ast::*;
use crate::analyzer::TypeInfo;

/// Infers type from a TypeScript type annotation.
pub fn infer_ts_type(ts_type: &TsType) -> TypeInfo {
    match ts_type {
        TsType::TsKeywordType(k) => infer_keyword(k),
        TsType::TsArrayType(a) => TypeInfo::Array(Box::new(infer_ts_type(&a.elem_type))),
        TsType::TsUnionOrIntersectionType(t) => {
            if t.ts_type_union.is_some() {
                TypeInfo::Unknown
            } else {
                TypeInfo::Unknown
            }
        }
        TsType::TsTypeRef(t) => infer_type_ref(t),
        TsType::TsLiteralType(l) => infer_literal(l),
        TsType::TsTupleType(t) => infer_tuple(t),
        TsType::TsParenthesizedType(p) => infer_ts_type(&p.type_ann),
        _ => TypeInfo::Unknown,
    }
}

/// Infers type from a keyword type.
fn infer_keyword(k: &TsKeywordType) -> TypeInfo {
    match k.kind {
        TsKeywordTypeKind::TsNumberKeyword => TypeInfo::Float,
        TsKeywordTypeKind::TsStringKeyword => TypeInfo::String,
        TsKeywordTypeKind::TsBooleanKeyword => TypeInfo::Boolean,
        TsKeywordTypeKind::TsNullKeyword => TypeInfo::Unknown,
        TsKeywordTypeKind::TsUndefinedKeyword => TypeInfo::Unknown,
        TsKeywordTypeKind::TsVoidKeyword => TypeInfo::Unknown,
        TsKeywordTypeKind::TsAnyType | TsKeywordTypeKind::TsUnknownType => TypeInfo::Unknown,
        _ => TypeInfo::Unknown,
    }
}

/// Infers type from a type reference.
fn infer_type_ref(t: &TsTypeRef) -> TypeInfo {
    let name = t.type_name.as_str();
    match name {
        "Array" | "Vec" => {
            if let Some(params) = &t.type_params {
                if !params.params.is_empty() {
                    let inner = infer_ts_type(&params.params[0]);
                    return TypeInfo::Array(Box::new(inner));
                }
            }
            TypeInfo::Array(Box::new(TypeInfo::Unknown))
        }
        "Option" => {
            if let Some(params) = &t.type_params {
                if !params.params.is_empty() {
                    let inner = infer_ts_type(&params.params[0]);
                    return TypeInfo::Option(Box::new(inner));
                }
            }
            TypeInfo::Option(Box::new(TypeInfo::Unknown))
        }
        "Result" => {
            if let Some(params) = &t.type_params {
                if params.params.len() >= 2 {
                    let ok = infer_ts_type(&params.params[0]);
                    let err = infer_ts_type(&params.params[1]);
                    return TypeInfo::Result(Box::new(ok), Box::new(err));
                }
            }
            TypeInfo::Result(Box::new(TypeInfo::Unknown), Box::new(TypeInfo::Unknown))
        }
        _ => TypeInfo::Unknown,
    }
}

/// Infers type from a literal type.
fn infer_literal(l: &TsLitType) -> TypeInfo {
    match &l.lit {
        TsLit::Str(s) => TypeInfo::StringLiteral(s.value.to_string()),
        TsLit::Num(n) => {
            if n.value.fract() == 0.0 {
                TypeInfo::Integer(n.value as i64)
            } else {
                TypeInfo::Float
            }
        }
        TsLit::BigInt(_) => TypeInfo::Integer(0),
        TsLit::Boolean(b) => TypeInfo::Boolean,
    }
}

/// Infers type from a tuple type.
fn infer_tuple(t: &TsTupleType) -> TypeInfo {
    let fields: Vec<(String, TypeInfo)> = t.elem_types.iter()
        .enumerate()
        .map(|(i, e)| (format!("_{}", i), infer_ts_type(&e.ty)))
        .collect();
    TypeInfo::Struct(crate::analyzer::StructInfo {
        name: String::new(),
        fields,
    })
}
