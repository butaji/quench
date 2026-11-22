//! The numeric instructions' rows: a Number operation or a numeric jump decoded over the frame's Number registers,
//! and the staged operators the decode of a numeric instruction names.

use super::*;

/// The row of a numeric instruction: `None` when the instruction is not one, `Some(None)` when it
/// is one a row cannot take.
pub(crate) fn numeric_row(numbers: Numbers, instruction: WideInstruction) -> Option<Option<Decoded>> {
    use quench_runtime::staged::{NumberOperation, Operation};
    let copy = || Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None);
    let plain = instruction.result_flags() == 0;
    // The moves and jumps by the staged operation the instruction denotes (`Operation::decode`).
    match Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY) {
        Some(Operation::Move { .. }) => return Some(plain.then(copy)),
        // A Number operand is its own numeric value: the loop's entry guards and its own rows prove
        // it, and a function's row proves it itself (`checked`).
        Some(Operation::Number(NumberOperation::ToNumeric { .. })) => return Some(plain.then(|| Decoded { checked: true, ..copy() })),
        Some(Operation::Jump) => return Some(Some(Decoded::number(Kind::Jump, None, None, None, Some(instruction.jump_target())))),
        Some(Operation::JumpIf { register, taken }) => return Some(Some(truthiness_jump(instruction, register, taken))),
        // A comparison jump (`JumpBinaryFalse`): a Number relation, or `==`/`===` (`!=`/`!==`)
        // of Numbers, which the loop's entry guards and its rows prove its operands are.
        Some(Operation::CompareJump { comparison, left, right, negate: true }) => return Some(relation_jump(numbers, instruction, comparison, (left, right)).ok()),
        // `!x`: the boolean of its truthiness.
        Some(Operation::LogicalNot { destination, source }) =>
            return Some(plain.then(|| Decoded::number(Kind::Not, Some(destination), Some(Source::Register(source)), None, None))),
        // `++`/`--`: the int32 update by one (the row's overflow leaves, as the staged one does).
        // `IntCaseIndex`: the case slot of the operand among the labels from `minimum` (its immediate) to `maximum`, which
        // the row takes as `Decoded::hole` (the stencil's hole 3).
        // A register operand only: the row reads its operand from a frame slot.
        Some(Operation::Number(NumberOperation::IntCase { destination, source, minimum, maximum, count: _ })) => return Some(source.register_index().filter(|_| plain).map(|source| Decoded {
            hole: u64::from(maximum as u32),
            ..Decoded::number(Kind::IntCase, Some(destination), Some(Source::Register(source)), Some(Source::Constant(Value::integer(minimum).to_bits())), None)
        })),
        Some(Operation::Number(NumberOperation::IncDec { destination, source, decrement, .. })) => return Some(plain.then(|| Decoded::number(
            if decrement { Kind::Sub } else { Kind::Add }, Some(destination), Some(Source::Register(source)),
            Some(Source::Constant(Value::integer(1).to_bits())), None))),
        _ => {}
    }
    // `-x` and `+x`: the Number unary the interpreter's operations name (`NumberUnary`).
    if let Some(operation) = quench_runtime::vm::operations::NumberUnary::from_js_instruction(instruction.op(), instruction.unary_operator()) {
        return Some(unary_row(instruction, operation).filter(|_| plain));
    }
    if let Some((destination, index)) = Operation::<(), ()>::decode_constant_load(instruction) {
        return Some(load_const(numbers, destination, index).ok().filter(|_| plain));
    }
    Some(match instruction.op().class() {
        Class::Binary => binary(numbers, instruction).ok().map(|row| Decoded { generic: true, ..row }).filter(|_| plain),
        Class::NumericAdd | Class::NumericMultiply => binary(numbers, instruction).ok().filter(|_| plain),
        _ => return None,
    })
}

/// A jump on `register`'s truthiness: taken where it is truthy (`taken`, `JumpTrue`) or falsy.
fn truthiness_jump(instruction: WideInstruction, register: quench_runtime::bytecode::Register, taken: bool) -> Decoded {
    let kind = if taken { Kind::JumpTrue } else { Kind::JumpFalse };
    Decoded::number(kind, None, Some(Source::Register(register)), None, Some(instruction.jump_target()))
}

/// The row of a Number unary over a register: `-` the negation of a Number, `+` a Number as it
/// is (proved, as `ToNumeric` is).
fn unary_row(instruction: WideInstruction, operation: quench_runtime::vm::operations::NumberUnary) -> Option<Decoded> {
    use quench_runtime::vm::operations::NumberUnary;
    let (result, operand) = (instruction.result_register(), Some(Source::Register(instruction.register_b())));
    let row = |kind| Decoded::number(kind, Some(result), operand, None, None);
    Some(match operation {
        NumberUnary::Negate => row(Kind::Neg),
        NumberUnary::Plus => Decoded { checked: true, ..row(Kind::Move) },
    })
}

/// The staged row (`staged_rows.rs`) of a `Binary` that no numeric row takes: `%`, and a relation or an equality as its
/// value. `kind_of` names none of them for a value form, so they reach the lowering (`steps::fast_arm`).
pub(crate) fn staged_binary(operator: u32) -> Option<u8> {
    [
        (BinaryOperator::Remainder, numeric::row_names::STAGED_REMAINDER),
        (BinaryOperator::LessThan, numeric::row_names::STAGED_RELVAL_LT),
        (BinaryOperator::LessEqualThan, numeric::row_names::STAGED_RELVAL_LE),
        (BinaryOperator::GreaterThan, numeric::row_names::STAGED_RELVAL_GT),
        (BinaryOperator::GreaterEqualThan, numeric::row_names::STAGED_RELVAL_GE),
        (BinaryOperator::Equality, numeric::row_names::STAGED_LOOSE_EQ),
        (BinaryOperator::Inequality, numeric::row_names::STAGED_LOOSE_NE),
        (BinaryOperator::StrictEquality, numeric::row_names::STAGED_STRICT_EQ),
        (BinaryOperator::StrictInequality, numeric::row_names::STAGED_STRICT_NE),
    ].into_iter().find(|(candidate, _)| *candidate as u32 == operator).map(|(_, code)| code)
}

/// The staged row of a `Unary` that no unary row takes: `~` and `void`.
pub(crate) fn staged_unary(operator: u32) -> Option<u8> {
    [
        (UnaryOperator::BitwiseNot, numeric::row_names::STAGED_BITWISE_NOT),
        (UnaryOperator::Void, numeric::row_names::STAGED_VOID),
    ].into_iter().find(|(candidate, _)| *candidate as u32 == operator).map(|(_, code)| code)
}
