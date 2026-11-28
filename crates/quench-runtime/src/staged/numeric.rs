//! Number x Number semantics over `Stage`/`Frame`, shared by the interpreter and native tiers.

use super::value::language;
use super::typing;
use super::{Frame, Stage, Step};
use crate::bytecode::{Operand, Register};
use crate::typed_ir::Type;
#[cfg(not(stencil_build))]
use crate::vm::operations::binary_operator;
use crate::vm::operations::{
    int32_bitwise_bits, CheckedInt32Stage, NumberArithmetic, NumberArithmeticStage, NumberBitwise, NumberF64Stage,
    NumberRelation, NumberUnary, PartialOrderStage,
};

#[path = "numeric/frame_trait.rs"]
mod frame_trait;
#[path = "numeric/operand.rs"]
mod operand;
#[path = "numeric/math_ops.rs"]
mod math_ops;
#[path = "numeric/unary.rs"]
mod unary;
#[path = "numeric/bitwise.rs"]
mod bitwise;
#[path = "numeric/int_case.rs"]
mod int_case;
#[path = "numeric/decode.rs"]
mod decode;
#[path = "numeric/relation.rs"]
mod relation;
#[path = "numeric/arithmetic.rs"]
mod arithmetic;
#[path = "numeric/remainder.rs"]
mod remainder;
#[path = "numeric/increment.rs"]
mod increment;
pub use frame_trait::*;
use operand::*;
use math_ops::*;
use unary::*;
pub use bitwise::*;
use int_case::*;
pub use decode::*;
pub use relation::*;
use arithmetic::*;
use remainder::*;
use increment::*;

/// The representation both operands and the result of a Number operation have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    Int32,
    Float64,
}

/// What an int32 operation does at an overflow or a negative zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overflow {
    /// Leave at the frame's miss edge.
    Exit,
    /// Keep the wrapping int32 result.
    Wrap,
    /// Continue as the binary64 result.
    Widen,
    /// A tier's range proof makes the exact int32 result fit: no overflow test, computed as `Wrap`.
    Impossible,
}

/// What an int32 product does at a zero whose sign is negative, which no int32 holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegativeZero {
    /// Leave at the frame's miss edge.
    Exit,
    /// Keep the int32 zero: a tier that proved no consumer reads its sign, or that no operand
    /// pair makes one, selects it.
    Ignore,
}

/// The int32 operation that leaves at an overflow and at a negative zero.
const LEAVES: (Overflow, NegativeZero) = (Overflow::Exit, NegativeZero::Exit);

