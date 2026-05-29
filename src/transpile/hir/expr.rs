//! Expression types

use super::{ClassMember, FunctionDecl, Param, TemplatePart, Type};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Expr {
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
    Ident {
        name: String,
    },
    JSX(JSXExpr),
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
    Assign {
        op: AssignOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Array {
        elems: Vec<Option<Expr>>,
    },
    Object {
        members: Vec<ObjectMemberExpr>,
    },
    Function(FunctionDecl),
    ArrowFunction {
        params: Vec<Param>,
        body: Box<Expr>,
        is_async: bool,
    },
    Await {
        arg: Box<Expr>,
    },
    Yield {
        arg: Option<Box<Expr>>,
        delegate: bool,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
    },
    New {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
    },
    Member {
        obj: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
    },
    Super,
    This,
    StaticMember {
        obj: Box<Expr>,
        property: String,
    },
    PrivateMember {
        obj: Box<Expr>,
        property: String,
    },
    MetaProperty {
        kind: MetaPropKind,
    },
    TaggedTemplate {
        tag: Box<Expr>,
        template: Box<Expr>,
    },
    Seq {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Spread {
        arg: Box<Expr>,
    },
    Class {
        id: Option<String>,
        super_class: Option<Box<Expr>>,
        members: Vec<ClassMember>,
    },
    TypeAnnot {
        type_: Box<Type>,
    },
    ArrowWithType {
        params: Vec<Param>,
        body: Box<Stmt>,
        return_type: Option<Type>,
        is_async: bool,
    },
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    DivStrict,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    UShr,
    Eq,
    StrictEq,
    Neq,
    StrictNeq,
    Lt,
    Lte,
    Gt,
    Gte,
    Instanceof,
    In,
    LogicalAnd,
    LogicalOr,
    NullishCoalescing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UnaryOp {
    Plus,
    Minus,
    Not,
    BitNot,
    Typeof,
    Void,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UpdateOp {
    PlusPlus,
    MinusMinus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogicalOp {
    And,
    Or,
    NullishCoalescing,
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
    ShlAssign,
    ShrAssign,
    UShrAssign,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MetaPropKind {
    NewTarget,
    ImportMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObjectProp {
    Init {
        key: PropKey,
        value: Expr,
        computed: bool,
    },
    Get {
        key: PropKey,
        value: Expr,
        computed: bool,
    },
    Set {
        key: PropKey,
        value: Expr,
        computed: bool,
    },
    Method {
        key: PropKey,
        value: Expr,
        computed: bool,
    },
    Spread {
        arg: Expr,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PropKey {
    Str(String),
    Num(f64),
    Computed { expr: Expr },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectMemberExpr {
    pub prop: ObjectProp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JSXExpr {
    pub opening: JSXOpening,
    pub closing: Option<JSXClosing>,
    pub children: Vec<JSXChild>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JSXOpening {
    pub name: JSXName,
    pub attrs: Vec<JSXAttr>,
    pub self_closing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JSXName {
    Ident(String),
    Member { object: String, property: String },
    Namespaced { ns: String, name: String },
    Dynamic(Box<Expr>),
    Fragment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JSXAttr {
    Attr {
        name: String,
        value: Option<JSXAttrValue>,
    },
    Spread {
        expr: Expr,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JSXAttrValue {
    String(String),
    Expr(Expr),
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JSXChild {
    Text(String),
    Expr(Expr),
    JSX(JSXExpr),
    Fragment { children: Vec<JSXChild> },
    Spread { expr: Expr },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JSXClosing {
    pub name: JSXName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDef {
    pub name: String,
    pub type_: Type,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticInfo {
    pub scope_id: usize,
    pub type_id: Option<usize>,
}

pub use super::stmt::Stmt;
