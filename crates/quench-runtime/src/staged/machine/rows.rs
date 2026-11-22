//! The op rows of the machine primitives: the one enum of each concept. A row names an
//! operation by its machine meaning, not a language's; rules that have a language meaning (a
//! Wasm operator, a JavaScript operation) map to a row once, in the backend that executes it.

use crate::staged::wasm::Width;
use crate::staged::wasm_float::Float;

/// The width and kind of a machine value or memory cell: an integer or float word, a narrow
/// integer cell, or a 128-bit vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane { I8, I16, I32, I64, F32, F64, V128 }

impl From<Width> for Lane {
    fn from(width: Width) -> Self {
        match width {
            Width::I32 => Self::I32,
            Width::I64 => Self::I64,
        }
    }
}

impl From<Float> for Lane {
    fn from(float: Float) -> Self {
        match float {
            Float::F32 => Self::F32,
            Float::F64 => Self::F64,
        }
    }
}

/// How a load narrower than its result extends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extend { Zero, Sign }

/// A two-operand row: integer arithmetic, bitwise and division, or float arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binary {
    Add, Sub, Mul, MulHighSigned, MulHighUnsigned,
    And, Or, Xor, ShiftLeft, ShiftRightUnsigned, ShiftRightSigned, RotateLeft, RotateRight,
    DivideSigned, DivideUnsigned, RemainderSigned, RemainderUnsigned,
    FloatAdd, FloatSub, FloatMul, FloatDiv, FloatMax, FloatMin, FloatCopySign,
}

/// A comparison row: its flag is a word that holds 0 or 1 (an `i8` lane); the integer
/// relations test the operands at their own width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Equal, NotEqual,
    SignedLess, SignedLessEqual, SignedGreater, SignedGreaterEqual,
    UnsignedLess, UnsignedLessEqual, UnsignedGreater, UnsignedGreaterEqual,
    FloatEqual, FloatNotEqual, FloatLess, FloatLessEqual, FloatGreater, FloatGreaterEqual, FloatUnordered,
    FloatUnorderedEqual,
}

/// What a language operator is on the machine: a comparison row, a two-operand row, or the
/// signed remainder, a row behind the guard of its overflowing quotient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineOperator {
    Relation(Relation),
    Binary(Binary),
    SignedRemainder,
}

/// The order a comparison tests, apart from the domain it reads its operands in: a backend
/// builds its condition codes from it (`Relation::order`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order { Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual, Unordered, UnorderedEqual }

impl Relation {
    /// The order the relation tests, and whether it reads its operands as floats.
    pub const fn order(self) -> (Order, bool) {
        match self {
            Self::Equal => (Order::Equal, false),
            Self::NotEqual => (Order::NotEqual, false),
            Self::SignedLess | Self::UnsignedLess => (Order::Less, false),
            Self::SignedLessEqual | Self::UnsignedLessEqual => (Order::LessEqual, false),
            Self::SignedGreater | Self::UnsignedGreater => (Order::Greater, false),
            Self::SignedGreaterEqual | Self::UnsignedGreaterEqual => (Order::GreaterEqual, false),
            Self::FloatEqual => (Order::Equal, true),
            Self::FloatNotEqual => (Order::NotEqual, true),
            Self::FloatLess => (Order::Less, true),
            Self::FloatLessEqual => (Order::LessEqual, true),
            Self::FloatGreater => (Order::Greater, true),
            Self::FloatGreaterEqual => (Order::GreaterEqual, true),
            Self::FloatUnordered => (Order::Unordered, true),
            Self::FloatUnorderedEqual => (Order::UnorderedEqual, true),
        }
    }

    /// Whether the integer relation reads its operands as signed.
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::SignedLess | Self::SignedLessEqual | Self::SignedGreater | Self::SignedGreaterEqual)
    }
}

/// A one-operand row: a conversion to the lane it names, a bit count, or a float rounding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unary {
    ZeroExtend(Lane), SignExtend(Lane), Narrow(Lane),
    Promote, Demote, Bitcast(Lane),
    FromSigned(Lane), FromUnsigned(Lane), ToSignedSaturating(Lane), ToUnsignedSaturating(Lane),
    LeadingZeros, TrailingZeros, PopulationCount,
    FloatNegate, FloatAbsolute, Ceiling, Floor, Truncate, Nearest, SquareRoot,
}
