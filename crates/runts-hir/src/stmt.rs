//! Statement types

use super::{
    Block, CatchClause, ClassDecl, EnumDecl, Export, Expr, FunctionDecl, ImportSpecifier,
    VariableDecl,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum Stmt {
    Empty,
    /// A block of statements. Uses a struct-variant form rather than
    /// a newtype tuple so that serde's internally-tagged enum
    /// representation works (newtype variants like
    /// `Block(Vec<Stmt>)` are not supported in stable Rust's
    /// `#[serde(tag = "...")]` mode — they produce the error
    /// "cannot serialize tagged newtype variant ... containing a
    /// sequence" on every round-trip through JSON).
    Block { stmts: Vec<Stmt> },
    Expr {
        expr: Expr,
    },
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    While {
        test: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        test: Expr,
    },
    For {
        init: Option<ForInit>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn {
        left: ForInit,
        right: Expr,
        body: Box<Stmt>,
    },
    ForOf {
        left: ForInit,
        right: Expr,
        body: Box<Stmt>,
        is_await: bool,
    },
    Continue {
        label: Option<String>,
    },
    Break {
        label: Option<String>,
    },
    Return {
        arg: Option<Expr>,
    },
    With {
        obj: Expr,
        body: Box<Stmt>,
    },
    Labeled {
        label: String,
        body: Box<Stmt>,
    },
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    Throw {
        arg: Expr,
    },
    Try {
        block: Block,
        handler: Option<CatchClause>,
        finalizer: Option<Block>,
    },
    FunctionDecl(FunctionDecl),
    Class(ClassDecl),
    Variable(VariableDecl),
    Enum(EnumDecl),
    ExportNamed {
        specifiers: Vec<Export>,
    },
    ExportDefault {
        expr: Expr,
    },
    ImportNamed {
        source: String,
        specifiers: Vec<ImportSpecifier>,
    },
    ImportDefault {
        source: String,
        local: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForInit {
    Variable(super::VariableKind, Vec<(String, Option<Expr>)>),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwitchCase {
    pub test: Option<Expr>,
    pub consequent: Vec<Stmt>,
}