/// A Number operation `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub enum NumberOperation {
    /// Number x Number arithmetic with both operands in `domain`.
    Arithmetic { destination: Register, left: Operand, right: Operand,
        operation: NumberArithmetic, domain: Domain, overflow: Overflow, zero: NegativeZero },
    /// Number x Number relation, producing its predicate word.
    Relation { left: Operand, right: Operand, relation: NumberRelation, domain: Domain },
    /// Number x Number relation of two operands in `domain`, defining its Boolean in `destination`
    /// (a `Binary` of a relation operator, whose value is the Boolean the relation answers).
    RelationValue { destination: Register, left: Operand, right: Operand, relation: NumberRelation, domain: Domain },
    /// Bitwise `operation` of two operands' ToInt32, defining the int32 it makes in `destination`
    /// (a `Binary` of a bitwise operator). A zero-fill shift of a negative dividend by a zero count
    /// is above `i32::MAX`, which no int32 holds: it leaves.
    Bitwise { destination: Register, left: Operand, right: Operand, operation: NumberBitwise },
    /// `left % right` with both operands in `domain`. An int32 result that is no int32 (a
    /// zero divisor's NaN, a negative dividend's `-0`) leaves. A tier that proved the divisor no
    /// zero (`divisor_nonzero`) skips that test, and one whose consumers read no sign of a zero
    /// (`zero: Ignore`) keeps the int32 zero of a negative dividend.
    Remainder { destination: Register, left: Operand, right: Operand, domain: Domain,
        zero: NegativeZero, divisor_nonzero: bool },
    /// `source ± 1` (`++`, or `--` where `decrement`) of an int32 `source`.
    IncDec { destination: Register, source: Register, decrement: bool, overflow: Overflow },
    /// `+source` or `-source` of a Number `source`, in `domain`; any other value leaves.
    Unary { destination: Register, source: Operand, operation: NumberUnary, domain: Domain },
    /// `~source` of an int32 `source`; any other value leaves.
    BitwiseNot { destination: Register, source: Operand },
    /// ToNumeric of a Number `source` into `destination`; any other value leaves.
    ToNumeric { destination: Register, source: Register },
    /// The Math function `operation` (`Math.sqrt`, `Math.abs`, ...) of a Number `source`, a
    /// double: `staged::math::float_unary`, the one definition the natives run. Any other value leaves.
    /// The b-jit's stencils run it; the runtime's natives call `float_unary` itself.
    #[cfg_attr(not(stencil_build), allow(dead_code))]
    Math { destination: Register, source: Operand, operation: super::math::FloatUnary },
    /// `Math.max` (`maximum`) or `Math.min` of two Numbers, a double: `staged::math::float_extreme`.
    #[cfg_attr(not(stencil_build), allow(dead_code))]
    Extreme { destination: Register, left: Operand, right: Operand, maximum: bool },
    /// `Math.imul` of two int32s: `staged::math::multiply`, the wrapping product. Any other value leaves.
    #[cfg_attr(not(stencil_build), allow(dead_code))]
    Imul { destination: Register, left: Operand, right: Operand },
    /// `base ** exponent` (`Math.pow`) of a Number base, where `exponent` is a constant `power::SmallPower` names:
    /// `power::power_by`. Any other exponent, or a base that is no Number, leaves to the VM's power.
    Power { destination: Register, base: Operand, exponent: Operand },
    /// The case slot of a `switch` over the dense int labels `minimum..=maximum` (`count` slots, the
    /// slot of label `i` at `i - minimum`): the slot of `source` where it is an int32 or an integral
    /// double in the run (`===` of that label), else `count`. Every value is decided: none leaves.
    IntCase { destination: Register, source: Operand, minimum: i32, maximum: i32, count: u16 },
}

impl NumberOperation {
    /// The type of the value this operation defines for proven-number operands, where its row continues. An arithmetic
    /// or an increment is an int32 where its overflow leaves rather than widens, and a Number otherwise; a remainder is an
    /// int32 in the int32 domain; a bitwise, `~` or `Math.imul` is an int32 (`>>>` excepted: its uint32 result may pass
    /// `i32::MAX`, so it is a Number); a relation defines a Boolean; the Math functions and `ToNumeric` are Numbers, and so
    /// is a unary in the int32 domain, whose operand that is no int32 takes the float row.
    #[cfg_attr(stencil_build, allow(dead_code))]
    pub fn result_domain(self) -> Type {
        match self {
            Self::Arithmetic { operation, domain, overflow, .. } =>
                if !matches!(operation, NumberArithmetic::Divide) && domain == Domain::Int32 && overflow != Overflow::Widen { Type::INT32 } else { Type::NUMBER },
            Self::Relation { .. } | Self::RelationValue { .. } => Type::languages(language::BOOLEAN),
            Self::Bitwise { operation, .. } => if operation == NumberBitwise::ShiftRightUnsigned { Type::NUMBER } else { Type::INT32 },
            Self::BitwiseNot { .. } | Self::Imul { .. } | Self::IntCase { .. } => Type::INT32,
            Self::Remainder { domain, .. } => if domain == Domain::Int32 { Type::INT32 } else { Type::NUMBER },
            Self::IncDec { overflow, .. } => if overflow == Overflow::Widen { Type::NUMBER } else { Type::INT32 },
            // No operand type is known here, so the `typing` rules are applied to the Number this domain names (`Type::NUMBER`,
            // which may be NaN); each rule then gives the NaN-tolerant Number, the same one rule the solve applies to real operands.
            Self::Unary { operation, .. } => typing::unary(operation, Type::NUMBER),
            Self::Math { operation, .. } => typing::math(operation, Type::NUMBER),
            Self::Extreme { maximum, .. } => typing::extreme(maximum, Type::NUMBER, Type::NUMBER),
            Self::Power { .. } => Type::NUMBER,
            Self::ToNumeric { .. } => Type::NUMBER,
        }
    }

