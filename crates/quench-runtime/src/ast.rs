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
    VarDeclaration {
        kind: VarKind,
        name: String,
        init: Option<Expression>,
    },
    /// Destructuring variable declaration (`let [a, b] = arr`) evaluated via iterator.
    PatternDeclaration {
        kind: VarKind,
        pattern: BindingElement,
        init: Option<Expression>,
    },
    /// Function declaration
    FunctionDeclaration {
        name: String,
        params: Vec<Param>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
    },
    /// Class declaration
    ClassDeclaration { name: String, class: Class },
    /// If statement
    If {
        condition: Box<Expression>,
        consequent: Box<Statement>,
        alternate: Option<Box<Statement>>,
    },
    /// While loop
    While {
        condition: Box<Expression>,
        body: Box<Statement>,
    },
    /// For loop
    For {
        init: Option<ForInit>,
        condition: Option<Box<Expression>>,
        update: Option<Box<Expression>>,
        body: Box<Statement>,
    },
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
    /// Labeled statement — `label: body`
    Labeled { label: String, body: Box<Statement> },
    /// Try statement with optional catch and finally
    Try {
        body: Box<Statement>,
        param: Option<String>,
        handler: Option<Box<Statement>>,
        finalizer: Option<Box<Statement>>,
    },
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
    ForIn {
        variable: Box<Expression>,
        object: Box<Expression>,
        body: Box<Statement>,
    },
    /// `with` statement — `with (obj) { body }`. Forbidden in strict mode
    /// (parser rejects), allowed in sloppy mode.
    With {
        object: Box<Expression>,
        body: Box<Statement>,
    },
    /// Do-while statement — `do { body } while (cond)`
    /// Kept as its own variant (not desugared) so the eval can capture the
    /// body completion value and return it when condition is false.
    /// `labels` stores any labels from the enclosing Statement::Labeled that
    /// should be visible to break/continue inside the body.
    DoWhile {
        body: Box<Statement>,
        condition: Box<Expression>,
        labels: Vec<String>,
    },
}

