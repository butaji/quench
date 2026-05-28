//! Type generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct TypeGen;

impl TypeGen {
    pub fn type_to_rust(cg: &CodeGenerator, type_: &Type) -> String {
        use Type::*;
        match type_ {
            Null => "()".to_string(),
            Undefined | Void => "()".to_string(),
            Boolean => "bool".to_string(),
            Number => "f64".to_string(),
            String => "String".to_string(),
            BigInt => "String".to_string(),
            Symbol => "String".to_string(),
            Array { elem } => format!("Vec<{}>", Self::type_to_rust(cg, elem)),
            Object { .. } => Self::object_to_struct(type_),
            Function { params, ret, generics: _ } => Self::function_sig(cg, params, ret),
            Ref { name, generics } => Self::ref_type(cg, name, generics),
            Union { types } => Self::union_type(cg, types),
            Intersection { types } => Self::intersection_type(cg, types),
            Tuple { types } => Self::tuple_type(cg, types),
            Index { .. } => "serde_json::Value".to_string(),
            Unknown => "serde_json::Value".to_string(),
            Literal { .. } => "serde_json::Value".to_string(),
            Never => "!".to_string(),
            Any => "serde_json::Value".to_string(),
            Paren { type_ } => Self::type_to_rust(cg, type_),
            Conditional { .. } => "serde_json::Value".to_string(),
            Mapped { .. } => "serde_json::Value".to_string(),
            Template { .. } => "String".to_string(),
        }
    }

    fn object_to_struct(type_: &Type) -> String {
        if let Type::Object { members } = type_ {
            let fields: Vec<String> = members.iter().map(|m| {
                let optional = if m.optional { "Option<" } else { "" };
                let end = if m.optional { ">" } else { "" };
                format!("pub {}: {}{}{}", m.key, optional, "serde_json::Value", end)
            }).collect();
            if fields.is_empty() {
                "serde_json::Value".to_string()
            } else {
                format!("impl {{ {} }}", fields.join(", "))
            }
        } else {
            "serde_json::Value".to_string()
        }
    }

    fn function_sig(cg: &CodeGenerator, params: &[Type], ret: &Box<Type>) -> String {
        let params_str = params.iter().map(|p| format!("_: {}", Self::type_to_rust(cg, p))).collect::<Vec<_>>().join(", ");
        let ret_str = Self::type_to_rust(cg, ret);
        format!("impl Fn({}) -> {}", params_str, ret_str)
    }

    fn ref_type(cg: &CodeGenerator, name: &str, generics: &[Type]) -> String {
        if generics.is_empty() {
            name.to_string()
        } else {
            let args = generics.iter().map(|g| Self::type_to_rust(cg, g)).collect::<Vec<_>>().join(", ");
            format!("{}<{}>", name, args)
        }
    }

    fn union_type(cg: &CodeGenerator, types: &[Type]) -> String {
        // Check for Option-like types: T | Null or T | Undefined
        if types.len() == 2 {
            let has_null = matches!(types[0], Type::Null) || matches!(types[1], Type::Null);
            let has_undef = matches!(types[0], Type::Undefined) || matches!(types[1], Type::Undefined);
            if has_null || has_undef {
                let inner = if matches!(&types[0], Type::Null | Type::Undefined) {
                    &types[1]
                } else {
                    &types[0]
                };
                return format!("Option<{}>", Self::type_to_rust(cg, inner));
            }
        }
        let variants: Vec<String> = types.iter().map(|t| Self::type_to_rust(cg, t)).collect();
        variants.join(" | ")
    }

    fn intersection_type(cg: &CodeGenerator, types: &[Type]) -> String {
        let parts: Vec<String> = types.iter().map(|t| Self::type_to_rust(cg, t)).collect();
        parts.join(" + ")
    }

    fn tuple_type(cg: &CodeGenerator, types: &[Type]) -> String {
        let elems: Vec<String> = types.iter().map(|t| Self::type_to_rust(cg, t)).collect();
        format!("({})", elems.join(", "))
    }
}
