//! Shared type-to-Rust conversion logic
//!
//! Provides a single source of truth for converting HIR types to Rust types.
//! Used by both String-based (type_gen.rs) and TokenStream-based (quote_codegen.rs) code generation.
//!

use crate::transpile::hir::{LiteralKind, ObjectMemberExpr, ObjectProp, Type, TypeMember};

/// Kind of Rust type output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// Generate Rust source code as String
    String,
    /// Generate proc_macro2::TokenStream
    TokenStream,
}

/// Shared type conversion logic
pub struct TypeToRust {
    pub kind: OutputKind,
}

impl TypeToRust {
    pub fn new(kind: OutputKind) -> Self {
        Self { kind }
    }

    pub fn convert(&self, ty: &Type) -> RustType {
        use Type::*;
        match ty {
            String | Number | Boolean | Void | Never => self.convert_prim2(ty),
            Unknown | Any | Null | Undefined | Query(..) | Infer(..) => RustType::Value,
            BigInt | Symbol | This => self.convert_prim3(ty),
            Array { elem } | Ref { name: _, generics: _ } | Object { members: _ } => self.convert_structlike(ty),
            Union { types } | Intersection { types } | Function { params: _, ret: _ } | Literal { kind: _, value: _ } | Template { parts: _, values: _ } => self.convert_multi(types, ty),
            Index { .. } | Mapped { .. } | Tuple { elements: _ } => self.convert_iterable(ty),
            Conditional { .. } | ReturnType { .. } | Parameters { .. } => self.convert_ty_wrapper(ty),
            Partial { inner } | Required { inner } | Readonly { inner } | Pick { inner: _, keys: _ } | Omit { inner: _, keys: _ } | Record { key: _, value: _ } | KeyOf { inner: _ } => self.convert_wrap_pick2(ty),
        }
    }

