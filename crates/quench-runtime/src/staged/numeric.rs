//! Number x Number semantics over `Stage`/`Frame`, shared by the interpreter and native tiers.

use super::{Frame, Stage, Step};
use crate::bytecode::{Operand, Register};
use crate::vm::operations::{
    int32_bitwise_bits, CheckedInt32Stage, NumberArithmetic, NumberArithmeticStage, NumberBitwise, NumberF64Stage,
    NumberRelation, NumberUnary, PartialOrderStage,
};

/// The representation both operands and the result of a Number operation have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Domain {
    Int32,
    Float64,
}

/// What an int32 operation does at an overflow or a negative zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Overflow {
    /// Leave at the frame's miss edge.
    Exit,
    /// Keep the wrapping int32 result.
    Wrap,
    /// Continue as the binary64 result.
    Widen,
}

/// What an int32 product does at a zero whose sign is negative, which no int32 holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NegativeZero {
    /// Leave at the frame's miss edge.
    Exit,
    /// Keep the int32 zero: a tier that proved no consumer reads its sign, or that no operand
    /// pair makes one, selects it.
    Ignore,
}

/// Machine primitives for the two Number representations: int32 words hold the unsigned
/// low 32 bits, float64 words hold their IEEE bits.
pub(crate) trait NumericStage: Stage {
    /// `relation` of two words in `domain`.
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: Self::Word, right: Self::Word) -> Self::Cond;
    /// `operation` of two binary64 words.
    fn float_arithmetic(&mut self, operation: NumberArithmetic,
        left: Self::Word, right: Self::Word) -> Self::Word;
    /// The binary64 negation of `value`: `-0` stays negative and NaN stays NaN.
    fn float_negate(&mut self, value: Self::Word) -> Self::Word;
    /// `operation` of two int32 words: the wrapped result and whether it overflowed.
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: Self::Word, right: Self::Word) -> (Self::Word, Self::Cond);
    /// The binary64 remainder of two binary64 words, keeping the dividend's sign.
    fn float_remainder(&mut self, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The truncated remainder of two int32 words, `right` nonzero: the dividend's sign, and
    /// `i32::MIN % -1` is zero rather than a trap.
    fn int_remainder(&mut self, left: Self::Word, right: Self::Word) -> Self::Word;
    /// `operation` of a binary64 word (`staged::math::float_unary`).
    fn float_unary(&mut self, operation: super::math::FloatUnary, value: Self::Word) -> Self::Word;
    /// `Math.max` (`maximum`) or `Math.min` of two binary64 words (`staged::math::float_extreme`).
    fn float_extreme(&mut self, maximum: bool, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The wrapping product of two int32 words (`Math.imul`; `staged::math::int_multiply`).
    fn int_multiply(&mut self, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The int32 word holding `value`.
    fn i32_constant(&mut self, value: i32) -> Self::Word;
    /// The binary64 word holding `value`.
    fn f64_constant(&mut self, value: f64) -> Self::Word;
    /// The int32 word holding the low 32 bits of the 64-bit word `value`.
    fn narrow(&mut self, value: Self::Word) -> Self::Word;
    fn i32_to_f64(&mut self, value: Self::Word) -> Self::Word;
    /// The binary64 of the unsigned low 32 bits of `value`.
    fn u32_to_f64(&mut self, value: Self::Word) -> Self::Word;
    /// ToInt32 of a binary64 word, as an int32 word; a specializer whose native range is narrower
    /// than ToInt32's leaves at `miss` for what it does not convert.
    fn to_int32(&mut self, number: Self::Word, miss: Self::Miss) -> Option<Self::Word>;
    /// The int32 word `operation` makes of two int32 words (a shift takes its count modulo 32).
    fn int_bitwise(&mut self, operation: NumberBitwise, left: Self::Word, right: Self::Word) -> Self::Word;
    fn i32_is_zero(&mut self, value: Self::Word) -> Self::Cond;
    fn i32_is_negative(&mut self, value: Self::Word) -> Self::Cond;
    fn both(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn either(&mut self, a: Self::Cond, b: Self::Cond) -> Self::Cond;
    fn cond_word(&mut self, cond: Self::Cond) -> Self::Word;
    /// Continue only where `cond` is false.
    fn guard_false(&mut self, cond: Self::Cond, miss: Self::Miss) -> Option<()>;
}

/// A frame whose operands a numeric operation proves in its representation and whose result
/// it defines with that representation's fact.
pub(crate) trait NumericFrame<S: Stage>: Frame<S> + super::GlobalFrame<S> + super::PropertyFrame<S> + super::ElementFrame<S> + super::CallFrame<S> {
    /// The int32 word of `operand`, proved here; `None` where it is none (a frame whose
    /// facts prove it, or whose guard branches away at `miss`, always answers `Some`).
    fn int_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<S::Word>;
    /// The binary64 word of `operand`, proved here; `None` where it is no Number.
    fn number_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<S::Word>;
    /// Continue where `register` holds a Number; `None` where it does not, so nothing runs
    /// past it. A specializer may prove a narrower representation its facts favor (an
    /// untyped register taken for an int32), leaving at `miss` for the rest.
    fn prove_number(&mut self, stage: &mut S, register: Register, miss: S::Miss) -> Option<()>;
    fn miss(&self) -> S::Miss;

    /// The tagged word of `operand`: a register's, or a constant the frame holds; `None` for any
    /// other operand, which leaves the row to the interpreter.
    #[inline(always)]
    fn operand_word(&mut self, stage: &mut S, operand: Operand) -> Option<S::Word> {
        operand.register_index().map(|register| self.tagged(stage, register))
    }

    /// `operand`'s ToInt32, as a bitwise operator reads it: an int32 as it is, a Number by
    /// truncation; `None` where it is no Number, which leaves the row to the interpreter. A frame
    /// that proves int32 operands only reads them as such.
    #[inline(always)]
    fn int32_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<S::Word> {
        self.int_operand(stage, operand, miss)
    }

    /// The encoded prototype link (`ProtoLink::bits`) of `constructor`'s `prototype`, where the
    /// frame holds it as a Stable fact together with the constructor's absent `Symbol.hasInstance`
    /// and its plain, unbound call: a dependency on both retires the code that relied on them
    /// where one changes, so an `instanceof` of `constructor` walks the value's chain by words.
    /// `None` where the frame holds no such fact, which leaves `instanceof` to the VM.
    #[inline(always)]
    fn stable_prototype(&mut self, _stage: &mut S, _constructor: Register) -> Option<S::Word> { None }

    /// Whether an IsHTMLDDA object can exist (the host provides one): a heap cell other than an
    /// object or array record may then be it, which no word decides.
    #[inline(always)]
    fn html_dda_possible(&self) -> bool { true }

    /// The miss edge a local access uses, where this frame guards it. A frame whose locals
    /// are plain storage reads and writes them without a guard returns `None`, and its
    /// `load_local`/`store_local` then ignore the edge.
    #[inline(always)]
    fn local_miss(&self) -> Option<S::Miss> { Some(self.miss()) }

    /// The word `write` takes for the int32 `word` this frame defines with `int_fact`: the
    /// int32 word itself, unless the frame keeps its registers tagged.
    #[inline(always)]
    fn int_result(&mut self, _stage: &mut S, word: S::Word) -> S::Word { word }

    fn int_fact(&self) -> Self::Fact;
    fn number_fact(&self) -> Self::Fact;
}

/// The int32 operation that leaves at an overflow and at a negative zero.
const LEAVES: (Overflow, NegativeZero) = (Overflow::Exit, NegativeZero::Exit);

/// A Number operation `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum NumberOperation {
    /// Number x Number arithmetic with both operands in `domain`.
    Arithmetic { destination: Register, left: Operand, right: Operand,
        operation: NumberArithmetic, domain: Domain, overflow: Overflow, zero: NegativeZero },
    /// Number x Number relation, producing its predicate word.
    Relation { left: Operand, right: Operand, relation: NumberRelation, domain: Domain },
    /// `left % right` with both operands in `domain`. An int32 result that is no int32 (a
    /// zero divisor's NaN, a negative dividend's `-0`) leaves.
    Remainder { destination: Register, left: Operand, right: Operand, domain: Domain },
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
    /// `Math.imul` of two int32s: `staged::math::int_multiply`. Any other value leaves.
    #[cfg_attr(not(stencil_build), allow(dead_code))]
    Imul { destination: Register, left: Operand, right: Operand },
}

/// Run the Number `operation` on `frame`; its word, or `Unresolved` where it left.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: NumberOperation) -> Step<S::Word, F::Control> {
    match operation {
        NumberOperation::Arithmetic { destination, left, right, operation, domain, overflow, zero } =>
            arithmetic(stage, frame, destination, left, right, operation, domain, (overflow, zero)),
        NumberOperation::Relation { left, right, relation, domain } =>
            self::relation(stage, frame, left, right, relation, domain),
        NumberOperation::Remainder { destination, left, right, domain } =>
            remainder(stage, frame, destination, left, right, domain),
        NumberOperation::IncDec { destination, source, decrement, overflow } =>
            increment(stage, frame, destination, source, decrement, overflow),
        NumberOperation::BitwiseNot { destination, source } => bitwise_not(stage, frame, destination, source),
        NumberOperation::ToNumeric { destination, source } => to_numeric(stage, frame, destination, source),
        NumberOperation::Unary { destination, source, operation, domain } =>
            unary(stage, frame, destination, source, operation, domain),
        NumberOperation::Math { destination, source, operation } => math(stage, frame, destination, source, operation),
        NumberOperation::Extreme { destination, left, right, maximum } => extreme(stage, frame, destination, (left, right), maximum),
        NumberOperation::Imul { destination, left, right } => imul(stage, frame, destination, left, right),
    }
}

/// The Math function `operation` of one Number `source`, as a double.
#[inline(always)]
fn math<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand, operation: super::math::FloatUnary,
) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(value) = frame.number_operand(stage, source, miss) else { return Step::Unresolved };
    let result = super::math::unary(stage, operation, value);
    let fact = frame.number_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `Math.max` or `Math.min` of two Numbers, a double; any other operand leaves.
