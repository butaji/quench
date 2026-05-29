//! Base HIR types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub source: String,
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub source: String,
    pub specifiers: Vec<ImportSpecifier>,
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImportSpecifier {
    Named { name: String, alias: Option<String> },
    Default { name: String },
    Namespace { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ImportKind {
    Value,
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Export {
    Named { name: String },
    NamedWithValue { name: String, value: Expr },
    Default { expr: Expr },
    ReExport { source: String, names: Vec<String> },
    All { source: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
    Class(ClassDecl),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Option<Block>,
    pub is_async: bool,
    pub is_generator: bool,
    pub decorators: Vec<Decorator>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_: Option<Type>,
    pub default: Option<Expr>,
    pub optional: bool,
    pub pattern: Option<Pat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenericParam {
    pub name: String,
    pub constraint: Option<Type>,
    pub default: Option<Type>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDecl {
    pub name: String,
    pub kind: VariableKind,
    pub type_: Option<Type>,
    pub init: Option<Expr>,
    pub pattern: Option<Pat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VariableKind {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub type_: Type,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypeMember {
    pub key: String,
    pub type_: Type,
    pub optional: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatchClause {
    pub param: String,
    pub body: Box<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassDecl {
    pub name: String,
    pub extends: Option<Type>,
    pub members: Vec<ClassMember>,
    pub generics: Vec<GenericParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassMember {
    pub name: String,
    pub type_: Option<Type>,
    pub is_static: bool,
    pub is_async: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LiteralKind {
    String,
    Number,
    Boolean,
    BigInt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Type {
    String,
    Number,
    Boolean,
    Undefined,
    Null,
    Void,
    Never,
    Unknown,
    Any,
    BigInt,
    Symbol,
    Literal {
        kind: LiteralKind,
        value: String,
    },
    Ref {
        name: String,
        generics: Vec<Type>,
    },
    Union {
        types: Vec<Type>,
    },
    Intersection {
        types: Vec<Type>,
    },
    Array {
        elem: Box<Type>,
    },
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
    Object {
        members: Vec<TypeMember>,
    },
    Index {
        obj: Box<Type>,
        index: Box<Type>,
    },
    Query {
        expr: String,
    },
    Infer {
        name: String,
    },
    Mapped {
        from: Box<Type>,
        to: Box<Type>,
    },
    Conditional {
        check: Box<Type>,
        extends: Box<Type>,
        true_type: Box<Type>,
        false_type: Box<Type>,
    },
    This,
    Template {
        parts: Vec<TemplatePart>,
        values: Vec<Type>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum TemplatePart {
    String(String),
    Type(Type),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMember {
    pub key: String,
    pub type_: Type,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Block(pub Vec<Stmt>);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Decorator {
    pub expr: Expr,
}

pub use super::expr::*;
pub use super::pat::Pat;
