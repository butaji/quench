//! Number x Number semantics over `Stage`/`Frame`, shared by the interpreter and native tiers.

use super::{Frame, Stage, Step};
use crate::bytecode::{Operand, Register};
#[cfg(not(stencil_build))]
use crate::vm::operations::binary_operator;
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
    /// A tier's range proof makes the exact int32 result fit: no overflow test, computed as `Wrap`.
    Impossible,
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

/// A frame whose operands a numeric operation proves in its representation and whose result
/// it defines with that representation's fact.
pub(crate) trait NumericFrame<S: Stage>: Frame<S> + super::GlobalFrame<S> + super::PropertyFrame<S> + super::ElementFrame<S> + super::CallFrame<S> {
    /// The int32 word of `operand`, proved here; `None` where it is none (a frame whose
    /// facts prove it, or whose guard branches away at `miss`, always answers `Some`).
    fn int_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<S::Word>;
    /// The binary64 word of `operand`, proved here; `None` where it is no Number.
    fn number_operand(&mut self, stage: &mut S, operand: Operand, miss: S::Miss) -> Option<S::Word>;
    /// The int32 `operand` holds where the frame already proved it one, read with no test; `None`
    /// where it did not (a frame that keeps no facts never has).
    #[inline(always)]
    fn held_int(&mut self, _stage: &mut S, _operand: Operand) -> Option<S::Word> { None }
    /// The program constant `operand` names, where it names one: the value a numeric operation folds into
    /// its stage (`int32_word`, `float_word`: `i32_constant`, `f64_constant`) in place of reading it, in every
    /// tier. `None` for a register or a local, and where the frame holds no constant table.
    #[inline(always)]
    fn constant_operand(&mut self, _operand: Operand) -> Option<crate::Value> { None }
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

    /// The fact a Number takes where the frame keeps Numbers tagged: an int32 boxed in place and a binary64
    /// canonicalized (`box_number`), defined once by `number_arithmetic`. `None` where the frame keeps every
    /// Number unboxed, and a row defines its binary64 result with `number_fact` alone.
    #[inline(always)]
    fn tagged_number_fact(&self) -> Option<Self::Fact> { None }

    /// Write `word`, of `fact`, to frame local slot `local` (a numeric local target); `false` where this
    /// frame keeps no locals of its own.
    #[inline(always)]
    fn store_numeric_local(&mut self, _stage: &mut S, _local: Register, _word: S::Word, _fact: Self::Fact) -> bool { false }

    /// Whether a Number row its words leave takes the VM's general operator in place (`CallOut::Binary`,
    /// `ToNumeric`, `IncDec`). A frame that keeps declining answers `false`, and the row stays unresolved.
    #[inline(always)]
    fn general_operators(&self) -> bool { false }

    /// Whether a store row its plan leaves takes the VM's store in place (`CallOut::StoreField`,
    /// `StoreElement`). A frame that keeps declining answers `false`, and the row stays unresolved.
    #[inline(always)]
    fn general_stores(&self) -> bool { false }

    /// Whether a load row its plan leaves takes the VM's read in place (`CallOut::LoadField`, `LoadElement`).
    /// A frame that keeps declining answers `false`, and the row stays unresolved.
    #[inline(always)]
    fn general_loads(&self) -> bool { false }
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
    /// The case slot of a `switch` over the dense int labels `minimum..=maximum` (`count` slots, the
    /// slot of label `i` at `i - minimum`): the slot of `source` where it is an int32 or an integral
    /// double in the run (`===` of that label), else `count`. Every value is decided: none leaves.
    IntCase { destination: Register, source: Operand, minimum: i32, maximum: i32, count: u16 },
}

/// A constant operand's int32 and Number readings (`NumericFrame::constant_operand`). The stencil
/// build holds no constant table, so there no constant reaches them.
#[cfg(not(stencil_build))]
#[inline(always)]
fn constant_int(value: crate::Value) -> Option<i32> { value.as_int() }
#[cfg(stencil_build)]
#[inline(always)]
fn constant_int(_value: crate::Value) -> Option<i32> { None }
#[cfg(not(stencil_build))]
#[inline(always)]
fn constant_number(value: crate::Value) -> Option<f64> { value.as_number() }
#[cfg(stencil_build)]
#[inline(always)]
fn constant_number(_value: crate::Value) -> Option<f64> { None }
/// ToInt32 of a constant Number (`crate::value::number_to_u32`'s bits).
#[cfg(not(stencil_build))]
#[inline(always)]
fn constant_int32(number: f64) -> i32 { crate::value::number_to_u32(number) as i32 }
#[cfg(stencil_build)]
#[inline(always)]
fn constant_int32(_number: f64) -> i32 { 0 }