#[inline(always)]
fn extreme<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, (left, right): (Operand, Operand), maximum: bool,
) -> Step<S::Word, F::Control> {
    let Some((left, right)) = operands(stage, frame, left, right, Domain::Float64) else { return Step::Unresolved };
    let result = super::math::extreme(stage, maximum, left, right);
    let fact = frame.number_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `Math.imul` of two int32s: their wrapping product; any other operand leaves.
#[inline(always)]
fn imul<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
) -> Step<S::Word, F::Control> {
    let Some((left, right)) = operands(stage, frame, left, right, Domain::Int32) else { return Step::Unresolved };
    let result = super::math::multiply(stage, left, right);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `operation` of one Number `source` in `domain`; a negation of `i32::MIN` widens and
/// leaves at the miss edge, since its Number result is no int32.
#[inline(always)]
fn unary<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand, operation: NumberUnary, domain: Domain,
) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match domain {
        Domain::Float64 => {
            let Some(value) = frame.number_operand(stage, source, miss) else { return Step::Unresolved };
            let result = match operation {
                NumberUnary::Plus => value,
                NumberUnary::Negate => stage.float_negate(value),
            };
            let fact = frame.number_fact();
            frame.write(stage, destination, result, fact);
            Step::Value(result)
        }
        Domain::Int32 => {
            let Some(value) = frame.int_operand(stage, source, miss) else { return Step::Unresolved };
            match operation {
                NumberUnary::Plus => {
                    let fact = frame.int_fact();
                    frame.write(stage, destination, value, fact);
                    Step::Value(value)
                }
                NumberUnary::Negate => {
                    // `-0` is no int32: a zero operand leaves, as `i32::MIN`'s overflow does.
                    let zero_operand = stage.i32_is_zero(value);
                    if stage.guard_false(zero_operand, miss).is_none() {
                        return Step::Unresolved;
                    }
                    let zero = stage.i32_constant(0);
                    int_arithmetic_result(stage, frame, destination, zero, value,
                        NumberArithmetic::Subtract, LEAVES, miss)
                }
            }
        }
    }
}