    /// Whether the left operand is the result where the right operand is the int32 `constant`: `x + 0` (int32 domain),
    /// `x - 0`, `x * 1` (any domain), and `x | 0`, `x ^ 0`, `x << 0`, `x >> 0`, `x & -1` (on an int32 `x`, which the bitwise
    /// operator reads as ToInt32). A commutative operation with the constant on the left is the same identity: the caller
    /// swaps the operands first.
    #[cfg_attr(stencil_build, allow(dead_code))]
    pub fn identity(self, constant: i32) -> bool {
        let neutral = match self {
            Self::Arithmetic { operation: NumberArithmetic::Add, domain: Domain::Int32, .. }
            | Self::Arithmetic { operation: NumberArithmetic::Subtract, .. } => 0,
            Self::Arithmetic { operation: NumberArithmetic::Multiply, .. } => 1,
            Self::Bitwise { operation: NumberBitwise::Or | NumberBitwise::Xor | NumberBitwise::ShiftLeft | NumberBitwise::ShiftRight, .. } => 0,
            Self::Bitwise { operation: NumberBitwise::And, .. } => -1,
            _ => return false,
        };
        constant == neutral
    }
}

/// Run the Number `operation` on `frame`; its word, or `Unresolved` where it left.
#[inline(always)]
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: NumberOperation) -> Step<S::Word, F::Control> {
    step_of::<{ kind::ANY }, S, F>(stage, frame, operation)
}

/// The `NumberOperation` variants as numbers, for `step_of`'s compile-time choice of arm: `ANY` runs every arm.
pub mod kind {
    pub const ANY: u8 = 0;
    pub const ARITHMETIC: u8 = 1;
    pub const RELATION: u8 = 2;
    pub const RELATION_VALUE: u8 = 3;
    pub const BITWISE: u8 = 4;
    pub const REMAINDER: u8 = 5;
    pub const INC_DEC: u8 = 6;
    pub const BITWISE_NOT: u8 = 7;
    pub const TO_NUMERIC: u8 = 8;
    pub const UNARY: u8 = 9;
    pub const MATH: u8 = 10;
    pub const EXTREME: u8 = 11;
    pub const IMUL: u8 = 12;
    pub const INT_CASE: u8 = 13;
    pub const POWER: u8 = 14;
}