/// Run the Number `operation` on `frame`; its word, or `Unresolved` where it left.
#[inline(always)]
pub(crate) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: NumberOperation) -> Step<S::Word, F::Control> {
    step_of::<{ kind::ANY }, S, F>(stage, frame, operation)
}

/// The `NumberOperation` variants as numbers, for `step_of`'s compile-time choice of arm: `ANY` runs every arm.
pub(crate) mod kind {
    pub(crate) const ANY: u8 = 0;
    pub(crate) const ARITHMETIC: u8 = 1;
    pub(crate) const RELATION: u8 = 2;
    pub(crate) const RELATION_VALUE: u8 = 3;
    pub(crate) const BITWISE: u8 = 4;
    pub(crate) const REMAINDER: u8 = 5;
    pub(crate) const INC_DEC: u8 = 6;
    pub(crate) const BITWISE_NOT: u8 = 7;
    pub(crate) const TO_NUMERIC: u8 = 8;
    pub(crate) const UNARY: u8 = 9;
    pub(crate) const MATH: u8 = 10;
    pub(crate) const EXTREME: u8 = 11;
    pub(crate) const IMUL: u8 = 12;
    pub(crate) const INT_CASE: u8 = 13;
}

/// `step` where the caller knows the operation's variant `KIND` (`kind`) at compile time: every other arm is
/// unresolved in a `const` test, which rustc drops before code generation, so a stencil row instantiates only its
/// own arm. `kind::ANY` is `step`.
#[inline(always)]
pub(crate) fn step_of<const KIND: u8, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: NumberOperation) -> Step<S::Word, F::Control> {
    #[cfg(not(stencil_build))]
    let general = if frame.general_operators() { general_binary(&operation) } else { None };
    #[cfg(stencil_build)]
    let general: Option<(super::CallOut, Option<Register>)> = None;
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
        NumberOperation::IntCase { destination, source, minimum, maximum, count } =>
            if const { KIND == kind::ANY || KIND == kind::INT_CASE } { int_case(stage, frame, destination, source, (minimum, maximum), count) } else { Step::Unresolved },
    };
    match (general, step) {
        (Some((call, destination)), Step::Unresolved) => super::callout::calling(stage, frame, call, destination),
        (_, step) => step,
    }
}

/// The general binary operation a Number operation computes where its words do not decide it: the
/// `Binary` of its operator over its operands (`CallOut::Binary`), with the register it defines. `None`
/// for an operation that is no binary operator's. A relation's value is left out where it is `==` or `===`,
/// which one relation names both of: the `Binary` row of that record asks the VM for it.
#[cfg(not(stencil_build))]
fn general_binary(operation: &NumberOperation) -> Option<(super::CallOut, Option<Register>)> {
    let (destination, left, right, operator) = match *operation {
        NumberOperation::Arithmetic { destination, left, right, operation, .. } => (destination, left, right, operation.binary_operator()),
        NumberOperation::Remainder { destination, left, right, .. } => (destination, left, right, binary_operator::Remainder),
        NumberOperation::Bitwise { destination, left, right, operation } => (destination, left, right, operation.binary_operator()),
        NumberOperation::RelationValue { destination, left, right, relation, .. }
            if !matches!(relation, NumberRelation::Equal | NumberRelation::NotEqual) =>
            (destination, left, right, relation.binary_operator()),
        _ => return None,
    };
    Some((super::CallOut::Binary { left, right, operator }, Some(destination)))
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

/// The case slot of `source` in a `switch` over the dense int labels `minimum..=maximum` (`count` slots). A value
/// is its own label's slot where it is an int32 or an integral double in the run; every other value (a string,
/// an object, a fraction) takes `count`, the default. The int32 word is ToInt32 of the Number, which is exact
/// for the integral ones, so the classes come from the staged tag tests and float steps only.
#[inline(always)]
fn int_case<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    source: Operand, (minimum, maximum): (i32, i32), count: u16) -> Step<S::Word, F::Control> {
    // An int32 the frame holds is its own integral value: only the label range decides its slot.
    if let Some(value) = frame.held_int(stage, source) {
        let slot = int_slot(stage, value, (minimum, maximum), count, None);
        let slot = frame.int_result(stage, slot);
        let fact = frame.int_fact();
        frame.write(stage, destination, slot, fact);
        return Step::Value(slot);
    }
    let Some(tagged) = frame.operand_word(stage, source) else { return Step::Unresolved };
    // An int word is read as the double it holds; any other word as the double its bits are.
    let narrowed = stage.narrow(tagged);
    let from_int = stage.i32_to_f64(narrowed);
    let int_tag = super::value::is_int(stage, tagged);
    let double_tag = super::value::is_double(stage, tagged);
    let number_tag = stage.either(int_tag, double_tag);
    let double = stage.f64_from_bits(tagged);
    let number = stage.select(int_tag, from_int, double);
    let truncated = stage.float_unary(super::math::FloatUnary::Trunc, number);
    let exact = stage.relation(NumberRelation::Equal, Domain::Float64, truncated, number);
    let lowest = stage.f64_constant(f64::from(i32::MIN));
    let highest = stage.f64_constant(f64::from(i32::MAX));
    let above = stage.relation(NumberRelation::GreaterEqual, Domain::Float64, truncated, lowest);
    let below = stage.relation(NumberRelation::LessEqual, Domain::Float64, truncated, highest);
    let integral = stage.both(exact, above);
    let integral = stage.both(integral, below);
    let integral = stage.both(integral, number_tag);
    let miss = frame.miss();
    let Some(value) = stage.to_int32(number, miss) else { return Step::Unresolved };
    let slot = int_slot(stage, value, (minimum, maximum), count, Some(integral));
    // The int's word as the frame holds an int (`int_result`: the interpreter's tagged int), as `typeof_index` defines its.
    let slot = frame.int_result(stage, slot);
    let fact = frame.int_fact();
    frame.write(stage, destination, slot, fact);
    Step::Value(slot)
}

