//! High-Level Intermediate Representation (HIR) — v2
//!
//! Sits between oxc AST and Rust codegen/dev interpreter.
//! Designed to be:
//!   - Serializable (JSON/bincode for incremental builds)
//!   - Interpretable (dev server evaluates HIR directly)
//!   - Code-generatable (build produces .rs from HIR)
//!
//! Covers the TS/TSX subset that runts compiles to native Rust.


// use std::collections::HashMap;

// ============================================================================
// Module
// ============================================================================

#[derive(Debug, Clone)]
pub struct Module {
    /// Source file path
    pub source: String,

    /// Top-level items
    pub items: Vec<ModuleItem>,

    /// Type declarations (interfaces, type aliases)
    pub types: Vec<TypeDecl>,

    /// Pre-computed semantic info
    pub semantic: SemanticInfo,
}

#[derive(Debug, Clone)]
pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

// ============================================================================
// Imports
// ============================================================================

#[derive(Debug, Clone)]
pub struct Import {
    pub source: String,
    pub specifiers: Vec<ImportSpec>,
    pub kind: ImportKind,
}

#[derive(Debug, Clone)]
pub enum ImportSpec {
    Named { name: String, alias: Option<String> },
    Default { name: String },
    Namespace { name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportKind {
    Value,
    Type,
    Namespace,
}

// ============================================================================
// Exports
// ============================================================================

#[derive(Debug, Clone)]
pub enum Export {
    Named { name: String },
    NamedValue { name: String, value: Expr },
    Default { expr: Expr },
    ReExport { source: String, names: Vec<String> },
    All { source: String },
}

// ============================================================================
// Declarations
// ============================================================================

#[derive(Debug, Clone)]
pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
    Class(ClassDecl),
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
    pub is_async: bool,
    pub is_generator: bool,
    pub decorators: Vec<Decorator>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pattern: Pat,
    pub type_: Option<Type>,
    pub default: Option<Expr>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct VariableDecl {
    pub kind: VarKind,
    pub pattern: Pat,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VarKind {
    Const,
    Let,
    Var,
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub type_: Type,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub extends: Option<Type>,
    pub implements: Vec<Type>,
    pub members: Vec<ClassMember>,
}

#[derive(Debug, Clone)]
pub struct ClassMember {
    pub name: String,
    pub type_: Option<Type>,
    pub optional: bool,
    pub readonly: bool,
    pub method: bool,
    pub getter: bool,
    pub setter: bool,
}

// ============================================================================
// Generic Parameters
// ============================================================================

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: String,
    pub bound: Option<Type>,
    pub default: Option<Type>,
}

// ============================================================================
// Decorators
// ============================================================================

#[derive(Debug, Clone)]
pub struct Decorator {
    pub expr: Expr,
    pub args: Vec<Expr>,
}

// ============================================================================
// Patterns (destructuring)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Pat {
    Ident { name: String },
    Array { elems: Vec<Option<Pat>>, rest: Option<Box<Pat>> },
    Object { props: Vec<ObjectPatProp>, rest: Option<Box<Pat>> },
    Rest { arg: Box<Pat> },
    Assign { left: Box<Pat>, right: Expr },
}

#[derive(Debug, Clone)]
pub enum ObjectPatProp {
    KeyValue { key: String, value: Pat },
    Shorthand { name: String },
    Rest { arg: Box<Pat> },
}

// ============================================================================
// Statements
// ============================================================================

#[derive(Debug, Clone)]
pub enum Stmt {
    Empty,
    Block(Vec<Stmt>),
    Expr { expr: Expr },
    Var { decl: VariableDecl },
    Return { arg: Option<Expr> },
    If { test: Expr, then: Box<Stmt>, else_: Option<Box<Stmt>> },
    While { test: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, test: Expr },
    For { init: Option<ForInit>, test: Option<Expr>, update: Option<Expr>, body: Box<Stmt> },
    ForIn { left: Pat, right: Expr, body: Box<Stmt> },
    ForOf { left: Pat, right: Expr, body: Box<Stmt>, is_await: bool },
    Switch { discriminant: Expr, cases: Vec<SwitchCase> },
    Try { block: Vec<Stmt>, catch: Option<CatchClause>, finally: Option<Vec<Stmt>> },
    Throw { arg: Expr },
    Break { label: Option<String> },
    Continue { label: Option<String> },
    Label { label: String, body: Box<Stmt> },
    Debugger,
    With { object: Expr, body: Box<Stmt> },
    Function { decl: FunctionDecl },
    Class { decl: ClassDecl },
}

#[derive(Debug, Clone)]
pub enum ForInit {
    Var(VariableDecl),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub test: Option<Expr>,
    pub consequent: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub param: Option<Pat>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Block(pub Vec<Stmt>);

impl Default for Block {
    fn default() -> Self {
        Self(Vec::new())
    }
}

// ============================================================================
// Expressions
// ============================================================================

#[derive(Debug, Clone)]
pub enum Expr {
    // Literals
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Undefined,
    BigInt(String),
    RegExp { pattern: String, flags: String },
    Template { parts: Vec<TemplatePart>, exprs: Vec<Expr> },

