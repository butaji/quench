//! High-level IR (Hir) for runts
//!
//! This module defines the intermediate representation that sits between
//! the swc AST and the generated Rust code.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A module (file) in the high-level IR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Source file path
    pub source: String,

    /// Top-level declarations
    pub items: Vec<ModuleItem>,

    /// Type declarations
    pub types: HashMap<String, TypeDef>,
}

/// Top-level module items
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

/// Import statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// Source module
    pub source: String,

    /// Import specifiers
    pub specifiers: Vec<ImportSpecifier>,

    /// Type-only import
    pub type_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImportSpecifier {
    Named { name: String, alias: Option<String> },
    Default { name: String },
    Namespace { name: String },
}

/// Import kind
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImportKind {
    Value,
    Type,
}

/// Export statement
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Export {
    Named { name: String },
    NamedWithValue { name: String, value: Expr },
    Default { expr: Expr },
    ReExport { source: String, names: Vec<String> },
    All { source: String },
}

/// Declarations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Decl {
    /// A function declaration
    Function(FunctionDecl),

    /// A variable declaration (const/let)
    Variable(VariableDecl),

    /// A type declaration (interface/type alias)
    Type(TypeDecl),

    /// A class declaration (not supported, for validation)
    Class(ClassDecl),
}

/// Function declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDecl {
    /// Function name
    pub name: String,

    /// Generic type parameters
    pub generics: Vec<GenericParam>,

    /// Parameters
    pub params: Vec<Param>,

    /// Return type
    pub return_type: Option<Type>,

    /// Function body
    pub body: Option<Block>,

    /// Is async
    pub is_async: bool,

    /// Is generator
    pub is_generator: bool,

    /// Decorators
    pub decorators: Vec<Decorator>,
}

/// Function parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub type_: Option<Type>,
    pub default: Option<Expr>,
    pub optional: bool,
    /// Pattern for destructuring
    pub pattern: Option<Pat>,
}

/// Generic type parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericParam {
    pub name: String,
    pub bound: Option<Type>,
    pub default: Option<Type>,
}

/// Variable declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDecl {
    pub name: String,
    pub kind: VariableKind,
    pub type_: Option<Type>,
    pub init: Option<Expr>,
    /// Destructuring pattern (for const [a, b] = ... or const { a, b } = ...)
    pub pattern: Option<Pat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VariableKind {
    Const,
    Let,
    Var,
}

/// Type declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub type_: Type,
}

/// Type member (for interface/object types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeMember {
    pub key: String,
    pub type_: Type,
    pub optional: bool,
    pub readonly: bool,
}

/// Catch clause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatchClause {
    pub param: String,
    pub body: Box<Block>,
}

/// Class declaration (for validation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDecl {
    pub name: String,
    pub extends: Option<Type>,
    pub implements: Vec<Type>,
    pub members: Vec<ClassMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassMember {
    pub name: String,
    pub type_: Option<Type>,
    pub optional: bool,
}

/// Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LiteralKind {
    String,
    Number,
    Boolean,
    BigInt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Type {
    /// Primitive types
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

    /// Literal type (string, number, boolean literals)
    Literal {
        kind: LiteralKind,
        value: String,
    },

    /// Reference to another type
    Ref {
        name: String,
        generics: Vec<Type>,
    },

    /// Union type
    Union {
        types: Vec<Type>,
    },

    /// Intersection type
    Intersection {
        types: Vec<Type>,
    },

    /// Array type
    Array {
        elem: Box<Type>,
    },

    /// Tuple type
    Tuple {
        types: Vec<Type>,
    },

    /// Object type (interface)
    Object {
        members: Vec<ObjectMember>,
    },

    /// Function type
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        generics: Vec<GenericParam>,
    },

    /// Parenthesized type
    Paren {
        type_: Box<Type>,
    },

    /// Index access type
    Index {
        obj: Box<Type>,
        index: Box<Type>,
    },

    /// Conditional type
    Conditional {
        check: Box<Type>,
        extends: Box<Type>,
        true_type: Box<Type>,
        false_type: Box<Type>,
    },

    /// Mapped type
    Mapped {
        param: String,
        type_: Box<Type>,
    },

    /// Template literal type
    Template {
        parts: Vec<TemplatePart>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub readonly: bool,
    // TODO: Methods, call signatures, construct signatures
}

