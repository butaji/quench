//! Value steps whose words decide them: `void`, a string's own property key, and equality of
//! two words (`Operation::Void`, `PropertyKey`, `Equality`). A value these words do not decide
//! is the VM's exact step (a conversion that allocates, a string or BigInt comparison, guest
//! code), which the frame makes in place (`Frame::vm_step`) or declines, so every tier shares
//! the one rule and the one boundary.

use super::value::{is_direct_string, is_double, is_heap, is_int};
use super::*;
#[cfg(not(stencil_build))]
use crate::bytecode::OperandKind;
use crate::value::ValueLayout;
#[cfg(not(stencil_build))]
use crate::vm::operations::equality_operator;
use crate::Value;

#[cfg(not(stencil_build))]
/// Whether `operand` names a register or a program constant: the operands a `Binary` or an
/// `Equality` row reads (`NumericFrame::operand_word`).
pub(super) fn is_value_operand(operand: Operand) -> bool {
    matches!(operand.kind(), Some(OperandKind::Register | OperandKind::Constant))
}

#[cfg(not(stencil_build))]
impl<W, P> Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The `Binary`, `ToPropertyKey` and `JumpBinaryFalse` operations of `decode`; `None` where a `Binary`
    /// carries a result flag (`decode_completed` strips it first).
    #[inline(always)]
    pub fn decode_generic(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::ToPropertyKey => Self::PropertyKey {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::Binary => return Self::decode_binary(instruction),
            #[cfg(not(stencil_build))]
            Op::NumericAdd | Op::NumericMultiply => return Self::decode_numeric_arithmetic(instruction),
            Op::JumpBinaryFalse => Self::decode_compare(instruction),
            Op::IntCaseIndex => return Self::decode_int_case(instruction),
            _ => return None,
        })
    }

    #[cfg(not(stencil_build))]
    /// `IntCaseIndex`: the case slot of register `c` among the `b` dense int labels from `imm`.
    fn decode_int_case(instruction: WideInstruction) -> Option<Self> {
        let count = instruction.element_count();
        if count == 0 { return None; }
        let minimum = instruction.imm() as i32;
        let maximum = minimum.checked_add(i32::from(count) - 1)?;
        Some(Self::Number(NumberOperation::IntCase {
            destination: instruction.result_register(),
            source: crate::bytecode::Operand::register(instruction.register_c()),
            minimum,
            maximum,
            count,
        }))
    }

    /// `Binary` over two registers: equality as its own row, any other operator the VM's.
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

    #[cfg(not(stencil_build))]
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
pub(super) fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Void { destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::VOID]) } { void(stage, frame, destination) } else { unreachable!() },
        Operation::PropertyKey { destination, source } => if const { super::op_kind::admits(KIND, &[super::op_kind::PROPERTY_KEY]) } { property_key(stage, frame, destination, source) } else { unreachable!() },
        Operation::Equality { destination, left, right, strict, negated } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::EQUALITY]) } { equality(stage, frame, destination, [left, right], (strict, negated)) } else { unreachable!() },
        Operation::Binary { destination, left, right, operator } if frame.general_operators() =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::BINARY]) } { super::callout::calling(stage, frame, CallOut::Binary { left, right, operator }, Some(destination)) } else { unreachable!() },
        // A string chain's operand (`UNARY_ADD_OPERAND_STRING`) that is a primitive other than a heap
        // value, or a string, is the operand itself; an object or symbol takes the VM's conversion.
        #[cfg(not(stencil_build))]
        Operation::Unary { destination, source, operator } if operator == crate::bytecode::UNARY_ADD_OPERAND_STRING
            && const { super::op_kind::admits(KIND, &[super::op_kind::UNARY]) } => {
            let bits = frame.tagged(stage, source);
            let heap = is_heap(stage, bits);
            let string = is_direct_string(stage, bits);
            let primitive = stage.not(heap);
            let itself = stage.either(primitive, string);
            let miss = frame.miss();
            if stage.guard(itself, miss).is_some() {
                return Step::Value(frame.move_register(stage, destination, source));
            }
            if !frame.general_operators() { return Step::Unresolved; }
            super::callout::calling(stage, frame, CallOut::Unary { operator, value: source }, Some(destination))
        }
        Operation::Unary { destination, source, operator } if frame.general_operators() =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::UNARY]) } { super::callout::calling(stage, frame, CallOut::Unary { operator, value: source }, Some(destination)) } else { unreachable!() },
        Operation::CompareJump { .. } => if const { super::op_kind::admits(KIND, &[super::op_kind::COMPARE_JUMP]) } { super::compare::step_of::<KIND, _, _>(stage, frame, operation) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::VOID, super::op_kind::PROPERTY_KEY, super::op_kind::EQUALITY, super::op_kind::COMPARE_JUMP]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// `undefined` into `destination`.