    fn convert_prim2(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::String => self.convert_primitive("String"), Type::Number => self.convert_primitive("f64"), Type::Boolean => self.convert_primitive("bool"), Type::Void => self.convert_primitive("()"), Type::Never => self.convert_primitive("!"), _ => RustType::Value } }
    fn convert_prim3(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::BigInt => self.convert_primitive("i64"), Type::Symbol => self.convert_primitive("std::sync::Arc<std::fmt::Debug>"), Type::This => self.convert_primitive("Self"), _ => RustType::Value } }
    fn convert_structlike(&self, ty: &Type) -> RustType { use Type::*; match ty { Array { elem } => self.convert_array(elem), Ref { name, generics } => self.convert_ref(name, generics), Object { members } => self.convert_object(members), _ => RustType::Value } }
    fn convert_multi(&self, types: &[Type], ty: &Type) -> RustType { use Type::*; match ty { Union { types } | Intersection { types } => self.convert_union(types), Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_wrap_pick(&self, ty: &Type) -> RustType { use Type::*; match ty { Partial { inner } | Required { inner } | Readonly { inner } => self.convert(inner), Pick { inner, keys } | Omit { inner, keys } => self.convert_pick(inner, keys), Record { key, value } | KeyOf { inner: value } => self.convert_key_value(value, key.as_ref()), _ => RustType::Value } }
    fn convert_wrap_pick2(&self, ty: &Type) -> RustType { self.convert_wrap_pick(ty) }

    fn convert_prim2(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::String => self.convert_primitive("String"), Type::Number => self.convert_primitive("f64"), Type::Boolean => self.convert_primitive("bool"), Type::Void => self.convert_primitive("()"), Type::Never => self.convert_primitive("!"), _ => RustType::Value } }
    fn convert_prim3(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::BigInt => self.convert_primitive("i64"), Type::Symbol => self.convert_primitive("std::sync::Arc<std::fmt::Debug>"), Type::This => self.convert_primitive("Self"), _ => RustType::Value } }
    fn convert_structlike(&self, ty: &Type) -> RustType { use Type::*; match ty { Array { elem } => self.convert_array(elem), Ref { name, generics } => self.convert_ref(name, generics), Object { members } => self.convert_object(members), _ => RustType::Value } }
    fn convert_multi(&self, types: &[Type], ty: &Type) -> RustType { use Type::*; match ty { Union { types } | Intersection { types } => self.convert_union(types), Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrap_pick(&self, ty: &Type) -> RustType { use Type::*; match ty { Partial { inner } | Required { inner } | Readonly { inner } => self.convert(inner), Pick { inner, keys } | Omit { inner, keys } => self.convert_pick(inner, keys), _ => RustType::Value } }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_iterable(&self, ty: &Type) -> RustType { use Type::*; match ty { Index { .. } | Mapped { .. } => self.convert_index_mapped(ty), Tuple { elements } => self.convert_tuple(elements), _ => RustType::Value } }

    fn convert_prim2(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::String => self.convert_primitive("String"), Type::Number => self.convert_primitive("f64"), Type::Boolean => self.convert_primitive("bool"), Type::Void => self.convert_primitive("()"), Type::Never => self.convert_primitive("!"), _ => RustType::Value } }
    fn convert_prim3(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::BigInt => self.convert_primitive("i64"), Type::Symbol => self.convert_primitive("std::sync::Arc<std::fmt::Debug>"), Type::This => self.convert_primitive("Self"), _ => RustType::Value } }
    fn convert_structlike(&self, ty: &Type) -> RustType { use Type::*; match ty { Array { elem } => self.convert_array(elem), Ref { name, generics } => self.convert_ref(name, generics), Object { members } => self.convert_object(members), _ => RustType::Value } }
    fn convert_multi(&self, types: &[Type], ty: &Type) -> RustType { use Type::*; match ty { Union { types } | Intersection { types } => self.convert_union(types), Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_iterable(&self, ty: &Type) -> RustType { use Type::*; match ty { Index { .. } | Mapped { .. } => self.convert_index_mapped(ty), Tuple { elements } => self.convert_tuple(elements), _ => RustType::Value } }

    fn convert_prim2(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::String => self.convert_primitive("String"), Type::Number => self.convert_primitive("f64"), Type::Boolean => self.convert_primitive("bool"), Type::Void => self.convert_primitive("()"), Type::Never => self.convert_primitive("!"), _ => RustType::Value } }
    fn convert_prim3(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::BigInt => self.convert_primitive("i64"), Type::Symbol => self.convert_primitive("std::sync::Arc<std::fmt::Debug>"), Type::This => self.convert_primitive("Self"), _ => RustType::Value } }
    fn convert_structlike(&self, ty: &Type) -> RustType { use Type::*; match ty { Array { elem } => self.convert_array(elem), Ref { name, generics } => self.convert_ref(name, generics), Object { members } => self.convert_object(members), _ => RustType::Value } }
    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_iterable(&self, ty: &Type) -> RustType { use Type::*; match ty { Index { .. } | Mapped { .. } => self.convert_index_mapped(ty), Tuple { elements } => self.convert_tuple(elements), _ => RustType::Value } }

    fn convert_prim2(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::String => self.convert_primitive("String"), Type::Number => self.convert_primitive("f64"), Type::Boolean => self.convert_primitive("bool"), Type::Void => self.convert_primitive("()"), Type::Never => self.convert_primitive("!"), _ => RustType::Value } }
    fn convert_prim3(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::BigInt => self.convert_primitive("i64"), Type::Symbol => self.convert_primitive("std::sync::Arc<std::fmt::Debug>"), Type::This => self.convert_primitive("Self"), _ => RustType::Value } }
    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_iterable(&self, ty: &Type) -> RustType { use Type::*; match ty { Index { .. } | Mapped { .. } => self.convert_index_mapped(ty), Tuple { elements } => self.convert_tuple(elements), _ => RustType::Value } }

    fn convert_prim2(&self, ty: &Type) -> RustType { use Type::*; match ty { Type::String => self.convert_primitive("String"), Type::Number => self.convert_primitive("f64"), Type::Boolean => self.convert_primitive("bool"), Type::Void => self.convert_primitive("()"), Type::Never => self.convert_primitive("!"), _ => RustType::Value } }
    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_iterable(&self, ty: &Type) -> RustType { use Type::*; match ty { Index { .. } | Mapped { .. } => self.convert_index_mapped(ty), Tuple { elements } => self.convert_tuple(elements), _ => RustType::Value } }

    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }
    fn convert_iterable(&self, ty: &Type) -> RustType { use Type::*; match ty { Index { .. } | Mapped { .. } => self.convert_index_mapped(ty), Tuple { elements } => self.convert_tuple(elements), _ => RustType::Value } }

    fn convert_union_intersection(&self, types: &[Type]) -> RustType { self.convert_union(types) }
    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }
    fn convert_complex(&self, ty: &Type) -> RustType { use Type::*; match ty { Function { params, ret } => self.convert_function(params, ret), Literal { kind, value } => self.convert_literal(kind, value), Template { parts, values } => self.convert_template(parts, values), _ => RustType::Value } }

    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }
    fn convert_ty_wrapper(&self, ty: &Type) -> RustType { use Type::*; match ty { ReturnType { inner } => self.convert_return_type(inner), Parameters { inner } => self.convert_parameters(inner), Conditional { .. } => self.convert_conditional(ty), _ => RustType::Value } }

    fn convert_wrapper(&self, inner: &Type) -> RustType { self.convert(inner) }
    fn convert_pick_omit(&self, inner: &Type, keys: &[String]) -> RustType { self.convert_pick(inner, keys) }
    fn convert_key_value(&self, value: &Type, key: Option<&Type>) -> RustType { match key { Some(k) => self.convert_record(k, value), None => self.convert_keyof(value) } }

    fn convert_primitive(&self, s: &str) -> RustType { RustType::Primitive(s.into()) }
    fn convert_array(&self, elem: &Type) -> RustType { RustType::Vec(Box::new(self.convert(elem))) }
    fn convert_ref(&self, name: &str, generics: &[Type]) -> RustType { if generics.is_empty() { RustType::Named(name.clone()) } else { let gs: Vec<_> = generics.iter().map(|g| self.convert(g)).collect(); RustType::Generic(name.clone(), gs) } }
    fn convert_object(&self, members: &[TypeMember]) -> RustType { if members.is_empty() { RustType::Value } else { RustType::Struct(members.iter().map(|m| self.convert_member(m)).collect()) } }
    fn convert_function(&self, params: &[Type], ret: &Type) -> RustType { let ps: Vec<_> = params.iter().map(|p| self.convert(p)).collect(); RustType::Fn(ps, Box::new(self.convert(ret))) }
    fn convert_index_mapped(&self, ty: &Type) -> RustType { match ty { Type::Index { obj, index } | Type::Mapped { from: obj, to: index } => { let obj_t = self.convert(obj); let index_t = self.convert(index); RustType::HashMap(Box::new(obj_t), Box::new(index_t)) } _ => RustType::Value } }

    fn convert_conditional(&self, ty: &Type) -> RustType {
        if let Type::Conditional { check, extends, true_type, false_type } = ty {
            let check_t = self.convert(check);
            let extends_t = self.convert(extends);
            let true_t = self.convert(true_type);
            let false_t = self.convert(false_type);
            RustType::Conditional {
                check: Box::new(check_t),
                extends: Box::new(extends_t),
                true_type: Box::new(true_t),
                false_type: Box::new(false_t),
            }
        } else {
            RustType::Value
        }
    }

    fn convert_partial(&self, inner: &Type) -> RustType {
        match inner {
            Type::Object { members } => {
                let partial_fields: Vec<RustStructField> = members.iter()
                    .map(|m| RustStructField {
                        name: m.key.clone(),
                        ty: self.make_optional(&m.type_),
                        optional: true,
                    })
                    .collect();
                RustType::Struct(partial_fields)
            }
            _ => RustType::Value,
        }
    }

    fn make_optional(&self, ty: &Type) -> RustType {
        let inner = self.convert(ty);
        RustType::Option(Box::new(inner))
    }

    fn convert_required(&self, inner: &Type) -> RustType {
        match inner {
            Type::Object { members } => {
                let required_fields: Vec<RustStructField> = members.iter()
                    .map(|m| RustStructField {
                        name: m.key.clone(),
                        ty: self.unwrap_option(&m.type_),
                        optional: false,
                    })
                    .collect();
                RustType::Struct(required_fields)
            }
            _ => self.convert(inner),
        }
    }

    fn unwrap_option(&self, ty: &Type) -> RustType {
        match ty {
            Type::Ref { name, generics } if name == "Option" => {
                if let Some(inner) = generics.first() {
                    self.convert(inner)
                } else {
                    self.convert(ty)
                }
            }
            _ => self.convert(ty),
        }
    }

    fn convert_pick(&self, inner: &Type, keys: &[String]) -> RustType {
        match inner {
            Type::Object { members } => {
                let picked_fields: Vec<RustStructField> = members.iter()
                    .filter(|m| keys.contains(&m.key))
                    .map(|m| self.convert_member(m))
                    .collect();
                if picked_fields.is_empty() {
                    RustType::Value
                } else {
                    RustType::Struct(picked_fields)
                }
            }
            _ => RustType::Value,
        }
    }

    fn convert_omit(&self, inner: &Type, keys: &[String]) -> RustType {
        match inner {
            Type::Object { members } => {
                let omitted_fields: Vec<RustStructField> = members.iter()
                    .filter(|m| !keys.contains(&m.key))
                    .map(|m| self.convert_member(m))
                    .collect();
                if omitted_fields.is_empty() {
                    RustType::Value
                } else {
                    RustType::Struct(omitted_fields)
                }
            }
            _ => RustType::Value,
        }
    }

    fn convert_record(&self, key: &Type, value: &Type) -> RustType {
        let key_t = self.convert(key);
        let value_t = self.convert(value);
        RustType::HashMap(Box::new(key_t), Box::new(value_t))
    }

    fn convert_keyof(&self, inner: &Type) -> RustType {
        match inner {
            Type::Object { members } => {
                let variants: Vec<RustType> = members.iter()
                    .map(|m| RustType::StringLiteral(m.key.clone()))
                    .collect();
                RustType::Enum(variants)
            }
            _ => RustType::Value,
        }
    }

    fn convert_return_type(&self, inner: &Type) -> RustType {
        match inner {
            Type::Function { params: _, ret } => self.convert(ret),
            _ => RustType::Value,
        }
    }

    fn convert_parameters(&self, inner: &Type) -> RustType {
        match inner {
            Type::Function { params, ret: _ } => {
                let param_types: Vec<RustType> = params.iter()
                    .map(|p| self.convert(p))
                    .collect();
                RustType::Tuple(param_types)
            }
            _ => RustType::Value,
        }
    }

    fn convert_tuple(&self, elements: &[crate::transpile::hir::TypeTupleElement]) -> RustType {
        let types: Vec<_> = elements.iter().map(|e| self.convert(&e.type_)).collect();
        RustType::Tuple(types)
    }

    fn convert_member(&self, member: &TypeMember) -> RustStructField {
        RustStructField {
            name: member.key.clone(),
            ty: self.convert(&member.type_),
            optional: member.optional,
        }
    }

    fn convert_union(&self, types: &[Type]) -> RustType {
        if types.is_empty() {
            return RustType::Value;
        }
        let variants: Vec<RustType> = types.iter()
            .enumerate()
            .map(|(i, t)| self.convert_union_variant(i, t))
            .collect();
        RustType::Enum(variants)
    }

    fn convert_union_variant(&self, index: usize, ty: &Type) -> RustType {
        match ty {
            Type::Ref { name, generics } => {
                if generics.is_empty() {
                    RustType::Variant(name.clone(), vec![])
                } else {
                    let gs: Vec<_> = generics.iter().map(|g| self.convert(g)).collect();
                    RustType::Variant(name.clone(), gs)
                }
            }
            Type::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .map(|m| self.convert_member(m))
                    .collect();
                RustType::VariantStruct { index, fields }
            }
            Type::Literal { kind, value } => {
                let variant_name = format!("{:?}{}", kind, value);
                RustType::Variant(variant_name, vec![])
            }
            _ => RustType::Variant(format!("Variant{}", index), vec![]),
        }
    }

    fn convert_intersection(&self, types: &[Type]) -> RustType {
        if types.is_empty() {
            return RustType::Value;
        }
        let all_fields: Vec<RustStructField> = types.iter()
            .filter_map(|t| {
                if let Type::Object { members } = t {
                    Some(members.iter().map(|m| self.convert_member(m)).collect::<Vec<_>>())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        if all_fields.is_empty() {
            let type_strs: Vec<_> = types.iter().map(|t| self.convert(t)).collect();
            RustType::Tuple(type_strs)
        } else {
            RustType::Struct(all_fields)
        }
    }

    fn convert_literal(&self, kind: &LiteralKind, value: &str) -> RustType {
        match kind {
            LiteralKind::String => RustType::StringLiteral(value.to_string()),
            LiteralKind::Number => {
                if let Ok(n) = value.parse::<f64>() {
                    RustType::NumberLiteral(n)
                } else {
                    RustType::Value
                }
            }
            LiteralKind::Boolean => {
                if value == "true" {
                    RustType::BoolLiteral(true)
                } else {
                    RustType::BoolLiteral(false)
                }
            }
            LiteralKind::BigInt => {
                if let Ok(n) = value.parse::<i64>() {
                    RustType::IntLiteral(n)
                } else {
                    RustType::Value
                }
            }
        }
    }

    fn convert_template(&self, parts: &[crate::transpile::hir::TemplatePart], values: &[Type]) -> RustType {
        let mut has_dynamic = false;
        let mut result = String::new();

        for (i, part) in parts.iter().enumerate() {
            match part {
                crate::transpile::hir::TemplatePart::String { value: s } => result.push_str(s),
                crate::transpile::hir::TemplatePart::Type { value: _ } => has_dynamic = true,
            }
            if i < values.len() {
                let val_t = self.convert(&values[i]);
                if !matches!(val_t, RustType::StringLiteral(_)) {
                    has_dynamic = true;
                }
            }
        }

        if has_dynamic {
            RustType::Value
        } else {
            RustType::StringLiteral(result)
        }
    }
}