/// `step` where the caller knows the operation's variant `KIND` (`kind`) at compile time: every other arm is
/// unresolved in a `const` test, which rustc drops before code generation, so a stencil row instantiates only its
/// own arm. `kind::ANY` is `step`.
#[inline(always)]
pub fn step_of<const KIND: u8, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: NumberOperation) -> Step<S::Word, F::Control> {
    // The general operator is decoded only once the words leave the row, so a decided row pays nothing for it.
    #[cfg(not(stencil_build))]
    let general = frame.general_operators().then_some(operation);
    #[cfg(stencil_build)]
    let general: Option<NumberOperation> = None;
    let step = match operation {
        NumberOperation::Arithmetic { destination, left, right, operation, domain, overflow, zero } =>
            if const { KIND == kind::ANY || KIND == kind::ARITHMETIC } { arithmetic(stage, frame, destination, left, right, operation, domain, (overflow, zero)) } else { Step::Unresolved },
        NumberOperation::Relation { left, right, relation, domain } =>
            if const { KIND == kind::ANY || KIND == kind::RELATION } { self::relation(stage, frame, left, right, relation, domain) } else { Step::Unresolved },
        NumberOperation::RelationValue { destination, left, right, relation, domain } =>
            if const { KIND == kind::ANY || KIND == kind::RELATION_VALUE } { relation_value(stage, frame, destination, left, right, relation, domain) } else { Step::Unresolved },
        NumberOperation::Bitwise { destination, left, right, operation } =>
            if const { KIND == kind::ANY || KIND == kind::BITWISE } { bitwise_value(stage, frame, destination, left, right, operation) } else { Step::Unresolved },
        NumberOperation::Remainder { destination, left, right, domain, zero, divisor_nonzero } =>
            if const { KIND == kind::ANY || KIND == kind::REMAINDER } { remainder(stage, frame, destination, left, right, domain, (zero, divisor_nonzero)) } else { Step::Unresolved },
        NumberOperation::IncDec { destination, source, decrement, overflow } =>
            if const { KIND == kind::ANY || KIND == kind::INC_DEC } { increment(stage, frame, destination, source, decrement, overflow) } else { Step::Unresolved },
        NumberOperation::BitwiseNot { destination, source } =>
            if const { KIND == kind::ANY || KIND == kind::BITWISE_NOT } { bitwise_not(stage, frame, destination, source) } else { Step::Unresolved },
        NumberOperation::ToNumeric { destination, source } =>
            if const { KIND == kind::ANY || KIND == kind::TO_NUMERIC } { to_numeric(stage, frame, destination, source) } else { Step::Unresolved },
        NumberOperation::Unary { destination, source, operation, domain } =>
            if const { KIND == kind::ANY || KIND == kind::UNARY } { unary(stage, frame, destination, source, operation, domain) } else { Step::Unresolved },
        NumberOperation::Math { destination, source, operation } =>
            if const { KIND == kind::ANY || KIND == kind::MATH } { math(stage, frame, destination, source, operation) } else { Step::Unresolved },
        NumberOperation::Extreme { destination, left, right, maximum } =>
            if const { KIND == kind::ANY || KIND == kind::EXTREME } { extreme(stage, frame, destination, (left, right), maximum) } else { Step::Unresolved },
        NumberOperation::Imul { destination, left, right } =>
            if const { KIND == kind::ANY || KIND == kind::IMUL } { imul(stage, frame, destination, left, right) } else { Step::Unresolved },
        NumberOperation::Power { destination, base, exponent } =>
            if const { KIND == kind::ANY || KIND == kind::POWER } { power(stage, frame, destination, base, exponent) } else { Step::Unresolved },
        NumberOperation::IntCase { destination, source, minimum, maximum, count } =>
            if const { KIND == kind::ANY || KIND == kind::INT_CASE } { int_case(stage, frame, destination, source, (minimum, maximum), count) } else { Step::Unresolved },
    };
    match (general, step) {
        #[cfg(not(stencil_build))]
        (Some(operation), Step::Unresolved) => match general_binary(&operation) {
            Some((call, destination)) => super::callout::calling(stage, frame, call, destination),
            None => Step::Unresolved,
        },
        (_, step) => step,
    }
}

/// The general binary operation a Number operation computes where its words do not decide it: the
/// `Binary` of its operator over its operands (`CallOut::Binary`), with the register it defines. `None`
/// for an operation that is no binary operator's. A relation's value is left out where it is `==` or `===`,
/// which one relation names both of: the `Binary` row of that record asks the VM for it.
#[cfg(not(stencil_build))]
#[inline(always)]
fn general_binary(operation: &NumberOperation) -> Option<(super::CallOut, Option<Register>)> {
    let (destination, left, right, operator) = match *operation {
        NumberOperation::Arithmetic { destination, left, right, operation, .. } => (destination, left, right, operation.binary_operator()),
        NumberOperation::Remainder { destination, left, right, .. } => (destination, left, right, binary_operator::Remainder),
        NumberOperation::Power { destination, base, exponent } => (destination, base, exponent, binary_operator::Exponential),
        NumberOperation::Bitwise { destination, left, right, operation } => (destination, left, right, operation.binary_operator()),
        NumberOperation::RelationValue { destination, left, right, relation, .. }
            if !matches!(relation, NumberRelation::Equal | NumberRelation::NotEqual) =>
            (destination, left, right, relation.binary_operator()),
        _ => return None,
    };
    Some((super::CallOut::Binary { left, right, operator }, Some(destination)))
}

#[path = "numeric/stage.rs"]
mod stage;

pub use stage::NumericStage;
#[path = "numeric/concrete.rs"]
mod concrete;

#[cfg(test)]
mod tests;