#[inline(always)]
fn void<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register) -> Step<S::Word, F::Control> {
    let undefined = Constant { value: Value::UNDEFINED, ty: Type::UNDEFINED };
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
    let key = stage.choose_leaving(string, |_| Some(bits), #[inline(always)] |stage| {
        let (key, fact) = frame.vm_step(stage, CallOut::PropertyKey { source }, Some(destination))?;
        converted = Some(fact);
        Some(key)
    });
    let Some(key) = key else { return Step::Unresolved };
    let fact = converted.unwrap_or_else(#[inline(always)] || frame.fact(source));
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
/// equal unless it is NaN. For `===`, two words that are not identical are unequal unless the
/// words cannot tell: two heap values (strings and BigInts compare by content), two doubles (zero
/// and negative zero), or an int32 and a double (the same Number). For `==`, two distinct int32
/// words are unequal; `null` and `undefined` are equal under `==` only.
#[inline(always)]
pub fn decide<S: NumericStage>(stage: &mut S, [left, right]: [S::Word; 2], strict: bool) -> (S::Cond, S::Cond) {
    let gap = stage.sub(left, right);
    let identical = stage.eq_imm(gap, 0u64);
    let ordered = stage.ne_imm(left, ValueLayout::CANONICAL_NAN);
    let same = stage.both(identical, ordered);
    let (left_int, right_int) = (is_int(stage, left), is_int(stage, right));
    let ints = stage.both(left_int, right_int);
    if strict {
        // Identical words and two int32s are settled by one compare each. Any other pair is tested
        // by its tags only where it is not settled: two Numbers compare as binary64 (zero and negative
        // zero, an int32 and a double); two records (direct objects or arrays) are apart, not being
        // identical; two other heap values (strings, BigInts) are the VM's; any other pair is apart.
        let settled = stage.either(identical, ints);
        let state = stage.choose(settled, #[inline(always)] |stage| stage.constant(NOT_OPEN), #[inline(always)] |stage| {
            let (left_double, right_double) = (is_double(stage, left), is_double(stage, right));
            let left_number = stage.either(left_int, left_double);
            let right_number = stage.either(right_int, right_double);
            let numbers = stage.both(left_number, right_number);
            stage.choose(numbers, #[inline(always)] |stage| number_state(stage, [left, right]), #[inline(always)] |stage| {
                let (left_heap, right_heap) = (is_heap(stage, left), is_heap(stage, right));
                let heap_pair = stage.both(left_heap, right_heap);
                let records = record_pair(stage, [left, right]);
                let open_records = stage.not(records);
                let open = stage.both(heap_pair, open_records);
                let (open_bit, apart) = (stage.constant(OPEN), stage.constant(NOT_OPEN));
                stage.select(open, open_bit, apart)
            })
        });
        let decided = stage.ne_imm(state, OPEN);
        let numbers_equal = stage.eq_imm(state, NUMBERS_EQUAL);
        return (stage.either(settled, decided), stage.either(same, numbers_equal));
    }
    let by_words = stage.either(identical, ints);
    let (left_nullish, right_nullish) = (nullish(stage, left), nullish(stage, right));
    let nullish_pair = stage.both(left_nullish, right_nullish);
    // One nullish side and one that is no nullish value: unequal under `==` unless the other side
    // is an [[IsHTMLDDA]] object, which is a heap cell that is neither a direct object nor a direct
    // array (`compare::loosely_nullish`). So a primitive word or a recorded object settles it.
    let one_nullish = stage.either(left_nullish, right_nullish);
    let other = stage.select(left_nullish, right, left);
    let other_heap = is_heap(stage, other);
    let (other_object, other_array) = (super::value::is_direct_object(stage, other), super::value::is_direct_array(stage, other));
    let recorded = stage.either(other_object, other_array);
    let other_primitive = stage.not(other_heap);
    let settles = stage.either(other_primitive, recorded);
    let nullish_apart = stage.both(one_nullish, settles);
    let decided = stage.either(by_words, nullish_pair);
    // Two Numbers compare as binary64 under `==` as under `===`; two records are equal only when identical.
    let words_settled = stage.either(decided, one_nullish);
    let state = stage.choose(words_settled, #[inline(always)] |stage| stage.constant(OPEN), #[inline(always)] |stage| {
        let (left_double, right_double) = (is_double(stage, left), is_double(stage, right));
        let left_number = stage.either(left_int, left_double);
        let right_number = stage.either(right_int, right_double);
        let numbers = stage.both(left_number, right_number);
        stage.choose(numbers, #[inline(always)] |stage| number_state(stage, [left, right]), #[inline(always)] |stage| {
            let records = record_pair(stage, [left, right]);
            let (apart, open) = (stage.constant(NOT_OPEN), stage.constant(OPEN));
            stage.select(records, apart, open)
        })
    });
    let by_state = stage.ne_imm(state, OPEN);
    let numbers_equal = stage.eq_imm(state, NUMBERS_EQUAL);
    let decided = stage.either(decided, nullish_apart);
    // `same` is false for a nullish side against a non-nullish word, so `equal` needs no change.
    let equal = stage.either(same, nullish_pair);
    (stage.either(decided, by_state), stage.either(equal, numbers_equal))
}