/// Patterns (destructuring)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Pat {
    Ident {
        name: String,
        type_: Option<Type>,
    },
    Array {
        elems: Vec<Option<Pat>>,
        rest: Option<Box<Pat>>,
    },
    Object {
        props: Vec<ObjectPatProp>,
        rest: Option<Box<Pat>>,
    },
    Assign {
        left: Box<Pat>,
        right: Box<Expr>,
    },
    Rest {
        arg: Box<Pat>,
    },
    /// Default value pattern
    Default {
        arg: Box<Pat>,
        default: Box<Expr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectPatProp {
    Init { key: String, value: Pat },
    Rest { arg: Box<Pat> },
}

/// Statements
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Stmt {
    /// Empty statement
    Empty,

    /// Block statement
    Block(Vec<Stmt>),

    /// Expression statement
    Expr { expr: Expr },

    /// If statement
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },

    /// While statement
    While { test: Expr, body: Box<Stmt> },

    /// Do-while statement
    DoWhile { body: Box<Stmt>, test: Expr },

    /// For statement
    For {
        init: Option<ForInit>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },

    /// For-in statement
    ForIn {
        left: ForInit,
        right: Expr,
        body: Box<Stmt>,
    },

    /// For-of statement
    ForOf {
        left: ForInit,
        right: Expr,
        body: Box<Stmt>,
        is_await: bool,
    },

    /// Switch statement
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },

    /// Return statement
    Return { arg: Option<Expr> },

    /// Throw statement
    Throw { arg: Expr },

    /// Break statement
    Break { label: Option<String> },

    /// Continue statement
    Continue { label: Option<String> },

    /// Labeled statement
    Label { label: String, body: Box<Stmt> },

    /// Try statement
    Try {
        block: Box<Stmt>,
        handler: Option<Box<Stmt>>,
        finalizer: Option<Box<Stmt>>,
    },

    /// Debugger statement
    Debugger,

    /// With statement (not supported)
    With { object: Expr, body: Box<Stmt> },

    /// Variable declaration statement
    Variable { decl: VariableDecl },

    /// Function declaration
    Function { decl: FunctionDecl },

    /// Class declaration
    Class { decl: ClassDecl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ForInit {
    Variable(VariableDecl),
    Expr(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchCase {
    pub test: Option<Expr>,
    pub consequent: Vec<Stmt>,
}

/// Block (function/arrow body)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block(pub Vec<Stmt>);

impl Block {
    #[allow(dead_code)]
    pub fn new(stmts: Vec<Stmt>) -> Self {
        Self(stmts)
    }
}

impl Default for Block {
    fn default() -> Self {
        Self(Vec::new())
    }
}

/// Expressions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Expr {
    // Literals
    String(String),
    Number(f64),
    BigInt(u64),
    Boolean(bool),
    Null,
    Undefined,
    RegExp {
        pattern: String,
        flags: String,
    },
    Template {
        parts: Vec<TemplatePart>,
        exprs: Vec<Expr>,
    },

    // Identifiers
    Ident {
        name: String,
    },

    // JSX
    JSX(JSXExpr),

    // Operators
    Bin {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        arg: Box<Expr>,
        prefix: bool,
    },
    Update {
        op: UpdateOp,
        arg: Box<Expr>,
        prefix: bool,
    },
    Logical {
        op: LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cond {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },

    // Function calls
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        type_args: Vec<Type>,
    },
    New {
        callee: Box<Expr>,
        args: Vec<Expr>,
        type_args: Vec<Type>,
    },
    TaggedTemplate {
        tag: Box<Expr>,
        template: Box<Expr>,
    },

    // Member access
    Member {
        object: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
        optional: bool,
    },

    // Object/Array
    Object {
        props: Vec<ObjectProp>,
    },
    Array {
        elems: Vec<Option<Expr>>,
    },

    // Function
    Arrow {
        params: Vec<Param>,
        body: Box<Stmt>,
        is_async: bool,
    },
    Function {
        decl: FunctionDecl,
    },

    // Await/Yield
    Await {
        arg: Box<Expr>,
    },
    Yield {
        arg: Option<Box<Expr>>,
        delegate: bool,
    },

    // Class expression
    Class {
        decl: ClassDecl,
    },

    // Type assertions (limited)
    TSAs {
        expr: Box<Expr>,
        type_: Type,
    },

    // Meta properties
    MetaProp {
        kind: MetaPropKind,
    },

    // Sequence expression
    Seq {
        exprs: Vec<Expr>,
    },

    // Spread
    Spread {
        arg: Box<Expr>,
    },

    // Assignment
    Assign {
        op: AssignOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    DivStrict,
    // Bitwise
    BitXor,
    BitAnd,
    BitOr,
    LeftShift,
    RightShift,
    RightShiftAll,
    // Comparison
    Eq,
    Ne,
    EqStrict,
    NeStrict,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    InstanceOf,
    // Logical (but handled as LogicalOp)
    LogicalAnd,
    LogicalOr,
    // Nullish
    NullishCoalesce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnaryOp {
    Plus,
    Minus,
    Not,
    BitNot,
    TypeOf,
    Void,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateOp {
    Increment,
    Decrement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogicalOp {
    And,
    Or,
    NullishCoalesce,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    ExpAssign,
    BitXorAssign,
    BitAndAssign,
    BitOrAssign,
    LeftShiftAssign,
    RightShiftAssign,
    RightShiftAllAssign,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetaPropKind {
    NewTarget,
    ImportMeta,
}

/// Object property
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ObjectProp {
    Init { key: PropKey, value: Expr },
    Method { key: PropKey, value: FunctionDecl },
    Shorthand { name: String },
    Spread { value: Expr },
    Get { key: PropKey, value: FunctionDecl },
    Set { key: PropKey, value: FunctionDecl },
}

/// Property key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropKey {
    Ident(String),
    String(String),
    Number(f64),
    Computed(Expr),
}

impl PartialEq for PropKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PropKey::Ident(a), PropKey::Ident(b)) => a == b,
            (PropKey::String(a), PropKey::String(b)) => a == b,
            (PropKey::Number(a), PropKey::Number(b)) => a == b,
            (PropKey::Computed(_), PropKey::Computed(_)) => false, // Conservative: computed keys not equal
            _ => false,
        }
    }
}

impl Eq for PropKey {}

impl std::hash::Hash for PropKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            PropKey::Ident(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            PropKey::String(s) => {
                1u8.hash(state);
                s.hash(state);
            }
            PropKey::Number(n) => {
                2u8.hash(state);
                n.to_bits().hash(state);
            }
            PropKey::Computed(_) => {
                3u8.hash(state);
            } // Conservative
        }
    }
}

