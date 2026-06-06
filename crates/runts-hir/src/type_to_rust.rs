//! Shared type-to-Rust conversion logic
//!
//! Provides a single source of truth for converting HIR types to Rust types.

use crate::{LiteralKind, Type, TypeMember};

/// Kind of Rust type output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    String,
    TokenStream,
}

/// The target Rust type representation
#[derive(Debug, Clone)]
pub enum RustType {
    Value,
    Primitive(String),
    Named(String),
    Struct(Vec<RustStructField>),
    Tuple(Vec<RustType>),
    Vec(Box<RustType>),
    Option(Box<RustType>),
    HashMap(Box<RustType>, Box<RustType>),
    Fn(Vec<RustType>, Box<RustType>),
    Generic(String, Vec<RustType>),
    StringLiteral(String),
    NumberLiteral(f64),
    BoolLiteral(bool),
    IntLiteral(i64),
}

/// A field in a Rust struct
#[derive(Debug, Clone)]
pub struct RustStructField {
    pub name: String,
    pub ty: RustType,
    pub optional: bool,
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
            String=>self.convert_primitive("String"),
            Number=>self.convert_primitive("f64"),
            Boolean=>self.convert_primitive("bool"),
            Void=>self.convert_primitive("()"),
            Never=>self.convert_primitive("!"),
            Unknown|Any|Null|Undefined=>RustType::Value,
            BigInt=>self.convert_primitive("i64"),
            Symbol=>self.convert_primitive("std::sync::Arc<std::fmt::Debug>"),
            This=>self.convert_primitive("Self"),
            Query{..}|Infer{..}=>RustType::Value,
            Array{elem}=>self.convert_array(elem),
            Ref{name,generics}=>self.convert_ref(name,generics),
            Object{members}=>self.convert_object(members),
            Union{types}|Intersection{types}=>self.convert_union(types),
            Function{params,ret}=>self.convert_function(params,ret),
            Literal{kind,value}=>self.convert_literal(kind,value),
            Template{..}=>RustType::Value,
            Index{obj,index}|Mapped{from:obj,to:index}=>self.convert_hashmap(obj,index),
            Tuple{elements}=>self.convert_tuple(elements),
            Partial{inner}|Required{inner}|Readonly{inner}=>self.convert(inner),
            Pick{inner,keys}|Omit{inner,keys}=>self.convert_pick_omit(inner,keys),
            Record{key,value}=>self.convert_hashmap(key,value),
            KeyOf{inner}=>self.convert_keyof(inner),
            ReturnType{inner}=>self.convert(inner),
            Parameters{..}|Conditional{..}=>RustType::Value,
        }
    }

    fn convert_primitive(&self, s: &str) -> RustType {
        RustType::Primitive(s.into())
    }

    fn convert_array(&self, elem: &Type) -> RustType {
        RustType::Vec(Box::new(self.convert(elem)))
    }

    fn convert_ref(&self, name: &str, generics: &[Type]) -> RustType {
        if generics.is_empty() {
            RustType::Named(name.to_string())
        } else {
            let gs: Vec<_> = generics.iter().map(|g| self.convert(g)).collect();
            RustType::Generic(name.to_string(), gs)
        }
    }

    fn convert_object(&self, members: &[TypeMember]) -> RustType {
        if members.is_empty() {
            RustType::Value
        } else {
            RustType::Struct(members.iter().map(|m| self.convert_member(m)).collect())
        }
    }

    fn convert_member(&self, m: &TypeMember) -> RustStructField {
        RustStructField {
            name: m.key.clone(),
            ty: self.convert(&m.type_),
            optional: m.optional,
        }
    }

    fn convert_function(&self, params: &[Type], ret: &Type) -> RustType {
        let ps: Vec<_> = params.iter().map(|p| self.convert(p)).collect();
        RustType::Fn(ps, Box::new(self.convert(ret)))
    }

    fn convert_union(&self, types: &[Type]) -> RustType {
        if types.is_empty() {
            RustType::Value
        } else if types.len() == 1 {
            self.convert(&types[0])
        } else {
            RustType::Value
        }
    }

    fn convert_literal(&self, kind: &LiteralKind, value: &str) -> RustType {
        match kind {
            LiteralKind::String=>RustType::StringLiteral(value.to_string()),
            LiteralKind::Number=>value.parse().map(RustType::NumberLiteral).unwrap_or(RustType::Value),
            LiteralKind::Boolean=>value.parse().map(RustType::BoolLiteral).unwrap_or(RustType::Value),
            LiteralKind::BigInt=>value.parse().map(RustType::IntLiteral).unwrap_or(RustType::Value),
        }
    }

    fn convert_tuple(&self, elements: &[crate::TupleElement]) -> RustType {
        let types: Vec<_> = elements.iter().map(|e| self.convert(&e.type_)).collect();
        RustType::Tuple(types)
    }

    fn convert_hashmap(&self, key: &Type, value: &Type) -> RustType {
        RustType::HashMap(Box::new(self.convert(key)), Box::new(self.convert(value)))
    }

    fn convert_keyof(&self, _inner: &Type) -> RustType {
        RustType::Value
    }

    fn convert_pick_omit(&self, inner: &Type, _keys: &[String]) -> RustType {
        self.convert(inner)
    }
}

impl RustType {
    pub fn type_name(&self) -> String {
        match self {
            RustType::Value=>"serde_json::Value".to_string(),
            RustType::Primitive(s)|RustType::Named(s)=>s.clone(),
            RustType::Struct(fields)=>{
                let fs:Vec<String>=fields.iter().map(|f|format!("{}: {}",f.name,f.ty.type_name())).collect();
                format!("{{ {} }}",fs.join(", "))
            }
            RustType::Tuple(types)=>{
                let inner:Vec<String>=types.iter().map(|t|t.type_name()).collect();
                format!("({})",inner.join(", "))
            }
            RustType::Vec(t)=>format!("Vec<{}>",t.type_name()),
            RustType::Option(t)=>format!("Option<{}>",t.type_name()),
            RustType::HashMap(k,v)=>format!("HashMap<{}, {}>",k.type_name(),v.type_name()),
            RustType::Fn(params,ret)=>{
                let ps:Vec<String>=params.iter().map(|p|p.type_name()).collect();
                format!("fn({}) -> {}",ps.join(", "),ret.type_name())
            }
            RustType::Generic(name,args)=>{
                let inner:Vec<String>=args.iter().map(|a|a.type_name()).collect();
                format!("{}<{}>",name,inner.join(", "))
            }
            RustType::StringLiteral(_)=>"String".to_string(),
            RustType::NumberLiteral(_)=>"f64".to_string(),
            RustType::BoolLiteral(_)=>"bool".to_string(),
            RustType::IntLiteral(_)=>"i64".to_string(),
        }
    }
}