/// The case slot of the int32 `value` among the labels from `minimum` to `maximum`: its offset
/// from `minimum` inside them (and where `integral` holds, the operand being that int), else `count`.
#[inline(always)]
fn int_slot<S: NumericStage>(stage: &mut S, value: S::Word, (minimum, maximum): (i32, i32), count: u16,
    integral: Option<S::Cond>) -> S::Word {
    let low = stage.i32_constant(minimum);
    let high = stage.i32_constant(maximum);
    let from_low = stage.relation(NumberRelation::GreaterEqual, Domain::Int32, value, low);
    let to_high = stage.relation(NumberRelation::LessEqual, Domain::Int32, value, high);
    let inside = match integral { Some(integral) => stage.both(integral, from_low), None => from_low };
    let inside = stage.both(inside, to_high);
    let (offset, _) = stage.int_arithmetic(NumberArithmetic::Subtract, value, low);
    let fallback = stage.i32_constant(i32::from(count));
    stage.select(inside, offset, fallback)
}

/// `Math.imul` of two int32s: their wrapping product, `math::multiply`; any other operand leaves.
#[inline(always)]
fn imul<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
) -> Step<S::Word, F::Control> {
    let Some((left, right)) = operands(stage, frame, left, right, Domain::Int32) else { return Step::Unresolved };
    let result = super::math::multiply(stage, left, right);
    let result = frame.int_result(stage, result);
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
            // A Number operand past the int32 domain (a double, `-0`, `i32::MIN`'s negation) takes the
            // float row before the VM's operator: only a non-Number leaves.
            let Some(value) = frame.int_operand(stage, source, miss) else {
                return float_or_general_unary(stage, frame, destination, source, operation);
            };
            match operation {
                NumberUnary::Plus => {
                    // The int's word as the frame holds an int (`int_result`): the operand is the raw int32.
                    let result = frame.int_result(stage, value);
                    let fact = frame.int_fact();
                    frame.write(stage, destination, result, fact);
                    Step::Value(result)
                }
                NumberUnary::Negate => {
                    // `-0` is no int32: a zero operand leaves, as `i32::MIN`'s overflow does; the VM's negation takes both.
                    let zero_operand = stage.i32_is_zero(value);
                    if stage.guard_false(zero_operand, miss).is_none() {
                        return float_or_general_unary(stage, frame, destination, source, operation);
                    }
                    let zero = stage.i32_constant(0);
                    match int_arithmetic_result(stage, frame, destination, zero, value,
                        NumberArithmetic::Subtract, LEAVES, miss) {
                        Step::Unresolved => float_or_general_unary(stage, frame, destination, source, operation),
                        step => step,
                    }
                }
            }
        }
    }
}

/// `unary` of a Number operand in the float domain, else the VM's operator (`general_unary`).
#[inline(always)]
fn float_or_general_unary<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand, operation: NumberUnary,
) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match frame.number_operand(stage, source, miss) {
        Some(value) => {
            let result = match operation {
                NumberUnary::Plus => value,
                NumberUnary::Negate => stage.float_negate(value),
            };
            let fact = frame.number_fact();
            frame.write(stage, destination, result, fact);
            Step::Value(result)
        }
        None => general_unary(stage, frame, unary_operator(operation), (destination, source)),
    }
}