/// `decide`'s states of a pair its words leave: the VM's (`OPEN`), apart, or two Numbers equal or not.
const OPEN: u64 = 0;
const NOT_OPEN: u64 = 1;
const NUMBERS_EQUAL: u64 = 2;

/// The state of two Number words: `NUMBERS_EQUAL` where their binary64 values are equal, else `NOT_OPEN`.
#[inline(always)]
fn number_state<S: NumericStage>(stage: &mut S, [left, right]: [S::Word; 2]) -> S::Word {
    let (left, right) = (number_bits(stage, left), number_bits(stage, right));
    let equal = stage.float_eq(left, right);
    let (equal_state, apart) = (stage.constant(NUMBERS_EQUAL), stage.constant(NOT_OPEN));
    stage.select(equal, equal_state, apart)
}

/// `===` of a proved binary64 `number` and any word `other` (`decide` where one side is known a
/// Number): equal only where `other` is a Number whose binary64 equals it, so NaN equals nothing.
#[inline(always)]
pub fn number_strict_equal<S: NumericStage>(stage: &mut S, number: S::Word, other: S::Word) -> S::Cond {
    let (int, double) = (is_int(stage, other), is_double(stage, other));
    let is_number = stage.either(int, double);
    let value = number_bits(stage, other);
    let equal = stage.float_eq(number, value);
    stage.both(is_number, equal)
}

/// The binary64 word of Number word `bits`: an int32 widened, a double as it is.
#[inline(always)]
fn number_bits<S: NumericStage>(stage: &mut S, bits: S::Word) -> S::Word {
    let int = is_int(stage, bits);
    stage.choose(int, #[inline(always)] |stage| {
        let payload = stage.narrow(bits);
        stage.i32_to_f64(payload)
    }, #[inline(always)] |stage| stage.f64_from_bits(bits))
}

/// Whether both words are records (direct objects or direct arrays), which are equal only when identical.
#[inline(always)]
fn record_pair<S: NumericStage>(stage: &mut S, [left, right]: [S::Word; 2]) -> S::Cond {
    let (left_object, left_array) = (super::value::is_direct_object(stage, left), super::value::is_direct_array(stage, left));
    let (right_object, right_array) = (super::value::is_direct_object(stage, right), super::value::is_direct_array(stage, right));
    let left_record = stage.either(left_object, left_array);
    let right_record = stage.either(right_object, right_array);
    stage.both(left_record, right_record)
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
        #[inline(always)] |stage| frame.vm_step(stage, call, Some(destination)).map(|(word, _)| word));
    let Some(value) = value else { return Step::Unresolved };
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}