/// `~source` of an int32 `source`: `-1 - source`, which never overflows.
#[inline(always)]
fn bitwise_not<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(value) = frame.int_operand(stage, source, miss) else { return Step::Unresolved };
    let all_ones = stage.i32_constant(-1);
    int_arithmetic_result(stage, frame, destination, all_ones, value,
        NumberArithmetic::Subtract, LEAVES, miss)
}

/// Both operands in `domain`'s representation, each proved by the frame; `None` where either
/// is not.
#[inline(always)]
fn operands<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, domain: Domain) -> Option<(S::Word, S::Word)> {
    let miss = frame.miss();
    Some(match domain {
        Domain::Int32 => (frame.int_operand(stage, left, miss)?, frame.int_operand(stage, right, miss)?),
        Domain::Float64 => (frame.number_operand(stage, left, miss)?, frame.number_operand(stage, right, miss)?),
    })
}

/// A relation of two operands in `domain`; its predicate word.
#[inline(always)]
fn relation<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Step<S::Word, F::Control> {
    let Some(holds) = relation_holds(stage, frame, left, right, relation, domain) else { return Step::Unresolved };
    Step::Value(stage.cond_word(holds))
}

/// Whether `relation` holds of two operands in `domain`; `None` where the frame proves neither.
#[inline(always)]
pub(super) fn relation_holds<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Option<S::Cond> {
    let (left, right) = operands(stage, frame, left, right, domain)?;
    Some(stage.relation(relation, domain, left, right))
}

