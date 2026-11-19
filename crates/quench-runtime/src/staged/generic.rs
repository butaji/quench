//! Value steps whose words decide them: `void`, a string's own property key, and equality of
//! two words (`Operation::Void`, `PropertyKey`, `Equality`). A value these words do not decide
//! is the VM's exact step (a conversion that allocates, a string or BigInt comparison, guest
//! code), which the frame makes in place (`Frame::vm_step`) or declines, so every tier shares
//! the one rule and the one boundary.

use super::value::{is_direct_string, is_int};
use super::*;
use crate::bytecode::OperandKind;
use crate::value::ValueLayout;
#[cfg(not(stencil_build))]
use crate::vm::operations::equality_operator;
use crate::Value;

/// Whether `operand` names a register or a program constant: the operands a `Binary` or an
/// `Equality` row reads (`NumericFrame::operand_word`).
pub(super) fn is_value_operand(operand: Operand) -> bool {
    matches!(operand.kind(), Some(OperandKind::Register | OperandKind::Constant))
}

impl<W, P> Operation<W, P> {
    /// The `Binary`, `ToPropertyKey` and `JumpBinaryFalse` operations of `decode`; `None` where a `Binary`
    /// carries a result flag (`decode_completed` strips it first).
    #[inline(always)]
    pub(crate) fn decode_generic(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::ToPropertyKey => Self::PropertyKey {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::Binary => return Self::decode_binary(instruction),
            #[cfg(not(stencil_build))]
            Op::NumericAdd | Op::NumericMultiply => return Self::decode_numeric_arithmetic(instruction),
            Op::JumpBinaryFalse => Self::decode_compare(instruction),
            _ => return None,
        })
    }

    /// `NumericAdd` and `NumericMultiply`: the Number arithmetic of their own operator (`+` or `*`)
    /// over their two operands, which `Binary` decodes to the same operation (`decode_binary`). A record
    /// whose operator field names another operator is none.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn decode_numeric_arithmetic(instruction: WideInstruction) -> Option<Self> {
        let own = match instruction.op() {
            Op::NumericAdd => oxc_ast::ast::BinaryOperator::Addition,
            _ => oxc_ast::ast::BinaryOperator::Multiplication,
        } as u32;
        if instruction.binary_operator() != own { return None; }
        Self::decode_binary(instruction)
    }

    /// `Binary`: an arithmetic, bitwise or relation operator as its Number operation
    /// (`numeric::binary_operation`), equality as its own row, and any other operator the VM's.
    /// Every operand kind decodes; whether a frame asks the VM for a record its words leave is
    /// the frame's (`NumericFrame::general_operators`), not the decode's.
    #[inline(always)]
    fn decode_binary(instruction: WideInstruction) -> Option<Self> {
        if instruction.result_flags() != 0 { return None; }
        let destination = instruction.result_register();
        let operator = instruction.binary_operator();
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if let Some(number) = super::numeric::binary_operation(operator, destination, left, right) {
            return Some(Self::Number(number));
        }
        #[cfg(not(stencil_build))]
        return Some(match equality_operator(operator) {
            Some((strict, negated)) => Self::Equality { destination, left, right, strict, negated },
            None => Self::Binary { destination, left, right, operator },
        });
        // The stencil build has no equality classifier: every other `Binary` is the VM's operator.
        #[cfg(stencil_build)]
        return Some(Self::Binary { destination, left, right, operator });
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
        Operation::Binary { destination, left, right, operator } if frame.general_operators() =>
            super::callout::calling(stage, frame, CallOut::Binary { left, right, operator }, Some(destination)),
        Operation::Unary { destination, source, operator } if frame.general_operators() =>
            super::callout::calling(stage, frame, CallOut::Unary { operator, value: source }, Some(destination)),
        Operation::CompareJump { .. } => super::compare::step(stage, frame, operation),
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
pub(super) fn nullish<S: NumericStage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let null = stage.eq_imm(bits, Value::NULL.to_bits());
    let undefined = stage.eq_imm(bits, Value::UNDEFINED.to_bits());
    stage.either(null, undefined)
}

/// The equality predicate where the two words decide it: `(decided, equal)`. The same word is
/// equal unless it is NaN; two distinct int32 words are unequal; `null` and `undefined` are
/// equal under `==` only.
#[inline(always)]
pub(super) fn decide<S: NumericStage>(stage: &mut S, [left, right]: [S::Word; 2], strict: bool) -> (S::Cond, S::Cond) {
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
    [left, right]: [Operand; 2], (strict, negated): (bool, bool)) -> Step<S::Word, F::Control> {
    let (Some(left_word), Some(right_word)) = (frame.operand_word(stage, left), frame.operand_word(stage, right)) else {
        // An operand the words do not hold (a constant, a field or a local): the VM's equality where the frame takes it.
        if !frame.general_operators() { return Step::Unresolved; }
        return super::callout::calling(stage, frame, CallOut::Equality { left, right, strict, negated }, Some(destination));
    };
    let (decided, equal) = decide(stage, [left_word, right_word], strict);
    let result = if negated { stage.not(equal) } else { equal };
    let (decision, fact) = frame.boolean(stage, result);
    let call = CallOut::Equality { left, right, strict, negated };
    let value = stage.choose_leaving(decided, |_| Some(decision),
        |stage| frame.vm_step(stage, call, Some(destination)).map(|(word, _)| word));
    let Some(value) = value else { return Step::Unresolved };
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}
