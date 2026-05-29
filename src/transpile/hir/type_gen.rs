//! Type code generation
//! 
//! Converts HIR Type to Rust type strings

use super::{Expr, Stmt, Type};

/// Type code generator
pub struct TypeGen;

impl TypeGen {
    /// Generate Rust type from HIR Type
    pub fn gen_type(&self, ty: &Type) -> String {
        use super::Type as T;
        match ty {
            T::String | T::Number | T::Boolean | T::Void | T::Never | T::Unknown | T::BigInt | T::Any => {
                self.gen_type_prim(ty)
            }
            _ => self.gen_type_complex(ty),
        }
    }
    
    fn gen_type_prim(&self, ty: &Type) -> String {
        use super::Type as T;
        match ty {
            T::String | T::Number | T::Boolean => self.gen_type_num(ty),
            T::Void | T::Never | T::Unknown => self.gen_type_void(ty),
            T::BigInt | T::Any => self.gen_type_big(ty),
            _ => "Value".to_string(),
        }
    }
    
    fn gen_type_num(&self, ty: &Type) -> String {
        use super::Type as T;
        match ty {
            T::String => "String".to_string(),
            T::Number => "f64".to_string(),
            T::Boolean => "bool".to_string(),
            _ => "Value".to_string(),
        }
    }
    
    fn gen_type_void(&self, ty: &Type) -> String {
        use super::Type as T;
        match ty {
            T::Void => "()".to_string(),
            T::Never => "!".to_string(),
            T::Unknown => "Value".to_string(),
            _ => "Value".to_string(),
        }
    }
    
    fn gen_type_big(&self, ty: &Type) -> String {
        use super::Type as T;
        match ty {
            T::BigInt => "i64".to_string(),
            T::Any => "serde_json::Value".to_string(),
            _ => "Value".to_string(),
        }
    }
    
    fn gen_type_complex(&self, ty: &Type) -> String {
        use super::Type as T;
        match ty {
            T::Array { elem } => format!("Vec<{}>", self.gen_type(elem)),
            T::Object { members } if members.is_empty() => "serde_json::Value".to_string(),
            T::Object { members } => self.gen_inline_object(members),
            T::Function { params, ret } => self.gen_fn_type(params, ret),
            T::Ref { name, generics } => self.gen_ref(name, generics),
            _ => "Value".to_string(),
        }
    }
    
    fn gen_inline_object(&self, members: &[super::TypeMember]) -> String {
        let fs: Vec<String> = members
            .iter()
            .map(|m| format!("{}: {}", m.key, self.gen_type(&m.type_)))
            .collect();
        format!("{{ {} }}", fs.join(", "))
    }
    
    fn gen_fn_type(&self, params: &[Type], ret: &Box<Type>) -> String {
        let ps: Vec<String> = params.iter().map(|p| self.gen_type(p)).collect();
        format!("fn({}) -> {}", ps.join(", "), self.gen_type(ret))
    }
    
    fn gen_ref(&self, name: &str, generics: &[Type]) -> String {
        if generics.is_empty() {
            name.to_string()
        } else {
            let gs: Vec<String> = generics.iter().map(|g| self.gen_type(g)).collect();
            format!("{}<{}>", name, gs.join(", "))
        }
    }
}

impl Default for TypeGen {
    fn default() -> Self {
        Self
    }
}
