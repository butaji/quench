//! Base HIR types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Route information for plugin code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    /// URL path pattern (e.g., "/", "/blog", "/blog/[slug]")
    pub path: String,
    /// HTTP methods supported
    pub methods: Vec<String>,
    /// Relative file path from project root
    pub file_path: String,
}

impl RouteInfo {
    pub fn new(path: &str, file_path: &str) -> Self {
        Self {
            path: path.to_string(),
            methods: Vec::new(),
            file_path: file_path.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Module {
    pub source: String,
    pub source_path: Option<String>,
    pub route_info: Option<RouteInfo>,
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}

/// Top-level items in a module: imports, exports, declarations, and
/// statements. Default external tagging produces unambiguous JSON like
/// `{"Decl": {"Function": ...}}` or `{"Stmt": {"Return": ...}}`.
///
/// Note: this used to be `#[serde(tag = "kind")]`, but the newtype
/// variant in an internally-tagged enum is broken — the inner type's
/// own `kind` tag (e.g. `FunctionDecl.kind = "Function"`,
/// `Stmt::Return.kind = "Return"`) collides with the outer tag, so
/// the outer variant name was being silently dropped. The current
/// shape (default external tagging) serializes every layer
/// explicitly and round-trips correctly.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[allow(dead_code)]
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
    NamedRenamed { local: String, exported: String },
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

#[allow(dead_code)]
impl Ownership {
    /// Returns true if this ownership represents a borrow (shared or mutable)
    pub fn is_borrow(&self) -> bool {
        matches!(self, Ownership::Borrow | Ownership::Mut)
    }

    /// Returns true if this ownership represents a mutable borrow
    pub fn is_mut(&self) -> bool {
        matches!(self, Ownership::Mut)
    }

    /// Get Rust lifetime annotation (empty for owned, '&' for borrow, '&mut ' for mut)
    /// Uses elided lifetimes for simplicity - caller should use explicit lifetimes where needed
    pub fn rust_lifetime(&self) -> &'static str {
        match self {
            Ownership::Owned => "",
            Ownership::Borrow => "&",
            Ownership::Mut => "&mut ",
        }
    }

    /// Get Rust lifetime annotation with a specific lifetime name
    pub fn rust_lifetime_named(&self, lifetime: &str) -> String {
        match self {
            Ownership::Owned => String::new(),
            Ownership::Borrow => format!("&{}", lifetime),
            Ownership::Mut => format!("&{} ", lifetime),
        }
    }
}

/// `Decl` is serialized with default external tagging for the same
/// reason as `ModuleItem` above: the newtype variant in an
/// internally-tagged enum collides with the inner type's own `kind`
/// tag (e.g. `VariableDecl.kind`, `FunctionDecl`'s sibling tags).
/// Externally tagged form is `{"Function": {...}}` or
/// `{"Variable": {...}}` — unambiguous and round-trippable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
    Class(ClassDecl),
    Enum(EnumDecl),
}

/// Enum declaration - TypeScript enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub members: Vec<EnumMember>,
    pub is_const: bool,
}

/// Enum member - a single enum value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnumMember {
    pub key: String,
    pub value: Option<EnumValue>,
}

/// Enum member value - numeric or string literal
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EnumValue {
    Number(f64),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VariableDecl {
    pub name: String,
    pub kind: VariableKind,
    pub type_: Option<Type>,
    pub init: Option<Expr>,
    pub pattern: Option<Pat>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum VariableKind {
    #[default]
    Var,
    Let,
    Const,
    /// ES2024 / TS 5.2 explicit resource management
    Using,
    /// ES2024 / TS 5.2 async explicit resource management
    AwaitUsing,
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
    pub decorators: Vec<Decorator>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Expr,
    pub kind: MethodKind,
    pub decorators: Vec<Decorator>,
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
    pub is_private: bool,
    pub decorators: Vec<Decorator>,
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
    /// Partial<T> - makes all fields optional
    Partial {
        inner: Box<Type>,
    },
    /// Required<T> - removes optional marker
    Required {
        inner: Box<Type>,
    },
    /// Pick<T, K> - extracts specified keys
    Pick {
        inner: Box<Type>,
        keys: Vec<String>,
    },
    /// Omit<T, K> - removes specified keys
    Omit {
        inner: Box<Type>,
        keys: Vec<String>,
    },
    /// Record<K, V> - creates object type with keys K and values V
    Record {
        key: Box<Type>,
        value: Box<Type>,
    },
    /// keyof T - creates union of field names
    KeyOf {
        inner: Box<Type>,
    },
    /// ReturnType<T> - extracts return type of function
    ReturnType {
        inner: Box<Type>,
    },
    /// Parameters<T> - extracts parameter types as tuple
    Parameters {
        inner: Box<Type>,
    },
    /// Readonly<T> - makes all fields readonly
    Readonly {
        inner: Box<Type>,
    },
    /// Tuple type with optional named elements
    /// e.g., [x: number, y: number]
    Tuple {
        elements: Vec<TupleElement>,
    },
}

/// A single element in a tuple type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TupleElement {
    /// Optional name of the element (e.g., `x` in `[x: number]`)
    pub name: Option<String>,
    /// The type of this element
    pub type_: Type,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TemplatePart {
    /// A literal string segment of a template literal (e.g. the "Hello " in
    /// `\`Hello ${name}!\``). Uses a struct-variant form rather than a newtype
    /// tuple so that serde's internally-tagged enum representation works
    /// (newtype variants like `String(String)` are not supported in stable
    /// Rust's `#[serde(tag = "...")]` mode — they raise "cannot serialize
    /// tagged newtype variant TemplatePart::String containing a string").
    String { value: String },
    /// A Type-carrying template segment (rare in practice; used for embedded
    /// type assertions inside template literals).
    Type { value: Type },
}

#[allow(dead_code)]
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
