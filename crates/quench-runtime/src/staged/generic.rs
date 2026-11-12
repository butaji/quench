//! Value steps whose words decide them: `void`, a string's own property key, and equality of
//! two words (`Operation::Void`, `PropertyKey`, `Equality`). A value these words do not decide
//! leaves at the frame's miss edge for the VM's exact step (a conversion that allocates, a
//! string or BigInt comparison, guest code), so every tier shares the one rule and the one
//! boundary.

use super::value::{is_direct_string, is_int};
use super::*;
use crate::value::ValueLayout;
use crate::vm::operations::equality_operator;
use crate::Value;

impl<W, P> Operation<W, P> {
    /// The `Binary` equality and `ToPropertyKey` operations of `decode`; `None` where an
    /// operand is no register or the instruction returns from the frame.
    pub(super) fn decode_generic(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::ToPropertyKey => Self::PropertyKey {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::Binary => return Self::decode_equality(instruction),
            _ => return None,
        })
    }

    /// `Binary` of an equality operator over two registers.
    fn decode_equality(instruction: WideInstruction) -> Option<Self> {
        let (strict, negated) = equality_operator(instruction.binary_operator())?;
        let left = instruction.operand_b().register_index()?;
        let right = instruction.operand_c().register_index()?;
        (instruction.result_flags() == 0).then_some(Self::Equality {
            destination: instruction.result_register(), left, right, strict, negated })
    }
}

/// Run the value step `operation`; the other operations are `Unresolved`.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Void { destination } => void(stage, frame, destination),
        Operation::PropertyKey { destination, source } => property_key(stage, frame, destination, source),
        Operation::Equality { destination, left, right, strict, negated } =>
            equality(stage, frame, destination, [left, right], (strict, negated)),
        _ => Step::Unresolved,
    }
}

/// `undefined` into `destination`.
#[inline(always)]
fn void<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let undefined = Constant { value: Value::UNDEFINED, ty: Type::immediate(Value::UNDEFINED) };
    let (value, fact) = frame.constant(stage, undefined);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// A direct string `source` is its own key, copied with its facts.
#[inline(always)]
fn property_key<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let bits = frame.tagged(stage, source);
    let string = is_direct_string(stage, bits);
    match stage.guard(string, miss) {
        Some(()) => Step::Value(frame.move_register(stage, destination, source)),
        None => Step::Unresolved,
    }
}

/// Whether `bits` is `null` or `undefined`.
#[inline(always)]
fn nullish<S: NumericStage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let null = stage.eq_imm(bits, Value::NULL.to_bits());
    let undefined = stage.eq_imm(bits, Value::UNDEFINED.to_bits());
    stage.either(null, undefined)
}

/// The equality predicate where the two words decide it: `(decided, equal)`. The same word is
/// equal unless it is NaN; two distinct int32 words are unequal; `null` and `undefined` are
/// equal under `==` only.
#[inline(always)]
fn decide<S: NumericStage>(stage: &mut S, [left, right]: [S::Word; 2], strict: bool) -> (S::Cond, S::Cond) {
    let gap = stage.sub(left, right);
    let identical = stage.eq_imm(gap, 0u64);
    let ordered = stage.ne_imm(left, ValueLayout::CANONICAL_NAN);
    let same = stage.both(identical, ordered);
    let (left_int, right_int) = (is_int(stage, left), is_int(stage, right));
    let ints = stage.both(left_int, right_int);
    let by_words = stage.either(identical, ints);
    if strict { return (by_words, same); }
    let (left_nullish, right_nullish) = (nullish(stage, left), nullish(stage, right));
    let nullish_pair = stage.both(left_nullish, right_nullish);
    (stage.either(by_words, nullish_pair), stage.either(same, nullish_pair))
}

/// The canonical boolean of the pair's equality, negated for `!=` and `!==`.
#[inline(always)]
fn equality<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    [left, right]: [Register; 2], (strict, negated): (bool, bool)) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let words = [frame.tagged(stage, left), frame.tagged(stage, right)];
    let (decided, equal) = decide(stage, words, strict);
    if stage.guard(decided, miss).is_none() { return Step::Unresolved; }
    let result = if negated { stage.not(equal) } else { equal };
    let (value, fact) = frame.boolean(stage, result);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}
