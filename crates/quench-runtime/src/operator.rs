//! The unary and binary operators an instruction's operator immediate names.
//!
//! The runtime owns these enums so no value, heap, native or image path names a parser type.
//! The discriminants are the parser's numbering, because the bytecode stores operators as the
//! numbers and every `binary_operator()` or `unary_operator()` field is compared with them.
//! `compile` pins each discriminant against the parser's enum at compile time, so a renumbered
//! parser fails the build rather than mis-decoding bytecode.

/// A binary operator, numbered as the bytecode numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum BinaryOperator {
    Equality = 0,
    Inequality = 1,
    StrictEquality = 2,
    StrictInequality = 3,
    LessThan = 4,
    LessEqualThan = 5,
    GreaterThan = 6,
    GreaterEqualThan = 7,
    Addition = 8,
    Subtraction = 9,
    Multiplication = 10,
    Division = 11,
    Remainder = 12,
    Exponential = 13,
    ShiftLeft = 14,
    ShiftRight = 15,
    ShiftRightZeroFill = 16,
    BitwiseOR = 17,
    BitwiseXOR = 18,
    BitwiseAnd = 19,
    In = 20,
    Instanceof = 21,
}

impl BinaryOperator {
    /// The operator as it appears in source code, the same text the parser's enum gives.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Equality => "==",
            Self::Inequality => "!=",
            Self::StrictEquality => "===",
            Self::StrictInequality => "!==",
            Self::LessThan => "<",
            Self::LessEqualThan => "<=",
            Self::GreaterThan => ">",
            Self::GreaterEqualThan => ">=",
            Self::Addition => "+",
            Self::Subtraction => "-",
            Self::Multiplication => "*",
            Self::Division => "/",
            Self::Remainder => "%",
            Self::Exponential => "**",
            Self::ShiftLeft => "<<",
            Self::ShiftRight => ">>",
            Self::ShiftRightZeroFill => ">>>",
            Self::BitwiseOR => "|",
            Self::BitwiseXOR => "^",
            Self::BitwiseAnd => "&",
            Self::In => "in",
            Self::Instanceof => "instanceof",
        }
    }
}

/// A unary operator, numbered as the bytecode numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum UnaryOperator {
    UnaryPlus = 0,
    UnaryNegation = 1,
    LogicalNot = 2,
    BitwiseNot = 3,
    Typeof = 4,
    Void = 5,
    Delete = 6,
}
