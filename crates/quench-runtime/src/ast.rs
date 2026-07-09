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
    FunctionDeclaration { name: String, params: Vec<Param>, body: Vec<Statement> },
    /// Class declaration
    ClassDeclaration { name: String, class: Class },
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
    /// Sequence of var declarations (avoids block scope for var hoisting)
    SequenceDecls(Vec<Statement>),
    /// Export statement (ES module export)
    Export(Box<Statement>),
    /// Import statement (ES module import)
    /// `import x from 'mod'` -> Import { default: Some("x"), named: [], namespace: None, source: "mod" }
    /// `import { a, b } from 'mod'` -> Import { default: None, named: [("a","a"),("b","b")], namespace: None, source: "mod" }
    /// `import * as ns from 'mod'` -> Import { default: None, named: [], namespace: Some("ns"), source: "mod" }
    Import {
        default: Option<String>,
        named: Vec<(String, String)>, // (local_name, exported_name)
        namespace: Option<String>,
        source: String,
    },
    /// For-in loop (ES6)
    /// for (x in object) { body }
    ForIn { variable: Box<Expression>, object: Box<Expression>, body: Box<Statement> },
}

impl Statement {
    /// Returns true if this statement (or any statement reachable from it)
    /// contains an explicit `return`. Does NOT recurse into nested function
    /// declarations, because their returns belong to those functions.
    pub fn has_explicit_return(&self) -> bool {
        match self {
            Statement::Return(_) => true,
            Statement::Block(stmts) => stmts.iter().any(Statement::has_explicit_return),
            Statement::If { consequent, alternate, .. } => {
                consequent.has_explicit_return()
                    || alternate.as_ref().is_some_and(|a| a.has_explicit_return())
            }
            Statement::While { body, .. } => body.has_explicit_return(),
            Statement::For { body, .. } => body.has_explicit_return(),
            Statement::TryCatch { body, handler, .. } => {
                body.has_explicit_return() || handler.has_explicit_return()
            }
            // Do not recurse into nested function declarations
            _ => false,
        }
    }
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
    /// Spread element: ...expr (used in array literals)
    Spread(Box<Expression>),
    FunctionExpression { name: Option<String>, params: Vec<Param>, body: Vec<Statement> },
    ArrowFunction { params: Vec<Param>, body: Box<ArrowBody> },
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
    /// Class expression
    Class(Class),
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
    /// JSX element: <tag {...props}>{children}</tag>
    JsxElement { tag: JsxTagName, props: Vec<JsxProp>, children: Vec<JsxChild> },
    /// JSX fragment: <>children</>
    JsxFragment { children: Vec<JsxChild> },
}

/// JSX tag name (element name or component reference)
#[derive(Debug, Clone, PartialEq)]
pub enum JsxTagName {
    /// Regular HTML tag name: div, span, input
    Ident(String),
    /// Member expression: Foo.Bar
    Member { object: String, property: String },
    /// Namespaced name: ns:Element
    Namespaced { namespace: String, name: String },
}

/// JSX property (attribute or spread)
#[derive(Debug, Clone, PartialEq)]
pub enum JsxProp {
    /// Regular attribute: className="foo"
    Attr { name: String, value: JsxAttrValue },
    /// Spread attribute: {...props}
    Spread(Expression),
}

/// JSX attribute value
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttrValue {
    /// String literal: className="foo"
    String(String),
    /// Expression: value={count}
    Expression(Expression),
}

/// JSX child element
#[derive(Debug, Clone, PartialEq)]
pub enum JsxChild {
    /// Text content
    Text(String),
    /// Expression: {count}
    Expression(Expression),
    /// Spread: {...children}
    Spread(Expression),
    /// Nested JSX element
    Element(Box<Expression>),
}

/// Binding element - can be a simple identifier or nested pattern
#[derive(Debug, Clone, PartialEq)]
pub enum BindingElement {
    Identifier(String),
    ArrayPattern(Vec<BindingElement>),
    ObjectPattern(Vec<(PropertyKey, BindingElement)>),
}

/// Function parameter - either a simple name or a name with a default value
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<Box<Expression>>,
}

impl Param {
    /// Create a simple parameter without default
    pub fn new(name: &str) -> Self {
        Param { name: name.to_string(), default: None }
    }

    /// Create a parameter with a default value
    pub fn with_default(name: &str, default: Expression) -> Self {
        Param { name: name.to_string(), default: Some(Box::new(default)) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKey {
    Ident(String),
    String(String),
    Number(f64),
    Computed(Box<Expression>),
}

/// Class definition - used for both class declarations and expressions
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    /// Optional class name (for named class expressions)
    pub name: Option<String>,
    /// Superclass expression (None for no extends)
    pub super_class: Option<Box<Expression>>,
    /// Class body members
    pub body: Vec<ClassMember>,
}

/// Class member - method, getter, setter, or static member
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    /// Constructor
    Constructor { params: Vec<String>, body: Vec<Statement> },
    /// Regular method
    Method { name: PropertyKey, params: Vec<String>, body: Vec<Statement> },
    /// Getter
    Getter { name: PropertyKey, body: Vec<Statement> },
    /// Setter
    Setter { name: PropertyKey, param: String, body: Vec<Statement> },
    /// Static method
    StaticMethod { name: PropertyKey, params: Vec<String>, body: Vec<Statement> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    Expression(Expression),
    Block(std::rc::Rc<Vec<Statement>>),
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
pub enum UnaryOp { Not, Neg, Plus, BitNot, Typeof, Void, Delete }

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