/// The oxc unary operator `operation` names, which the VM's `unary` takes (`CallOut::Unary`).
#[cfg(not(stencil_build))]
fn unary_operator(operation: NumberUnary) -> u32 {
    match operation {
        NumberUnary::Plus => crate::vm::operations::unary_operator::UnaryPlus,
        NumberUnary::Negate => crate::vm::operations::unary_operator::UnaryNegation,
    }
}

/// The stencil build has no parser, and no stencil frame takes the VM's general operators
/// (`NumericFrame::general_operators`), so no operator code reaches a callout there.
#[cfg(stencil_build)]
fn unary_operator(_operation: NumberUnary) -> u32 {
    u32::MAX
}


/// The VM's unary `operator` of the register `source`, defining `destination`, where the frame takes
/// the general operator in place (`NumericFrame::general_operators`); otherwise the row stays unresolved.
#[inline(always)]
fn general_unary<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operator: u32,
    (destination, source): (Register, Operand)) -> Step<S::Word, F::Control> {
    match source.register_index() {
        Some(value) if frame.general_operators() =>
            super::callout::calling(stage, frame, super::CallOut::Unary { operator, value }, Some(destination)),
        _ => Step::Unresolved,
    }
}

/// `~source` of an int32 `source`: `-1 - source`, which never overflows.
#[inline(always)]
fn bitwise_not<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Operand) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(value) = frame.int_operand(stage, source, miss) else {
        return general_unary(stage, frame, crate::vm::operations::unary_operator::BitwiseNot, (destination, source));
    };
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
        Domain::Int32 => (int32_word(stage, frame, left, miss)?, int32_word(stage, frame, right, miss)?),
        Domain::Float64 => (float_word(stage, frame, left, miss)?, float_word(stage, frame, right, miss)?),
    })
}

/// `operand`'s int32 word: a constant int32 is the stage's own constant (`i32_constant`), so a constant
/// left operand folds in every tier; any other operand is `int_operand`'s.
#[inline(always)]
fn int32_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand, miss: S::Miss) -> Option<S::Word> {
    match frame.constant_operand(operand) {
        Some(value) => Some(stage.i32_constant(value.as_int()?)),
        None => frame.int_operand(stage, operand, miss),
    }
}

/// `operand`'s ToInt32 word for a bitwise operator: a constant's ToInt32 is the stage's own constant
/// (`i32_constant`), else `int32_operand`'s.
#[inline(always)]
pub(super) fn bitwise_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand, miss: S::Miss) -> Option<S::Word> {
    match frame.constant_operand(operand) {
        Some(value) => {
            let int = match value.as_int() {
                Some(int) => int,
                None => crate::value::number_to_u32(value.as_number()?) as i32,
            };
            Some(stage.i32_constant(int))
        }
        None => frame.int32_operand(stage, operand, miss),
    }
}

/// `operand`'s binary64 word: a constant Number is the stage's own constant (`f64_constant`), else
/// `number_operand`'s. Subsumes the constant-left Number handlers (`lane_operator_constant_left`, one per
/// operator): the operator is the row's, and the constant folds in every tier.
#[inline(always)]
fn float_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operand: Operand, miss: S::Miss) -> Option<S::Word> {
    match frame.constant_operand(operand) {
        Some(value) => Some(stage.f64_constant(value.as_number()?)),
        None => frame.number_operand(stage, operand, miss),
    }
}

/// A relation of two operands in `domain`; its predicate word.
#[inline(always)]
fn relation<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Step<S::Word, F::Control> {
    let Some(holds) = relation_holds(stage, frame, left, right, relation, domain) else { return Step::Unresolved };
    Step::Value(stage.cond_word(holds))
}

