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
    Stmt(Stmt),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub source: String,
    pub specifiers: Vec<ImportSpecifier>,
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// Ownership qualifier - mirrors Rust's borrow semantics
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Ownership {
    /// Owned value - takes ownership, moves on assignment
    Owned,
    /// Shared borrow - read-only access
    Borrow,
    /// Mutable borrow - read-write access
    Mut,
}

impl Default for Ownership {
    fn default() -> Self {
        Ownership::Owned
    }
}

impl Ownership {
    /// Returns true if this ownership represents a borrow (shared or mutable)
    pub fn is_borrow(&self) -> bool {
        matches!(self, Ownership::Borrow | Ownership::Mut)
    }
    
    /// Returns true if this ownership represents a mutable borrow
    pub fn is_mut(&self) -> bool {
        matches!(self, Ownership::Mut)
    }
    
    /// Get Rust lifetime annotation (empty for owned, ''' for borrow, ''' for mut)
    pub fn rust_lifetime(&self) -> &'static str {
        match self {
            Ownership::Owned => "",
            Ownership::Borrow => "&'",
            Ownership::Mut => "&'mut ",
        }
    }
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
    /// Whether this function can throw
    pub throws: bool,
    /// The error type if throws is true (None = JsValue)
    pub error_type: Option<Type>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_: Option<Type>,
    pub default: Option<Expr>,
    pub optional: bool,
    pub pattern: Option<Pat>,
    pub ownership: Ownership,
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
    pub methods: Vec<ClassMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Expr,
    pub kind: MethodKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MethodKind {
    Constructor,
    Method,
    Getter,
    Setter,
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
