use super::{Expression, Statement};

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