/// A relation of two operands in `domain`, defining its Boolean in `destination`.
#[inline(always)]
fn relation_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Step<S::Word, F::Control> {
    let Some(holds) = relation_holds(stage, frame, left, right, relation, domain) else { return Step::Unresolved };
    let (value, fact) = frame.boolean(stage, holds);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// The bitwise `operation` of two operands' ToInt32, defining its int32 in `destination`. A
/// zero-fill shift leaves where its count is zero and its dividend negative (the Number is above
/// `i32::MAX`, which no int32 holds).
#[inline(always)]
fn bitwise_value<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, operation: NumberBitwise) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some(left) = bitwise_word(stage, frame, left, miss) else { return Step::Unresolved };
    let Some(right) = bitwise_word(stage, frame, right, miss) else { return Step::Unresolved };
    if operation == NumberBitwise::ShiftRightUnsigned {
        let count = stage.and_imm(right, 31u64);
        let zero_count = stage.eq_imm(count, 0u64);
        let negative = stage.i32_is_negative(left);
        let widens = stage.both(zero_count, negative);
        // A frame of tagged words writes a result past int32 (`x >>> 0` of a negative `x`) as its boxed
        // binary64 word, under the one tagged fact; any other frame leaves there.
        if let Some(fact) = frame.tagged_number_fact() {
            let bits = stage.int_bitwise(operation, left, right);
            let int = frame.int_result(stage, bits);
            let result = stage.choose(widens, #[inline(always)] |stage| {
                let number = stage.u32_to_f64(left);
                stage.box_number(number)
            }, #[inline(always)] |_| int);
            frame.write(stage, destination, result, fact);
            return Step::Value(result);
        }
        if stage.guard_false(widens, miss).is_none() {
            return Step::Unresolved;
        }
    }
    let bits = stage.int_bitwise(operation, left, right);
    let result = frame.int_result(stage, bits);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// The Number operation a `Binary` of `operator` over `left` and `right` denotes, defining
/// `destination`: an arithmetic, a remainder, a bitwise or a relation. The decode cannot see the
/// operands' representations, so it names the widest domain (`Float64`; a bitwise is int32 by its
/// operator). `None` for any other operator (equality, `in`, `instanceof`): the VM's. The operator
/// tables are the operations' own (`NumberArithmetic::from_operator` and its siblings).
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn binary_operation(operator: u32, destination: Register, left: Operand, right: Operand) -> Option<NumberOperation> {
    if let Some(operation) = NumberArithmetic::from_operator(operator) {
        return Some(NumberOperation::Arithmetic { destination, left, right, operation, domain: Domain::Float64,
            overflow: Overflow::Widen, zero: NegativeZero::Ignore });
    }
    if operator == binary_operator::Remainder {
        return Some(NumberOperation::Remainder { destination, left, right, domain: Domain::Float64,
            zero: NegativeZero::Exit, divisor_nonzero: false });
    }
    if let Some(operation) = NumberBitwise::from_operator(operator) {
        return Some(NumberOperation::Bitwise { destination, left, right, operation });
    }
    NumberRelation::from_operator(operator).map(|relation|
        NumberOperation::RelationValue { destination, left, right, relation, domain: Domain::Float64 })
}

/// `binary_operation` for two operands an int32 constant decides: add, subtract and multiply in
/// `Int32`, leaving at an overflow and at a negative zero, and the remainder in `Int32`, leaving at a
/// zero divisor and a negative zero. A division has no int32 result, and the other operators are
/// `binary_operation`'s own.
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn int32_binary_operation(operator: u32, destination: Register, left: Operand, right: Operand) -> Option<NumberOperation> {
    let int32 = match NumberArithmetic::from_operator(operator) {
        Some(NumberArithmetic::Divide) => None,
        Some(operation) => Some(NumberOperation::Arithmetic { destination, left, right, operation, domain: Domain::Int32,
            overflow: Overflow::Exit, zero: NegativeZero::Exit }),
        None if operator == binary_operator::Remainder =>
            Some(NumberOperation::Remainder { destination, left, right, domain: Domain::Int32,
                zero: NegativeZero::Exit, divisor_nonzero: false }),
        None => None,
    };
    int32.or_else(|| binary_operation(operator, destination, left, right))
}

/// Whether `relation` holds of two operands in `domain`; `None` where the frame proves neither.
#[inline(always)]
pub(super) fn relation_holds<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand, relation: NumberRelation, domain: Domain) -> Option<S::Cond> {
    match domain {
        Domain::Int32 => {
            let (left, right) = operands(stage, frame, left, right, domain)?;
            Some(stage.relation(relation, domain, left, right))
        }
        Domain::Float64 => relation_number(stage, frame, (left, right), relation),
    }
}