impl Statement {
    /// Returns true if this statement (or any statement reachable from it)
    /// contains an explicit `return`. Does NOT recurse into nested function
    /// declarations, because their returns belong to those functions.
    #[allow(clippy::complexity)]
    pub fn has_explicit_return(&self) -> bool {
        match self {
            Statement::Return(_) => true,
            Statement::Block(stmts) => stmts.iter().any(Statement::has_explicit_return),
            Statement::If {
                consequent,
                alternate,
                ..
            } => {
                consequent.has_explicit_return()
                    || alternate.as_ref().is_some_and(|a| a.has_explicit_return())
            }
            Statement::While { body, .. } => body.has_explicit_return(),
            Statement::For { body, .. } => body.has_explicit_return(),
            Statement::DoWhile { body, .. } => body.has_explicit_return(),
            Statement::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                body.has_explicit_return()
                    || handler.as_ref().is_some_and(|h| h.has_explicit_return())
                    || finalizer.as_ref().is_some_and(|f| f.has_explicit_return())
            }
            // Do not recurse into nested function declarations
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForInit {
    Expression(Box<Expression>),
    VarDeclaration {
        kind: VarKind,
        name: String,
        init: Option<Expression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Var,
    Let,
    Const,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// Regular value expression
    Value(Expression),
    /// Getter property: { get x() { return 42; } }
    Getter {
        params: Vec<String>,
        body: Vec<Statement>,
    },
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
    /// Array elision (hole) from `[ , ]` syntax; contributes to length but not to properties.
    Elision,
    /// Spread element: ...expr (used in array literals)
    Spread(Box<Expression>),
    FunctionExpression {
        name: Option<String>,
        params: Vec<Param>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
    },
    ArrowFunction {
        params: Vec<Param>,
        body: Box<ArrowBody>,
        is_async: bool,
        is_generator: bool,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Unary {
        op: UnaryOp,
        argument: Box<Expression>,
    },
    Assignment {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    CompoundAssignment {
        op: CompoundOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    /// Logical compound assignment with short-circuit evaluation (||=, &&=, ??=)
    LogicalCompoundAssignment {
        op: CompoundOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Call {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
    },
    Member {
        object: Box<Expression>,
        property: PropertyKey,
        computed: bool,
    },
    Conditional {
        condition: Box<Expression>,
        consequent: Box<Expression>,
        alternate: Box<Expression>,
    },
    Update {
        op: UpdateOp,
        argument: Box<Expression>,
        prefix: bool,
    },
    New {
        constructor: Box<Expression>,
        arguments: Vec<Expression>,
    },
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
    ForOf {
        variable: Box<Expression>,
        iterable: Box<Expression>,
        body: Box<Statement>,
    },
    /// For-in loop: for (x in object) { ... }
    ForIn {
        variable: Box<Expression>,
        object: Box<Expression>,
        body: Box<Statement>,
    },
    /// JSX element: <tag {...props}>{children}</tag>
    JsxElement {
        tag: JsxTagName,
        props: Vec<JsxProp>,
        children: Vec<JsxChild>,
    },
    /// JSX fragment: <>children</>
    JsxFragment {
        children: Vec<JsxChild>,
    },
    /// RegExp literal: /pattern/flags
    RegExp {
        pattern: String,
        flags: String,
    },
    /// BigInt literal: 123n, 0xFFn, etc.
    BigInt(String),
    /// Yield expression: yield expr (in generator functions)
    Yield(Option<Box<Expression>>),
    /// Yield* expression: yield* expr (delegate to another generator)
    YieldDelegate(Box<Expression>),
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
    Default(Box<BindingElement>, Box<Expression>),
    /// Array rest element: `...binding` inside `[a, ...rest]`.
    Rest(Box<BindingElement>),
    /// Full assignment target as an expression (e.g. `target()[key]` inside an
    /// object destructuring pattern). Evaluated at the spec-mandated point
    /// and PutValue'd with the destructured value.
    AssignmentTarget(Expression),
}

/// Function parameter - either a simple name, name with default, or rest parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<Box<Expression>>,
    /// Destructuring pattern for non-identifier parameters.
    pub pattern: Option<BindingElement>,
    /// Whether this is a rest parameter (...name)
    pub rest: bool,
}

impl Param {
    /// Create a simple parameter without default
    pub fn new(name: &str) -> Self {
        Param {
            name: name.to_string(),
            default: None,
            pattern: None,
            rest: false,
        }
    }

    /// Create a parameter with a default value
    pub fn with_default(name: &str, default: Expression) -> Self {
        Param {
            name: name.to_string(),
            default: Some(Box::new(default)),
            pattern: None,
            rest: false,
        }
    }

    /// Create a rest parameter (...name)
    pub fn rest(name: &str) -> Self {
        Param {
            name: name.to_string(),
            default: None,
            pattern: None,
            rest: true,
        }
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

/// Class member - method, getter, setter, static member, or field
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    /// Constructor
    Constructor {
        params: Vec<String>,
        body: Vec<Statement>,
    },
    /// Regular method (params include default values)
    Method {
        name: PropertyKey,
        params: Vec<Param>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
    },
    /// Getter
    Getter {
        name: PropertyKey,
        body: Vec<Statement>,
    },
    /// Setter
    Setter {
        name: PropertyKey,
        param: Param,
        body: Vec<Statement>,
    },
    /// Static method (params include default values)
    StaticMethod {
        name: PropertyKey,
        params: Vec<Param>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
    },
    /// Instance field: x = expr
    Field {
        name: PropertyKey,
        value: Expression,
    },
    /// Static field: static x = expr
    StaticField {
        name: PropertyKey,
        value: Expression,
    },
    /// Static getter
    StaticGetter {
        name: PropertyKey,
        body: Vec<Statement>,
    },
    /// Static setter
    StaticSetter {
        name: PropertyKey,
        param: Param,
        body: Vec<Statement>,
    },
    /// Static initialization block: static { ... }
    StaticBlock { body: Vec<Statement> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    Expression(Expression),
    Block(std::rc::Rc<Vec<Statement>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    And,
    Or,
    Eq,
    Neq,
    LooseEq,
    StrictEq,
    StrictNeq,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    /// Exponentiation: a ** b
    Pow,
    /// The `in` operator - checks if property exists in object
    In,
    /// The `instanceof` operator - checks if object is instance of constructor
    Instanceof,
    /// Nullish coalescing: a ?? b (returns b if a is null/undefined)
    NullishCoalescing,
}

impl BinaryOp {
    #[allow(clippy::complexity)]
    pub fn precedence(&self) -> u8 {
        match self {
            BinaryOp::Or | BinaryOp::NullishCoalescing => 1,
            BinaryOp::And => 2,
            BinaryOp::BitOr => 3,
            BinaryOp::BitXor => 4,
            BinaryOp::BitAnd => 5,
            BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::LooseEq
            | BinaryOp::StrictEq
            | BinaryOp::StrictNeq => 6,
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => 7,
            BinaryOp::In | BinaryOp::Instanceof => 7,
            BinaryOp::Shl | BinaryOp::Shr | BinaryOp::Ushr => 8,
            BinaryOp::Add | BinaryOp::Sub => 9,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => 10,
            BinaryOp::Pow => 11,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    Plus,
    BitNot,
    Typeof,
    Void,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// Exponentiation assignment: x **= y
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    /// Logical OR assignment: x ||= y
    LogicalOrAssign,
    /// Logical AND assignment: x &&= y
    LogicalAndAssign,
    /// Nullish coalescing assignment: x ??= y
    NullishCoalescingAssign,
}

impl CompoundOp {
    #[allow(clippy::complexity)]
    pub fn to_binary(&self) -> BinaryOp {
        match self {
            CompoundOp::Add => BinaryOp::Add,
            CompoundOp::Sub => BinaryOp::Sub,
            CompoundOp::Mul => BinaryOp::Mul,
            CompoundOp::Div => BinaryOp::Div,
            CompoundOp::Mod => BinaryOp::Mod,
            CompoundOp::Pow => BinaryOp::Pow,
            CompoundOp::BitAnd => BinaryOp::BitAnd,
            CompoundOp::BitOr => BinaryOp::BitOr,
            CompoundOp::BitXor => BinaryOp::BitXor,
            CompoundOp::Shl => BinaryOp::Shl,
            CompoundOp::Shr => BinaryOp::Shr,
            CompoundOp::Ushr => BinaryOp::Ushr,
            // Logical compound ops don't map to binary ops (they short-circuit)
            CompoundOp::LogicalOrAssign
            | CompoundOp::LogicalAndAssign
            | CompoundOp::NullishCoalescingAssign => {
                unreachable!("Logical compound ops don't use to_binary()")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    Increment,
    Decrement,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Param ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_param_new() {
        let p = Param::new("x");
        assert_eq!(p.name, "x");
        assert!(p.default.is_none());
        assert!(p.pattern.is_none());
        assert!(!p.rest);
    }

    #[test]
    fn test_param_with_default() {
        let p = Param::with_default("y", Expression::Number(42.0));
        assert_eq!(p.name, "y");
        assert!(p.default.is_some());
        assert!(!p.rest);
    }

    #[test]
    fn test_param_rest() {
        let p = Param::rest("args");
        assert_eq!(p.name, "args");
        assert!(p.default.is_none());
        assert!(p.rest);
    }

    // ── BinaryOp ────────────────────────────────────────────────────────────────

    #[test]
    fn test_binary_op_precedence_order() {
        assert!(BinaryOp::Pow.precedence() > BinaryOp::Mul.precedence());
        assert!(BinaryOp::Mul.precedence() > BinaryOp::Add.precedence());
        assert!(BinaryOp::Add.precedence() > BinaryOp::Shl.precedence());
        assert!(BinaryOp::Shl.precedence() > BinaryOp::Lt.precedence());
        assert!(BinaryOp::Lt.precedence() > BinaryOp::StrictEq.precedence());
        assert!(BinaryOp::StrictEq.precedence() > BinaryOp::BitAnd.precedence());
        assert!(BinaryOp::BitAnd.precedence() > BinaryOp::BitXor.precedence());
        assert!(BinaryOp::BitXor.precedence() > BinaryOp::BitOr.precedence());
        assert!(BinaryOp::BitOr.precedence() > BinaryOp::And.precedence());
        assert!(BinaryOp::And.precedence() > BinaryOp::Or.precedence());
    }

    #[test]
    fn test_binary_op_all_variants() {
        for op in [
            BinaryOp::And,
            BinaryOp::Or,
            BinaryOp::Eq,
            BinaryOp::Neq,
            BinaryOp::LooseEq,
            BinaryOp::StrictEq,
            BinaryOp::StrictNeq,
            BinaryOp::Lt,
            BinaryOp::Gt,
            BinaryOp::Le,
            BinaryOp::Ge,
            BinaryOp::Add,
            BinaryOp::Sub,
            BinaryOp::Mul,
            BinaryOp::Div,
            BinaryOp::Mod,
            BinaryOp::BitAnd,
            BinaryOp::BitOr,
            BinaryOp::BitXor,
            BinaryOp::Shl,
            BinaryOp::Shr,
            BinaryOp::Ushr,
            BinaryOp::Pow,
            BinaryOp::In,
            BinaryOp::Instanceof,
            BinaryOp::NullishCoalescing,
        ] {
            assert!(op.precedence() > 0);
        }
    }

    // ── CompoundOp ─────────────────────────────────────────────────────────────

    #[test]
    fn test_compound_op_to_binary() {
        assert_eq!(CompoundOp::Add.to_binary(), BinaryOp::Add);
        assert_eq!(CompoundOp::Sub.to_binary(), BinaryOp::Sub);
        assert_eq!(CompoundOp::Mul.to_binary(), BinaryOp::Mul);
        assert_eq!(CompoundOp::Div.to_binary(), BinaryOp::Div);
        assert_eq!(CompoundOp::Mod.to_binary(), BinaryOp::Mod);
        assert_eq!(CompoundOp::Pow.to_binary(), BinaryOp::Pow);
        assert_eq!(CompoundOp::BitAnd.to_binary(), BinaryOp::BitAnd);
        assert_eq!(CompoundOp::BitOr.to_binary(), BinaryOp::BitOr);
        assert_eq!(CompoundOp::BitXor.to_binary(), BinaryOp::BitXor);
        assert_eq!(CompoundOp::Shl.to_binary(), BinaryOp::Shl);
        assert_eq!(CompoundOp::Shr.to_binary(), BinaryOp::Shr);
        assert_eq!(CompoundOp::Ushr.to_binary(), BinaryOp::Ushr);
    }

    #[test]
    fn test_compound_op_logical_unreachable() {
        use std::panic;
        let r1 = panic::catch_unwind(|| CompoundOp::LogicalOrAssign.to_binary());
        let r2 = panic::catch_unwind(|| CompoundOp::LogicalAndAssign.to_binary());
        let r3 = panic::catch_unwind(|| CompoundOp::NullishCoalescingAssign.to_binary());
        assert!(r1.is_err());
        assert!(r2.is_err());
        assert!(r3.is_err());
    }

    // ── UnaryOp ────────────────────────────────────────────────────────────────

    #[test]
    fn test_unary_op_derives() {
        assert_eq!(UnaryOp::Not, UnaryOp::Not);
        assert_eq!(UnaryOp::Neg, UnaryOp::Neg);
        assert_eq!(UnaryOp::Plus, UnaryOp::Plus);
        assert_eq!(UnaryOp::BitNot, UnaryOp::BitNot);
        assert_eq!(UnaryOp::Typeof, UnaryOp::Typeof);
        assert_eq!(UnaryOp::Void, UnaryOp::Void);
        assert_eq!(UnaryOp::Delete, UnaryOp::Delete);
    }

    // ── UpdateOp ───────────────────────────────────────────────────────────────

    #[test]
    fn test_update_op_derives() {
        assert_eq!(UpdateOp::Increment, UpdateOp::Increment);
        assert_eq!(UpdateOp::Decrement, UpdateOp::Decrement);
    }

    // ── VarKind ────────────────────────────────────────────────────────────────

    #[test]
    fn test_var_kind_derives() {
        assert_eq!(VarKind::Var, VarKind::Var);
        assert_eq!(VarKind::Let, VarKind::Let);
        assert_eq!(VarKind::Const, VarKind::Const);
    }

    // ── PropertyKey ────────────────────────────────────────────────────────────

    #[test]
    fn test_property_key_derives() {
        let k1 = PropertyKey::Ident("a".to_string());
        let k2 = PropertyKey::Ident("a".to_string());
        let k3 = PropertyKey::Ident("b".to_string());
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
        assert_ne!(
            PropertyKey::Ident("x".to_string()),
            PropertyKey::String("x".to_string())
        );
    }

    // ── BindingElement ─────────────────────────────────────────────────────────

    #[test]
    fn test_binding_element_derives() {
        let b1 = BindingElement::Identifier("x".to_string());
        let b2 = BindingElement::Identifier("x".to_string());
        let b3 = BindingElement::Identifier("y".to_string());
        assert_eq!(b1, b2);
        assert_ne!(b1, b3);
    }

    // ── ArrowBody ─────────────────────────────────────────────────────────────

    #[test]
    fn test_arrow_body_derives() {
        let e = ArrowBody::Expression(Expression::Number(1.0));
        let b = ArrowBody::Block(std::rc::Rc::new(vec![Statement::Empty]));
        assert_eq!(e, ArrowBody::Expression(Expression::Number(1.0)));
        assert_ne!(e, b);
    }

    // ── Span ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_span_display() {
        let span = Span { start: 10, end: 20 };
        assert_eq!(format!("{}", span), "10..20");
    }

    #[test]
    fn test_span_default() {
        let span = Span::default();
        assert_eq!(span.start, 0);
        assert_eq!(span.end, 0);
    }

    // ── Program ────────────────────────────────────────────────────────────────

    #[test]
    fn test_program_derives() {
        let p1 = Program::Script(vec![Statement::Empty]);
        let p2 = Program::Script(vec![Statement::Empty]);
        assert_eq!(p1, p2);
    }
}
