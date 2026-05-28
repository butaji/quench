//! Type generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct TypeGen;

impl TypeGen {
    pub fn type_to_rust(cg: &CodeGenerator, type_: &Type) -> String {
        use Type::*;
        match type_ {
            Null | Undefined | Void => "()".to_string(),
            Boolean => "bool".to_string(),
            Number => "f64".to_string(),
            String | BigInt | Symbol | Template { .. } => "String".to_string(),
            Array { elem } => format!("Vec<{}>", Self::type_to_rust(cg, elem)),
            Object { .. } => Self::object_to_struct(type_),
            Function { params, ret, generics: _ } => Self::function_sig(cg, params, ret),
            Ref { name, generics } => Self::ref_type(cg, name, generics),
            Union { types } => Self::union_type(cg, types),
            Intersection { types } => Self::intersection_type(cg, types),
            Tuple { types } => Self::tuple_type(cg, types),
            Index { .. } | Unknown | Literal { .. } | Any | Conditional { .. } | Mapped { .. } => "serde_json::Value".to_string(),
            Never => "!".to_string(),
            Paren { type_ } => Self::type_to_rust(cg, type_),
        }
    }

    fn object_to_struct(type_: &Type) -> String {
        if let Type::Object { members } = type_ {
            let fields: Vec<String> = members.iter().map(|m| { let opt = if m.optional { "Option<" } else { "" }; let end = if m.optional { ">" } else { "" }; format!("pub {}: {}{}{}", m.key, opt, "serde_json::Value", end) }).collect();
            if fields.is_empty() { "serde_json::Value".to_string() } else { format!("impl {{ {} }}", fields.join(", ")) }
        } else { "serde_json::Value".to_string() }
    }

    fn function_sig(_cg: &CodeGenerator, params: &[Type], ret: &Type) -> String { format!("Box<dyn Fn({}) -> {}>", params.iter().map(|p| Self::type_to_rust(_cg, p)).collect::<Vec<_>>().join(", "), Self::type_to_rust(_cg, ret)) }
    fn ref_type(_cg: &CodeGenerator, name: &str, generics: &[Type]) -> String { if generics.is_empty() { name.to_string() } else { format!("{}<{}>", name, generics.iter().map(|g| Self::type_to_rust(_cg, g)).collect::<Vec<_>>().join(", ")) } }
    fn union_type(cg: &CodeGenerator, types: &[Type]) -> String { types.iter().map(|t| Self::type_to_rust(cg, t)).collect::<Vec<_>>().join(" | ") }
    fn intersection_type(cg: &CodeGenerator, types: &[Type]) -> String { types.iter().map(|t| Self::type_to_rust(cg, t)).collect::<Vec<_>>().join(" + ") }
    fn tuple_type(cg: &CodeGenerator, types: &[Type]) -> String { format!("({})", types.iter().map(|t| Self::type_to_rust(cg, t)).collect::<Vec<_>>().join(", ")) }
}