/// `relation` of two Number operands: where both are int32 at run time it is one int32 compare, and every
/// other pair takes the binary64 compare (`relation_float`). A choice yields a predicate word, so the two
/// arms each keep their own compare, and the word is read back as the condition.
#[inline(always)]
fn relation_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (left, right): (Operand, Operand), relation: NumberRelation) -> Option<S::Cond> {
    // A pair match rather than `Option::zip`, whose instance stays out of line across crates.
    let (Some((left_int, left_test)), Some((right_int, right_test))) =
        (fast_int(stage, frame, left), fast_int(stage, frame, right))
    else {
        return relation_float(stage, frame, (left, right), relation);
    };
    let Some(tests) = both_tests(stage, [left_test, right_test]) else {
        return Some(stage.relation(relation, Domain::Int32, left_int, right_int));
    };
    let word = stage.choose_leaving(tests, #[inline(always)] |stage| {
        let holds = stage.relation(relation, Domain::Int32, left_int, right_int);
        Some(stage.cond_word(holds))
    }, #[inline(always)] |stage| {
        let holds = relation_float(stage, frame, (left, right), relation)?;
        Some(stage.cond_word(holds))
    })?;
    Some(stage.ne_imm(word, 0u64))
}

/// `relation` of two Number operands as binary64 (`float_word`); `None` where an operand is no Number.
#[inline(always)]
fn relation_float<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (left, right): (Operand, Operand), relation: NumberRelation) -> Option<S::Cond> {
    let (left, right) = operands(stage, frame, left, right, Domain::Float64)?;
    Some(stage.relation(relation, Domain::Float64, left, right))
}

/// Arithmetic of two operands in `domain`; the result in `domain` or widened to binary64.
#[inline(always)]
fn arithmetic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand,
    operation: NumberArithmetic, domain: Domain, overflow: (Overflow, NegativeZero)) -> Step<S::Word, F::Control> {
    match domain {
        Domain::Float64 => match number_arithmetic(stage, frame, destination, (left, right), operation) {
            Some(result) => Step::Value(result),
            None => Step::Unresolved,
        },
        Domain::Int32 => {
            let Some((left, right)) = operands(stage, frame, left, right, domain) else { return Step::Unresolved };
            let miss = frame.miss();
            int_arithmetic_result(stage, frame, destination, left, right, operation, overflow, miss)
        }
    }
}

/// `left operation right` of two Number operands, defining `destination` and returning its Number word.
/// A frame that keeps Numbers tagged (`tagged_number_fact`) takes the int32 fast path: where both operands
/// are int32 at run time and the operation is integral, the result is boxed directly as an int32 when it
/// neither overflows nor is `-0` (`int_fast_result`), and every other pair is the binary64 result boxed
/// (`box_number`). The tags are tested first, so a Number that is no int32 computes no int32 result. The
/// register is written once, with the tagged fact, so no arm defines it in another representation. Any
/// other frame defines the binary64 result with `number_fact`. `None` where an operand is no Number, which
/// leaves the row.
#[inline(always)]
fn number_arithmetic<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, (left, right): (Operand, Operand), operation: NumberArithmetic) -> Option<S::Word> {
    let int_operands = match frame.tagged_number_fact() {
        Some(fact) if operation.integral() => match (fast_int(stage, frame, left), fast_int(stage, frame, right)) {
            (Some(left), Some(right)) => Some((fact, left, right)),
            _ => None,
        },
        _ => None,
    };
    let Some((fact, (left_int, left_test), (right_int, right_test))) = int_operands else {
        let result = float_operation(stage, frame, (left, right), operation)?;
        let fact = frame.number_fact();
        frame.write(stage, destination, result, fact);
        return Some(result);
    };
    let packed = match both_tests(stage, [left_test, right_test]) {
        Some(tests) => stage.choose(tests,
            #[inline(always)] |stage| int_fast_result(stage, operation, left_int, right_int),
            #[inline(always)] |stage| stage.constant(NOT_INT32)),
        None => int_fast_result(stage, operation, left_int, right_int),
    };
    let taken = stage.ne_imm(packed, NOT_INT32);
    let boxed_int = frame.int_result(stage, packed);
    let boxed = stage.choose_leaving(taken, |_| Some(boxed_int), #[inline(always)] |stage| {
        let number = float_operation(stage, frame, (left, right), operation)?;
        Some(stage.box_number(number))
    })?;
    frame.write(stage, destination, boxed, fact);
    Some(boxed)
}

/// The word no int32 fast path yields. An int32 word is its low 32 bits, so a word above them names no
/// int32, and the row takes the binary64 path.
const NOT_INT32: u64 = 1 << u32::BITS;

/// `left operation right` of two int32 words, as the int32 result where it neither overflows nor is `-0`
/// (`negative_zero`); `NOT_INT32` otherwise. Only integral operations (add, subtract, multiply) reach it.
#[inline(always)]
fn int_fast_result<S: NumericStage>(stage: &mut S, operation: NumberArithmetic, left: S::Word, right: S::Word) -> S::Word {
    let (result, overflowed) = stage.int_arithmetic(operation, left, right);
    let mut fast = stage.not(overflowed);
    if let Some(zero) = negative_zero(stage, operation, left, right, result) {
        let not_zero = stage.not(zero);
        fast = stage.both(fast, not_zero);
    }
    let sentinel = stage.constant(NOT_INT32);
    stage.select(fast, result, sentinel)
}

