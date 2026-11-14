//! The op rows of the machine primitives: the one enum of each concept. A row names an
//! operation by its machine meaning, not a language's; rules that have a language meaning (a
//! Wasm operator, a JavaScript operation) map to a row once, in the backend that executes it.

use crate::staged::wasm::Width;
use crate::staged::wasm_float::Float;

/// The width and kind of a machine value or memory cell: an integer or float word, a narrow
/// integer cell, or a 128-bit vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane { I8, I16, I32, I64, F32, F64, V128 }

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
pub(crate) enum Extend { Zero, Sign }

/// A two-operand row: integer arithmetic, bitwise and division, or float arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binary {
    Add, Sub, Mul, MulHighSigned, MulHighUnsigned,
    And, Or, Xor, ShiftLeft, ShiftRightUnsigned, ShiftRightSigned, RotateLeft, RotateRight,
    DivideSigned, DivideUnsigned, RemainderSigned, RemainderUnsigned,
    FloatAdd, FloatSub, FloatMul, FloatDiv, FloatMax, FloatMin, FloatCopySign,
}

/// A comparison row: its flag is a word that holds 0 or 1 (an `i8` lane); the integer
/// relations test the operands at their own width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation {
    Equal, NotEqual,
    SignedLess, SignedLessEqual, SignedGreater, SignedGreaterEqual,
    UnsignedLess, UnsignedLessEqual, UnsignedGreater, UnsignedGreaterEqual,
    FloatEqual, FloatNotEqual, FloatLess, FloatLessEqual, FloatGreater, FloatGreaterEqual, FloatUnordered,
}

impl Relation {
    /// Whether the integer relation reads its operands as signed.
    pub(crate) const fn is_signed(self) -> bool {
        matches!(self, Self::SignedLess | Self::SignedLessEqual | Self::SignedGreater | Self::SignedGreaterEqual)
    }
}

/// A one-operand row: a conversion to the lane it names, a bit count, or a float rounding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unary {
    ZeroExtend(Lane), SignExtend(Lane), Narrow(Lane),
    Promote, Demote, Bitcast(Lane),
    FromSigned(Lane), FromUnsigned(Lane), ToSignedSaturating(Lane), ToUnsignedSaturating(Lane),
    LeadingZeros, TrailingZeros, PopulationCount,
    FloatNegate, FloatAbsolute, Ceiling, Floor, Truncate, Nearest, SquareRoot,
}