    // Identifiers
    Ident(String),
    This,
    Super,

    // Operations
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, arg: Box<Expr> },
    Update { op: UpdateOp, arg: Box<Expr>, prefix: bool },
    Logical { op: LogicalOp, left: Box<Expr>, right: Box<Expr> },
    Conditional { test: Box<Expr>, then: Box<Expr>, else_: Box<Expr> },

    // Access
    Member { object: Box<Expr>, property: Box<Expr>, computed: bool, optional: bool },

    // Calls
    Call { callee: Box<Expr>, args: Vec<Expr> },
    New { callee: Box<Expr>, args: Vec<Expr> },

    // Functions
    Arrow { params: Vec<Param>, body: ArrowBody, is_async: bool },
    Function { decl: FunctionDecl },

    // Objects / Arrays
    Object { props: Vec<Prop> },
    Array { elems: Vec<Option<Expr>> },

    // Assignment
    Assign { op: AssignOp, left: Box<Expr>, right: Box<Expr> },

    // JSX
    JSX(JSXExpr),

    // Async / Yield
    Await { arg: Box<Expr> },
    Yield { arg: Option<Box<Expr>>, delegate: bool },

    // Types
    TypeCast { expr: Box<Expr>, type_: Type },
    Typeof { arg: Box<Expr> },
    InstanceOf { left: Box<Expr>, right: Box<Expr> },
    In { left: Box<Expr>, right: Box<Expr> },

    // Meta
    MetaProp { kind: MetaPropKind },
    Spread { arg: Box<Expr> },
    Sequence { exprs: Vec<Expr> },

