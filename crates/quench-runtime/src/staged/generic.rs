//! Value steps whose words decide them: `void`, a string's own property key, and equality of
//! two words (`Operation::Void`, `PropertyKey`, `Equality`). A value these words do not decide
//! is the VM's exact step (a conversion that allocates, a string or BigInt comparison, guest
//! code), which the frame makes in place (`Frame::vm_step`) or declines, so every tier shares
//! the one rule and the one boundary.

use super::value::{is_direct_string, is_int};
use super::*;
use crate::value::ValueLayout;
use crate::vm::operations::equality_operator;
use crate::Value;

impl<W, P> Operation<W, P> {
    /// The `Binary` and `ToPropertyKey` operations of `decode`; `None` where an
    /// operand is no register or the instruction returns from the frame.
    pub(super) fn decode_generic(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::ToPropertyKey => Self::PropertyKey {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::Binary => return Self::decode_binary(instruction),
            _ => return None,
        })
    }

    /// `Binary` over two registers: equality as its own row, any other operator the VM's.
    fn decode_binary(instruction: WideInstruction) -> Option<Self> {
        let left = instruction.operand_b().register_index()?;
        let right = instruction.operand_c().register_index()?;
        if instruction.result_flags() != 0 { return None; }
        let destination = instruction.result_register();
        let operator = instruction.binary_operator();
        Some(match equality_operator(operator) {
            Some((strict, negated)) => Self::Equality { destination, left, right, strict, negated },
            None => Self::Binary { destination, left, right, operator },
        })
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
        Operation::Binary { destination, left, right, operator } =>
            super::callout::calling(stage, frame, CallOut::Binary { left, right, operator }, Some(destination)),
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

/// A direct string `source` is its own key; any other value is the VM's conversion.
#[inline(always)]
fn property_key<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let bits = frame.tagged(stage, source);
    let string = is_direct_string(stage, bits);
    let mut converted = None;
    let key = stage.choose_leaving(string, |_| Some(bits), |stage| {
        let (key, fact) = frame.vm_step(stage, CallOut::PropertyKey { source }, Some(destination))?;
        converted = Some(fact);
        Some(key)
    });
    let Some(key) = key else { return Step::Unresolved };
    let fact = converted.unwrap_or_else(|| frame.fact(source));
    frame.write(stage, destination, key, fact);
    Step::Value(key)
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

/// The canonical boolean of the pair's equality, negated for `!=` and `!==`; the VM's where the
/// words do not decide it.
#[inline(always)]
fn equality<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, destination: Register,
    [left, right]: [Register; 2], (strict, negated): (bool, bool)) -> Step<S::Word, F::Control> {
    let words = [frame.tagged(stage, left), frame.tagged(stage, right)];
    let (decided, equal) = decide(stage, words, strict);
    let result = if negated { stage.not(equal) } else { equal };
    let (decision, fact) = frame.boolean(stage, result);
    let call = CallOut::Equality { left, right, strict, negated };
    let value = stage.choose_leaving(decided, |_| Some(decision),
        |stage| frame.vm_step(stage, call, Some(destination)).map(|(word, _)| word));
    let Some(value) = value else { return Step::Unresolved };
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}