/// `left operation right` of two Number operands as binary64 (`float_word`); `None` where an operand is no Number.
#[inline(always)]
fn float_operation<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    (left, right): (Operand, Operand), operation: NumberArithmetic) -> Option<S::Word> {
    let (left, right) = operands(stage, frame, left, right, Domain::Float64)?;
    Some(stage.float_arithmetic(operation, left, right))
}

/// The conjunction of the int32 tests `fast_int` returned for the register operands, or `None` where
/// both operands are constants that need none.
#[inline(always)]
fn both_tests<S: NumericStage>(stage: &mut S, tests: [Option<S::Cond>; 2]) -> Option<S::Cond> {
    match tests {
        [Some(a), Some(b)] => Some(stage.both(a, b)),
        [Some(a), None] | [None, Some(a)] => Some(a),
        [None, None] => None,
    }
}

/// `left % right` in `domain`. An int32 remainder is an int32 except at a zero divisor (NaN)
/// and at a zero with a negative dividend (`-0`, `i32::MIN % -1` included): those leave. A
/// Float64 remainder takes `remainder_number`'s int32 fast path where both operands are int32.
#[inline(always)]
fn remainder<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, left: Operand, right: Operand, domain: Domain,
    (zero, divisor_nonzero): (NegativeZero, bool)) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let Some((left, right)) = operands(stage, frame, left, right, domain) else { return Step::Unresolved };
    let (result, fact) = match domain {
        Domain::Float64 => {
            // A pair the integral remainder does not answer (a fraction, a zero divisor, `-0`) takes
            // the binary64 remainder, which is `%` for every pair of Numbers.
            let (result, inexact) = stage.integral_remainder(left, right);
            let result = stage.choose(inexact, #[inline(always)] |stage| stage.float_remainder(left, right), #[inline(always)] |_| result);
            (result, frame.number_fact())
        }
        Domain::Int32 => {
            if !divisor_nonzero {
                let zero_divisor = stage.i32_is_zero(right);
                if stage.guard_false(zero_divisor, miss).is_none() {
                    return Step::Unresolved;
                }
            }
            let result = stage.int_remainder(left, right);
            if zero == NegativeZero::Exit {
                let is_zero = stage.i32_is_zero(result);
                let negative = stage.i32_is_negative(left);
                let negative_zero = stage.both(is_zero, negative);
                if stage.guard_false(negative_zero, miss).is_none() {
                    return Step::Unresolved;
                }
            }
            (frame.int_result(stage, result), frame.int_fact())
        }
    };
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `left % right` of two Number operands, as a binary64 word. Where both are int32 at run time,
/// the divisor is nonzero and the dividend is non-negative, the remainder is one `idiv` and a
/// conversion of its int32 result: a non-negative dividend's remainder is never `-0` and needs
/// no fmod. Every other pair takes the binary64 remainder. `None` where an operand is no Number,
/// which leaves the row.
#[inline(always)]
fn remainder_number<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand) -> Option<S::Word> {
    // A pair match rather than `Option::zip`, whose instance stays out of line across crates.
    let (Some((left_int, left_test)), Some((right_int, right_test))) =
        (fast_int(stage, frame, left), fast_int(stage, frame, right))
    else {
        return remainder_float(stage, frame, left, right);
    };
    let divisor_nonzero = stage.i32_is_zero(right_int);
    let divisor_nonzero = stage.not(divisor_nonzero);
    let dividend_negative = stage.i32_is_negative(left_int);
    let mut fast = stage.not(dividend_negative);
    fast = stage.both(fast, divisor_nonzero);
    for test in [left_test, right_test].into_iter().flatten() {
        fast = stage.both(fast, test);
    }
    stage.choose_leaving(fast, #[inline(always)] |stage| {
        let remainder = stage.int_remainder(left_int, right_int);
        Some(stage.i32_to_f64(remainder))
    }, #[inline(always)] |stage| remainder_float(stage, frame, left, right))
}

/// `left % right` of two binary64 operands (`float_word`), the general remainder.
#[inline(always)]
fn remainder_float<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    left: Operand, right: Operand) -> Option<S::Word> {
    let (left, right) = operands(stage, frame, left, right, Domain::Float64)?;
    Some(stage.float_remainder(left, right))
}