    // Class
    Class { decl: ClassDecl },
}

#[derive(Debug, Clone)]
pub enum ArrowBody {
    Expr(Box<Expr>),
    Block(Block),
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    String(String),
    Expr(Expr),
}

// ============================================================================
// Operators
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod, Exp,
    BitXor, BitAnd, BitOr,
    LeftShift, RightShift, UnsignedRightShift,
    Eq, Ne, EqStrict, NeStrict,
    Lt, Le, Gt, Ge,
    In, InstanceOf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Plus, Minus, Not, BitNot, TypeOf, Void, Delete,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateOp {
    Increment, Decrement,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalOp {
    And, Or, NullishCoalesce,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignOp {
    Assign,
    AddAssign, SubAssign, MulAssign, DivAssign, ModAssign,
    ExpAssign,
    BitXorAssign, BitAndAssign, BitOrAssign,
    LeftShiftAssign, RightShiftAssign, UnsignedRightShiftAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetaPropKind {
    NewTarget,
    ImportMeta,
}

// ============================================================================
// Object Properties
// ============================================================================

#[derive(Debug, Clone)]
pub enum Prop {
    Init { key: PropKey, value: Expr },
    Method { key: PropKey, value: FunctionDecl },
    Shorthand { name: String },
    Spread { value: Expr },
    Get { key: PropKey, value: FunctionDecl },
    Set { key: PropKey, value: FunctionDecl },
}

#[derive(Debug, Clone)]
pub enum PropKey {
    Ident(String),
    String(String),
    Number(f64),
    Computed(Box<Expr>),
}

// ============================================================================
// JSX
// ============================================================================

#[derive(Debug, Clone)]
pub struct JSXExpr {
    pub name: JSXName,
    pub attrs: Vec<JSXAttr>,
    pub children: Vec<JSXChild>,
    pub is_fragment: bool,
    pub is_self_closing: bool,
    pub key: Option<Box<Expr>>,
}

#[derive(Debug, Clone)]
pub enum JSXName {
    Ident(String),
    Member { object: String, property: String },
    Namespaced { ns: String, name: String },
    Dynamic(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum JSXAttr {
    Regular { name: String, value: Option<JSXAttrValue> },
    Spread { expr: Expr },
    Event { name: String, handler: Expr },
    Directive { name: String, value: Expr },
    Bool { name: String },
}

#[derive(Debug, Clone)]
pub enum JSXAttrValue {
    String(String),
    Expr(Expr),
    Boolean(bool),
}

#[derive(Debug, Clone)]
pub enum JSXChild {
    Text(String),
    Expr(Expr),
    JSX(JSXExpr),
    Fragment { children: Vec<JSXChild> },
    Spread { expr: Expr },
    Whitespace,
}

// ============================================================================
// Types (TypeScript type system)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Type {
    // Primitives
    String, Number, Boolean,
    Null, Undefined, Void,
    Any, Unknown, Never,
    BigInt, Symbol,

    // References
    Ref { name: String, generics: Vec<Type> },
    Qualified { left: Box<Type>, right: String },

    // Composites
    Array { elem: Box<Type> },
    Tuple { elems: Vec<Type> },
    Union { types: Vec<Type> },
    Intersection { types: Vec<Type> },
    Object { members: Vec<ObjectMember> },
    Function { params: Vec<ParamType>, ret: Box<Type>, generics: Vec<GenericParam> },

    // Advanced
    Index { object: Box<Type>, index: Box<Type> },
    Conditional { check: Box<Type>, extends: Box<Type>, true_type: Box<Type>, false_type: Box<Type> },
    Mapped { param: String, type_: Box<Type> },
    Paren { type_: Box<Type> },
    Template { parts: Vec<TypeTemplatePart> },
    Literal(LiteralType),
    Infer { param: String },
    Keyof(Box<Type>),
    Typeof(Box<Expr>),
    Readonly(Box<Type>),
    Partial(Box<Type>),
    ArrayReadonly(Box<Type>),
    Optional(Box<Type>),
}

#[derive(Debug, Clone)]
pub struct ObjectMember {
    pub key: String,
    pub type_: Type,
    pub optional: bool,
    pub readonly: bool,
    pub method: bool,
    pub getter: bool,
    pub setter: bool,
}

#[derive(Debug, Clone)]
pub struct ParamType {
    pub name: String,
    pub type_: Type,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone)]
pub enum LiteralType {
    String(String),
    Number(f64),
    Bool(bool),
    BigInt(String),
}

#[derive(Debug, Clone)]
pub enum TypeTemplatePart {
    String(String),
    Type(Type),
}

// ============================================================================
// Semantic Info (pre-computed during HIR building)
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct SemanticInfo {
    pub is_route: bool,
    pub is_island: bool,
    pub is_layout: bool,
    pub is_app: bool,
    pub is_middleware: bool,
    pub route_pattern: Option<String>,
    pub hooks: Vec<String>,
    pub signals: Vec<String>,
    pub components: Vec<String>,
    pub islands: Vec<String>,
    pub imports: Vec<ImportInfo>,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub source: String,
    pub names: Vec<String>,
    pub is_type_only: bool,
}

// ============================================================================
// Utility: display types as strings (for debugging)
// ============================================================================

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::String => write!(f, "string"),
            Type::Number => write!(f, "number"),
            Type::Boolean => write!(f, "boolean"),
            Type::Null => write!(f, "null"),
            Type::Undefined => write!(f, "undefined"),
            Type::Void => write!(f, "void"),
            Type::Any => write!(f, "any"),
            Type::Unknown => write!(f, "unknown"),
            Type::Never => write!(f, "never"),
            Type::BigInt => write!(f, "bigint"),
            Type::Symbol => write!(f, "symbol"),
            Type::Ref { name, generics } => {
                write!(f, "{}", name)?;
                if !generics.is_empty() {
                    let g: Vec<String> = generics.iter().map(|t| t.to_string()).collect();
                    write!(f, "<{}>", g.join(", "))?;
                }
                Ok(())
            }
            Type::Array { elem } => write!(f, "{}[]", elem),
            Type::Tuple { elems } => {
                let t: Vec<String> = elems.iter().map(|t| t.to_string()).collect();
                write!(f, "[{}]", t.join(", "))
            }
            Type::Union { types } => {
                let t: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", t.join(" | "))
            }
            Type::Intersection { types } => {
                let t: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", t.join(" & "))
            }
            Type::Object { members } => {
                write!(f, "{{ ")?;
                for m in members {
                    if m.optional {
                        write!(f, "{}?: {}; ", m.key, m.type_)?;
                    } else {
                        write!(f, "{}: {}; ", m.key, m.type_)?;
                    }
                }
                write!(f, "}}")
            }
            Type::Function { params, ret, .. } => {
                let p: Vec<String> = params.iter().map(|p| format!("{}: {}", p.name, p.type_)).collect();
                write!(f, "({}) => {}", p.join(", "), ret)
            }
            _ => write!(f, "<complex type>"),
        }
    }
}

impl std::fmt::Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.rest {
            write!(f, "...{}", self.name)?;
        } else {
            write!(f, "{}", self.name)?;
        }
        if self.optional {
            write!(f, "?")?;
        }
        write!(f, ": {}", self.type_)
    }
}