/// JSX expressions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSXExpr {
    /// Opening element
    pub opening: JSXOpening,

    /// Children (if fragment or has children)
    pub children: Vec<JSXChild>,

    /// Closing element (if not self-closing)
    pub closing: Option<JSXClosing>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSXOpening {
    /// Element name (for elements) or components
    pub name: JSXName,

    /// Attributes
    pub attrs: Vec<JSXAttr>,

    /// Is self-closing
    pub self_closing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JSXName {
    /// HTML element name (lowercase)
    Ident(String),
    /// Member expression (Component.Nested)
    Member { object: String, property: String },
    /// Namespaced (not common in Fresh)
    Namespaced { ns: String, name: String },
    /// Dynamic component
    Dynamic(Box<Expr>),
    /// Fragment (<>...</>)
    Fragment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JSXAttr {
    /// Regular attribute
    Attr {
        name: String,
        value: Option<JSXAttrValue>,
    },
    /// Spread attributes
    Spread { expr: Expr },
    /// Event handler
    Event { name: String, handler: Expr },
    /// Boolean attribute (true if present)
    Bool { name: String },
    /// JSX expression container
    Expr { name: Option<String>, expr: Expr },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JSXAttrValue {
    String(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JSXChild {
    /// Text content
    Text(String),
    /// JSX expression
    Expr(Expr),
    /// Nested JSX
    JSX(JSXExpr),
    /// Fragment
    Fragment { children: Vec<JSXChild> },
    /// Spread children
    Spread { expr: Expr },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSXClosing {
    pub name: JSXName,
}

/// Decorator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decorator {
    pub expr: Expr,
    pub args: Vec<Expr>,
}

/// Type definitions (top-level interfaces/types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDef {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub type_: Type,
}

// ============================================================================
// Semantic Analysis Types
// ============================================================================

/// Semantic information for a module
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticInfo {
    /// Is this an island file?
    pub is_island: bool,

    /// Is this a route file?
    pub is_route: bool,

    /// Route pattern (if route file)
    pub route_pattern: Option<String>,

    /// Is this an app wrapper?
    pub is_app: bool,

    /// Is this a layout?
    pub is_layout: bool,

    /// Is this middleware?
    pub is_middleware: bool,

    /// Imported hooks
    pub hooks: Vec<String>,

    /// Components defined
    pub components: Vec<String>,

    /// Functions defined
    pub functions: Vec<String>,
}