/// Arithmetic of two operands in `domain`; the result in `domain` or widened to binary64.
#[inline(always)]
fn arithmetic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
    operation: NumberArithmetic, domain: Domain, overflow: (Overflow, NegativeZero)) -> Step<S::Word, F::Control> {
    let Some((left, right)) = operands(stage, frame, left, right, domain) else { return Step::Unresolved };
    match domain {
        Domain::Float64 => {
            let result = stage.float_arithmetic(operation, left, right);
            let fact = frame.number_fact();
            frame.write(stage, destination, result, fact);
            Step::Value(result)
        }
        Domain::Int32 => {
            let miss = frame.miss();
            int_arithmetic_result(stage, frame, destination, left, right, operation, overflow, miss)
        }
    }
}

/// `left % right` in `domain`. An int32 remainder is an int32 except at a zero divisor (NaN)
/// and at a zero with a negative dividend (`-0`, `i32::MIN % -1` included): those leave.
#[inline(always)]
fn remainder<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, domain: Domain) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some((left, right)) = operands(stage, frame, left, right, domain) else { return Step::Unresolved };
    let (result, fact) = match domain {
        Domain::Float64 => (stage.float_remainder(left, right), frame.number_fact()),
        Domain::Int32 => {
            let zero_divisor = stage.i32_is_zero(right);
            if stage.guard_false(zero_divisor, miss).is_none() {
                return Step::Unresolved;
            }
            let result = stage.int_remainder(left, right);
            let zero = stage.i32_is_zero(result);
            let negative = stage.i32_is_negative(left);
            let negative_zero = stage.both(zero, negative);
            if stage.guard_false(negative_zero, miss).is_none() {
                return Step::Unresolved;
            }
            (frame.int_result(stage, result), frame.int_fact())
        }
    };
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// ToNumeric of a Number `source`: the Number itself, copied with its facts. Any other
/// value leaves (the interpreter converts a BigInt or a primitive by its general path).
#[inline(always)]
fn to_numeric<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match frame.prove_number(stage, source, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => Step::Unresolved,
    }
}

