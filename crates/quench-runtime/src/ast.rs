//! AST types for the JavaScript interpreter

use std::fmt;

/// Source position for error reporting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// JavaScript AST nodes
#[derive(Debug, Clone, PartialEq)]
pub enum Program {
    Script(Vec<Statement>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// Variable declaration
    VarDeclaration { kind: VarKind, name: String, init: Option<Expression> },
    /// Function declaration
    FunctionDeclaration { name: String, params: Vec<String>, body: Vec<Statement> },
    /// If statement
    If { condition: Box<Expression>, consequent: Box<Statement>, alternate: Option<Box<Statement>> },
    /// While loop
    While { condition: Box<Expression>, body: Box<Statement> },
    /// For loop
    For { init: Option<ForInit>, condition: Option<Box<Expression>>, update: Option<Box<Expression>>, body: Box<Statement> },
    /// Block statement
    Block(Vec<Statement>),
    /// Return statement
    Return(Option<Box<Expression>>),
    /// Expression statement
    Expression(Box<Expression>),
    /// Empty statement
    Empty,
    /// Break statement
    Break(Option<String>),
    /// Continue statement
    Continue(Option<String>),
    /// Try-catch statement
    TryCatch { body: Box<Statement>, param: Option<String>, handler: Box<Statement> },
    /// Throw statement
    Throw(Box<Expression>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForInit {
    Expression(Box<Expression>),
    VarDeclaration { kind: VarKind, name: String, init: Option<Expression> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind { Var, Let, Const }

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// Regular value expression
    Value(Expression),
    /// Getter property: { get x() { return 42; } }
    Getter { params: Vec<String>, body: Vec<Statement> },
    /// Setter property: { set x(v) { this._x = v; } }
    Setter { param: String, body: Vec<Statement> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
    Undefined,
    Identifier(String),
    /// Object with getter/setter support
    Object(Vec<(PropertyKey, PropertyValue)>),
    Array(Vec<Expression>),
    FunctionExpression { name: Option<String>, params: Vec<String>, body: Vec<Statement> },
    ArrowFunction { params: Vec<String>, body: Box<ArrowBody> },
    Binary { op: BinaryOp, left: Box<Expression>, right: Box<Expression> },
    Unary { op: UnaryOp, argument: Box<Expression> },
    Assignment { left: Box<Expression>, right: Box<Expression> },
    CompoundAssignment { op: CompoundOp, left: Box<Expression>, right: Box<Expression> },
    Call { callee: Box<Expression>, arguments: Vec<Expression> },
    Member { object: Box<Expression>, property: PropertyKey, computed: bool },
    Conditional { condition: Box<Expression>, consequent: Box<Expression>, alternate: Box<Expression> },
    Update { op: UpdateOp, argument: Box<Expression>, prefix: bool },
    New { constructor: Box<Expression>, arguments: Vec<Expression> },
    Sequence(Vec<Expression>),
    /// Block expression (for arrow functions with block bodies)
    BlockExpr(Vec<Statement>),
    /// Array destructuring pattern: [a, b] = expr
    ArrayPattern(Vec<BindingElement>),
    /// Object destructuring pattern: {a, b} = expr
    ObjectPattern(Vec<(PropertyKey, BindingElement)>),
    /// For-of loop: for (x of iterable) { ... }
    ForOf { variable: Box<Expression>, iterable: Box<Expression>, body: Box<Statement> },
    /// For-in loop: for (x in object) { ... }
    ForIn { variable: Box<Expression>, object: Box<Expression>, body: Box<Statement> },
    /// Optional chain member access: obj?.prop
    OptChain { object: Box<Expression>, property: PropertyKey, computed: bool },
    /// Optional chain call: obj?.method()
    OptChainCall { object: Box<Expression>, property: PropertyKey, computed: bool, arguments: Vec<Expression> },
}

/// Binding element - can be a simple identifier or nested pattern
#[derive(Debug, Clone, PartialEq)]
pub enum BindingElement {
    Identifier(String),
    ArrayPattern(Vec<BindingElement>),
    ObjectPattern(Vec<(PropertyKey, BindingElement)>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKey {
    Ident(String),
    String(String),
    Number(f64),
    Computed(Box<Expression>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    Expression(Expression),
    Block(Vec<Statement>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    And, Or,
    Eq, Neq, StrictEq, StrictNeq,
    Lt, Gt, Le, Ge,
    Add, Sub, Mul, Div, Mod,
    BitAnd, BitOr, BitXor, Shl, Shr, Ushr,
    /// The `in` operator - checks if property exists in object
    In,
    /// The `instanceof` operator - checks if object is instance of constructor
    Instanceof,
    /// Nullish coalescing: a ?? b (returns b if a is null/undefined)
    NullishCoalescing,
}

impl BinaryOp {
    pub fn precedence(&self) -> u8 {
        match self {
            BinaryOp::Or | BinaryOp::NullishCoalescing => 1,
            BinaryOp::And => 2,
            BinaryOp::BitOr => 3,
            BinaryOp::BitXor => 4,
            BinaryOp::BitAnd => 5,
            BinaryOp::Eq | BinaryOp::Neq | BinaryOp::StrictEq | BinaryOp::StrictNeq => 6,
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => 7,
            BinaryOp::In | BinaryOp::Instanceof => 7,
            BinaryOp::Shl | BinaryOp::Shr | BinaryOp::Ushr => 8,
            BinaryOp::Add | BinaryOp::Sub => 9,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp { Not, Neg, BitNot, Typeof, Void }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOp {
    Add, Sub, Mul, Div, Mod, BitAnd, BitOr, BitXor, Shl, Shr, Ushr,
}

impl CompoundOp {
    pub fn to_binary(&self) -> BinaryOp {
        match self {
            CompoundOp::Add => BinaryOp::Add,
            CompoundOp::Sub => BinaryOp::Sub,
            CompoundOp::Mul => BinaryOp::Mul,
            CompoundOp::Div => BinaryOp::Div,
            CompoundOp::Mod => BinaryOp::Mod,
            CompoundOp::BitAnd => BinaryOp::BitAnd,
            CompoundOp::BitOr => BinaryOp::BitOr,
            CompoundOp::BitXor => BinaryOp::BitXor,
            CompoundOp::Shl => BinaryOp::Shl,
            CompoundOp::Shr => BinaryOp::Shr,
            CompoundOp::Ushr => BinaryOp::Ushr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp { Increment, Decrement }