/// `operand`'s int32 word for the int32 fast paths, with the test that makes it an int32 at run
/// time. A program constant that is an int32 is its own word with no test: its decode proved it,
/// so its tag is never read at run time. A register is its low word, tested by its tag unless the
/// frame's type fact already proves it an int32 (then the test is folded away: `None`). A frame that
/// keeps its Numbers unboxed (`tagged_number_fact` is `None`) has no tag to test, and boxing a binary64
/// register to test it would cost more than the binary64 path, so such a register takes that path.
/// `None` where the operand is neither, or a constant that is no int32, which keeps the binary64 path.
#[inline(always)]
fn fast_int<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operand: Operand) -> Option<(S::Word, Option<S::Cond>)> {
    match frame.constant_operand(operand) {
        Some(value) => Some((stage.i32_constant(value.as_int()?), None)),
        None => {
            let proven = operand.register_index()
                .and_then(|register| frame.register_type(register))
                .is_some_and(|ty| ty.is_int32());
            if !proven && frame.tagged_number_fact().is_none() {
                return None;
            }
            let bits = frame.operand_word(stage, operand)?;
            let test = (!proven).then(|| super::value::is_int(stage, bits));
            Some((stage.narrow(bits), test))
        }
    }
}

/// ToNumeric of a Number `source`: the Number itself, copied with its facts. Any other
/// value leaves (the interpreter converts a BigInt or a primitive by its general path).
#[inline(always)]
fn to_numeric<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    match frame.prove_number(stage, source, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => general_update(stage, frame, super::CallOut::ToNumeric { source }, destination),
    }
}

/// `source ± 1` (`++`/`--`) of an int32 `source`: the int32 sum or difference with one.
#[inline(always)]
fn increment<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register, decrement: bool,
    overflow: Overflow) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let operation = if decrement { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
    let update = super::CallOut::IncDec { source, decrement };
    let Some(value) = frame.int_operand(stage, Operand::register(source), miss) else {
        // A stage that found the operand no int32 (the interpreter's, or a tier that knows it a
        // binary64) steps the binary64 by one; any other operand is the VM's update.
        let Some(number) = frame.number_operand(stage, Operand::register(source), miss) else {
            return general_update(stage, frame, update, destination);
        };
        let one = stage.f64_constant(1.0);
        let result = stage.float_arithmetic(operation, number, one);
        let fact = frame.number_fact();
        frame.write(stage, destination, result, fact);
        return Step::Value(result);
    };
    let one = stage.i32_constant(1);
    match int_arithmetic_result(stage, frame, destination, value, one, operation, (overflow, NegativeZero::Exit), miss) {
        // An int32 overflow is the VM's update, which steps it as a double.
        Step::Unresolved => general_update(stage, frame, update, destination),
        step => step,
    }
}

/// The VM's general operation `call` (ToNumeric or an update) defining `destination`, where the frame takes
/// the general operator in place (`NumericFrame::general_operators`); otherwise the row stays unresolved.
#[inline(always)]
fn general_update<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, call: super::CallOut,
    destination: Register) -> Step<S::Word, F::Control> {
    if frame.general_operators() { super::callout::calling(stage, frame, call, Some(destination)) } else { Step::Unresolved }
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
    if overflow == Overflow::Widen || negative_zero.is_some() {
        return widened_or_negative_zero_result(stage, frame, destination, [left_value, right_value], operation,
            (result, overflow, overflowed, negative_zero), miss);
    }
    let result = frame.int_result(stage, result);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// Writes an int32 result that widens to a binary64 or is guarded by a negative zero; the in-range int32 result stays in `int_arithmetic_result`.
#[cfg_attr(not(stencil_build), cold, inline(never))]
#[cfg_attr(stencil_build, inline(always))]
fn widened_or_negative_zero_result<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, [left_value, right_value]: [S::Word; 2], operation: NumberArithmetic,
    (result, overflow, overflowed, negative_zero): (S::Word, Overflow, S::Cond, Option<S::Cond>),
    miss: S::Miss) -> Step<S::Word, F::Control> {
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
        let number = stage.choose(widen, #[inline(always)] |stage| {
            let left = stage.i32_to_f64(left_value);
            let right = stage.i32_to_f64(right_value);
            stage.float_arithmetic(operation, left, right)
        }, #[inline(always)] |stage| stage.i32_to_f64(result));
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


#[path = "numeric/stage.rs"]
mod stage;
pub(crate) use stage::NumericStage;
#[path = "numeric/concrete.rs"]
mod concrete;

#[cfg(test)]
mod tests;