/// `source ± 1` (`++`/`--`) of an int32 `source`: the int32 sum or difference with one.
#[inline(always)]
fn increment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register, decrement: bool,
    overflow: Overflow) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let operation = if decrement { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
    let Some(value) = frame.int_operand(stage, Operand::register(source), miss) else {
        // A stage that found the operand no int32 (the interpreter's, or a tier that knows it a
        // binary64) steps the binary64 by one.
        let Some(number) = frame.number_operand(stage, Operand::register(source), miss) else { return Step::Unresolved };
        let one = stage.f64_constant(1.0);
        let result = stage.float_arithmetic(operation, number, one);
        let fact = frame.number_fact();
        frame.write(stage, destination, result, fact);
        return Step::Value(result);
    };
    let one = stage.i32_constant(1);
    int_arithmetic_result(stage, frame, destination, value, one, operation, (overflow, NegativeZero::Exit), miss)
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn int_arithmetic_result<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left_value: S::Word, right_value: S::Word,
    operation: NumberArithmetic, (overflow, zero): (Overflow, NegativeZero), miss: S::Miss) -> Step<S::Word, F::Control> {
    let (result, overflowed) = stage.int_arithmetic(operation, left_value, right_value);
    if overflow == Overflow::Exit && stage.guard_false(overflowed, miss).is_none() {
        return Step::Unresolved;
    }
    let negative_zero = negative_zero(stage, operation, left_value, right_value, result)
        .filter(|_| zero == NegativeZero::Exit || overflow == Overflow::Widen);
    if overflow == Overflow::Widen {
        let widen = match negative_zero {
            Some(negative_zero) => stage.either(overflowed, negative_zero),
            None => overflowed,
        };
        // A stage that knows whether the result widens (the interpreter's) writes the int32 or
        // the binary64 it is, rather than the binary64 of either.
        match stage.known(widen) {
            Some(false) => {
                let result = frame.int_result(stage, result);
                let fact = frame.int_fact();
                frame.write(stage, destination, result, fact);
                return Step::Value(result);
            }
            Some(true) => {
                let left = stage.i32_to_f64(left_value);
                let right = stage.i32_to_f64(right_value);
                let number = stage.float_arithmetic(operation, left, right);
                let fact = frame.number_fact();
                frame.write(stage, destination, number, fact);
                return Step::Value(number);
            }
            None => {}
        }
        let number = stage.choose(widen, |stage| {
            let left = stage.i32_to_f64(left_value);
            let right = stage.i32_to_f64(right_value);
            stage.float_arithmetic(operation, left, right)
        }, |stage| stage.i32_to_f64(result));
        let fact = frame.number_fact();
        frame.write(stage, destination, number, fact);
        return Step::Value(number);
    }
    if let Some(negative_zero) = negative_zero
        && stage.guard_false(negative_zero, miss).is_none() {
        return Step::Unresolved;
    }
    let result = frame.int_result(stage, result);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `-0` a zero int32 product yields, which no int32 holds: a zero result with a negative
/// sign on either factor.
#[inline(always)]
fn negative_zero<S: NumericStage>(stage: &mut S, operation: NumberArithmetic,
    left: S::Word, right: S::Word, result: S::Word) -> Option<S::Cond> {
    (operation == NumberArithmetic::Multiply).then(|| {
        let zero = stage.i32_is_zero(result);
        let signs = stage.or(left, right);
        let negative = stage.i32_is_negative(signs);
        stage.both(zero, negative)
    })
}

#[inline(always)]
fn int32(word: u64) -> i32 { word as u32 as i32 }

/// The interpreter's wrapping int32 result, from the same rows as every arithmetic stage.
struct WrappingInt32Stage;

macro_rules! wrapping_int32_arithmetic {
    ($($operation:ident => $operator:ident, $method:ident, $instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl NumberArithmeticStage<i32> for WrappingInt32Stage {
            type Output = i32;
            $(
                #[inline(always)]
                fn $method(&mut self, left: i32, right: i32) -> Self::Output {
                    wrapping_int32_arithmetic!(@wrapped left, right, $symbol)
                }
            )+
        }
    };
    (@wrapped $left:ident, $right:ident, +) => { $left.wrapping_add($right) };
    (@wrapped $left:ident, $right:ident, -) => { $left.wrapping_sub($right) };
    (@wrapped $left:ident, $right:ident, *) => { $left.wrapping_mul($right) };
    (@wrapped $left:ident, $right:ident, /) => {
        { let _ = ($left, $right); unreachable!("division has a binary64 result") }
    };
}
crate::vm::operations::number_arithmetic_rows!(wrapping_int32_arithmetic);

/// The sign bit of a binary64 word.
const F64_SIGN_BIT: u64 = 1 << (u64::BITS - 1);

impl NumericStage for super::Concrete {
    #[inline(always)]
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: u64, right: u64) -> bool {
        match domain {
            Domain::Int32 => relation.apply(&mut PartialOrderStage, int32(left), int32(right)),
            Domain::Float64 => relation.apply(&mut PartialOrderStage,
                f64::from_bits(left), f64::from_bits(right)),
        }
    }
    #[inline(always)]
    fn float_arithmetic(&mut self, operation: NumberArithmetic, left: u64, right: u64) -> u64 {
        operation.apply(&mut NumberF64Stage,
            f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    #[inline(always)]
    fn float_negate(&mut self, value: u64) -> u64 {
        // The sign bit flipped. A stencil has no constant pool to hold a mask for a value that lives in a
        // vector register, so the mask is an opaque word it makes in a general register.
        #[cfg(stencil_build)]
        let mask = core::hint::black_box(F64_SIGN_BIT);
        #[cfg(not(stencil_build))]
        let mask = F64_SIGN_BIT;
        value ^ mask
    }
    #[inline(always)]
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: u64, right: u64) -> (u64, bool) {
        let (left, right) = (int32(left), int32(right));
        let checked = operation.apply(&mut CheckedInt32Stage, left, right);
        let overflowed = checked.is_none();
        let result = checked.unwrap_or_else(|| operation.apply(&mut WrappingInt32Stage, left, right));
        (u64::from(result as u32), overflowed)
    }
    #[inline(always)]
    fn float_remainder(&mut self, left: u64, right: u64) -> u64 {
        crate::vm::operations::number_remainder(f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    #[inline(always)]
    fn int_remainder(&mut self, left: u64, right: u64) -> u64 {
        u64::from(int32(left).wrapping_rem(int32(right)) as u32)
    }
    #[inline(always)]
    fn float_unary(&mut self, operation: super::math::FloatUnary, value: u64) -> u64 {
        super::math::float_unary(operation, f64::from_bits(value)).to_bits()
    }
    #[inline(always)]
    fn float_extreme(&mut self, maximum: bool, left: u64, right: u64) -> u64 {
        super::math::float_extreme(maximum, f64::from_bits(left), f64::from_bits(right)).to_bits()
    }
    #[inline(always)]
    fn int_multiply(&mut self, left: u64, right: u64) -> u64 {
        u64::from(super::math::int_multiply(int32(left), int32(right)) as u32)
    }
    #[inline(always)]
    fn i32_constant(&mut self, value: i32) -> u64 { u64::from(value as u32) }
    #[inline(always)]
    fn f64_constant(&mut self, value: f64) -> u64 { value.to_bits() }
    #[inline(always)]
    fn narrow(&mut self, value: u64) -> u64 { u64::from(value as u32) }
    #[inline(always)]
    fn i32_to_f64(&mut self, value: u64) -> u64 { (i64::from(int32(value)) as f64).to_bits() }
    #[inline(always)]
    fn u32_to_f64(&mut self, value: u64) -> u64 { (i64::from(value as u32) as f64).to_bits() }
    #[inline(always)]
    fn to_int32(&mut self, number: u64, _miss: ()) -> Option<u64> {
        Some(u64::from(crate::units::to_int32(f64::from_bits(number)) as u32))
    }
    #[inline(always)]
    fn int_bitwise(&mut self, operation: NumberBitwise, left: u64, right: u64) -> u64 {
        u64::from(int32_bitwise_bits(operation, int32(left), int32(right)) as u32)
    }
    #[inline(always)]
    fn i32_is_zero(&mut self, value: u64) -> bool { int32(value) == 0 }
    #[inline(always)]
    fn i32_is_negative(&mut self, value: u64) -> bool { int32(value) < 0 }
    #[inline(always)]
    fn both(&mut self, a: bool, b: bool) -> bool { a && b }
    #[inline(always)]
    fn either(&mut self, a: bool, b: bool) -> bool { a || b }
    #[inline(always)]
    fn cond_word(&mut self, cond: bool) -> u64 { u64::from(cond) }
    #[inline(always)]
    fn guard_false(&mut self, cond: bool, _miss: ()) -> Option<()> { (!cond).then_some(()) }
}

#[cfg(test)]
mod tests;
