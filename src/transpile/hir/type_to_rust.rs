//! Shared type-to-Rust conversion logic
//!
//! Provides a single source of truth for converting HIR types to Rust types.
//! Used by both String-based (type_gen.rs) and TokenStream-based (quote_codegen.rs) code generation.
//!
//! allow:complexity,too_many_lines

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

    // allow:complexity,too_many_lines
    pub fn convert(&self, ty: &Type) -> RustType {
        match ty {
            Type::String => RustType::Primitive("String".into()),
            Type::Number => RustType::Primitive("f64".into()),
            Type::Boolean => RustType::Primitive("bool".into()),
            Type::Void => RustType::Primitive("()".into()),
            Type::Never => RustType::Primitive("!".into()),
            Type::Unknown => RustType::Value,
            Type::Any => RustType::Value,
            Type::BigInt => RustType::Primitive("i64".into()),
            Type::Symbol => RustType::Primitive("std::sync::Arc<std::fmt::Debug>".into()),
            Type::This => RustType::Primitive("Self".into()),
            Type::Null => RustType::Value,
            Type::Undefined => RustType::Value,
            Type::Array { elem } => {
                let inner = self.convert(elem);
                RustType::Vec(Box::new(inner))
            }
            Type::Ref { name, generics } => {
                if generics.is_empty() {
                    RustType::Named(name.clone())
                } else {
                    let gs: Vec<_> = generics.iter().map(|g| self.convert(g)).collect();
                    RustType::Generic(name.clone(), gs)
                }
            }
            Type::Object { members } => {
                if members.is_empty() {
                    RustType::Value
                } else {
                    RustType::Struct(members.iter().map(|m| self.convert_member(m)).collect())
                }
            }
            Type::Union { types } => self.convert_union(types),
            Type::Intersection { types } => self.convert_intersection(types),
            Type::Function { params, ret } => {
                let ps: Vec<_> = params.iter().map(|p| self.convert(p)).collect();
                let r = self.convert(ret);
                RustType::Fn(ps, Box::new(r))
            }
            Type::Literal { kind, value } => self.convert_literal(kind, value),
            Type::Template { parts, values } => self.convert_template(parts, values),
            Type::Index { obj, index } => {
                let obj_t = self.convert(obj);
                let index_t = self.convert(index);
                RustType::HashMap(Box::new(obj_t), Box::new(index_t))
            }
            Type::Mapped { from, to } => {
                let from_t = self.convert(from);
                let to_t = self.convert(to);
                RustType::HashMap(Box::new(from_t), Box::new(to_t))
            }
            Type::Conditional { check, extends, true_type, false_type } => {
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
            }
            Type::Query { expr } => {
                let _ = expr;
                RustType::Value
            }
            Type::Infer { name } => {
                let _ = name;
                RustType::Value
            }
            Type::Partial { inner } => self.convert_partial(inner),
            Type::Required { inner } => self.convert_required(inner),
            Type::Pick { inner, keys } => self.convert_pick(inner, keys),
            Type::Omit { inner, keys } => self.convert_omit(inner, keys),
            Type::Record { key, value } => {
                let key_t = self.convert(key);
                let value_t = self.convert(value);
                RustType::HashMap(Box::new(key_t), Box::new(value_t))
            }
            Type::KeyOf { inner } => self.convert_keyof(inner),
            Type::ReturnType { inner } => self.convert_return_type(inner),
            Type::Parameters { inner } => self.convert_parameters(inner),
            Type::Readonly { inner } => self.convert(inner),
            Type::Tuple { elements } => {
                let types: Vec<_> = elements.iter()
                    .map(|e| self.convert(&e.type_))
                    .collect();
                RustType::Tuple(types)
            }
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

/// Represents a converted Rust type
#[derive(Debug, Clone, PartialEq)]
pub enum RustType {
    /// Primitive type (f64, bool, String, etc.)
    Primitive(String),
    /// A named type (MyStruct, MyEnum)
    Named(String),
    /// A generic type (Vec<T>, Option<T>)
    Generic(String, Vec<RustType>),
    /// A struct with named fields
    Struct(Vec<RustStructField>),
    /// A vector type
    Vec(Box<RustType>),
    /// A function type
    Fn(Vec<RustType>, Box<RustType>),
    /// A hashmap type
    HashMap(Box<RustType>, Box<RustType>),
    /// An enum with variants
    Enum(Vec<RustType>),
    /// A variant (for discriminated unions)
    Variant(String, Vec<RustType>),
    /// A variant with struct fields
    VariantStruct { index: usize, fields: Vec<RustStructField> },
    /// A tuple type
    Tuple(Vec<RustType>),
    /// A conditional type
    Conditional {
        check: Box<RustType>,
        extends: Box<RustType>,
        true_type: Box<RustType>,
        false_type: Box<RustType>,
    },
    /// Fallback to Value
    Value,
    /// String literal type
    StringLiteral(String),
    /// Number literal type
    NumberLiteral(f64),
    /// Boolean literal type
    BoolLiteral(bool),
    /// Integer literal type
    IntLiteral(i64),
    /// Optional type
    Option(Box<RustType>),
}

impl RustType {
    // allow:complexity,too_many_lines
    pub fn type_name(&self) -> String {
        match self {
            RustType::Primitive(s) => s.clone(),
            RustType::Named(s) => s.clone(),
            RustType::Generic(name, args) => {
                let args_str: Vec<_> = args.iter().map(|a| a.type_name()).collect();
                format!("{}<{}>", name, args_str.join(", "))
            }
            RustType::Struct(fields) => {
                let fields_str: Vec<_> = fields.iter()
                    .map(|f| format!("{}: {}", f.name, f.ty.type_name()))
                    .collect();
                format!("{{ {} }}", fields_str.join(", "))
            }
            RustType::Vec(inner) => format!("Vec<{}>", inner.type_name()),
            RustType::Fn(params, ret) => {
                let params_str: Vec<_> = params.iter().map(|p| p.type_name()).collect();
                format!("fn({}) -> {}", params_str.join(", "), ret.type_name())
            }
            RustType::HashMap(k, v) => format!("std::collections::HashMap<{}, {}>", k.type_name(), v.type_name()),
            RustType::Enum(variants) => {
                let var_strs: Vec<_> = variants.iter().map(|v| v.type_name()).collect();
                format!("enum {{ {} }}", var_strs.join(", "))
            }
            RustType::Variant(name, args) => {
                if args.is_empty() {
                    name.clone()
                } else {
                    let args_str: Vec<_> = args.iter().map(|a| a.type_name()).collect();
                    format!("{}({})", name, args_str.join(", "))
                }
            }
            RustType::VariantStruct { index, fields } => {
                let fields_str: Vec<_> = fields.iter()
                    .map(|f| format!("{}: {}", f.name, f.ty.type_name()))
                    .collect();
                format!("Variant{}{{{}}}", index, fields_str.join(", "))
            }
            RustType::Tuple(types) => {
                let types_str: Vec<_> = types.iter().map(|t| t.type_name()).collect();
                format!("({})", types_str.join(", "))
            }
            RustType::Conditional { .. } => "Value".to_string(),
            RustType::Value => "Value".to_string(),
            RustType::StringLiteral(s) => format!("\"{}\"", s),
            RustType::NumberLiteral(n) => n.to_string(),
            RustType::BoolLiteral(b) => b.to_string(),
            RustType::IntLiteral(n) => format!("{}i64", n),
            RustType::Option(inner) => format!("Option<{}>", inner.type_name()),
        }
    }
}

/// A struct field in a converted Rust type
#[derive(Debug, Clone, PartialEq)]
pub struct RustStructField {
    pub name: String,
    pub ty: RustType,
    pub optional: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_primitives() {
        let converter = TypeToRust::new(OutputKind::String);

        assert_eq!(converter.convert(&Type::String).type_name(), "String");
        assert_eq!(converter.convert(&Type::Number).type_name(), "f64");
        assert_eq!(converter.convert(&Type::Boolean).type_name(), "bool");
    }

    #[test]
    fn test_convert_ref_type() {
        let converter = TypeToRust::new(OutputKind::String);

        assert_eq!(
            converter.convert(&Type::Ref { name: "MyType".into(), generics: vec![] }).type_name(),
            "MyType"
        );
        assert_eq!(
            converter.convert(&Type::Ref { name: "Result".into(), generics: vec![Type::String] }).type_name(),
            "Result<String>"
        );
    }

    #[test]
    fn test_convert_array() {
        let converter = TypeToRust::new(OutputKind::String);

        assert_eq!(
            converter.convert(&Type::Array { elem: Box::new(Type::Number) }).type_name(),
            "Vec<f64>"
        );
    }
}
