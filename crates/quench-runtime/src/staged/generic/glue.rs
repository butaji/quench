//! Runtime glue of `generic`: the decode of the `Binary`, `ToPropertyKey`, `IntCaseIndex` and
//! numeric arithmetic records of `Operation::decode`.
#![allow(unused_imports)]
use super::*;
use crate::staged::OperationCompareExt;

/// The generic records of `decode`: `Binary`, `ToPropertyKey`, `IntCaseIndex` and numeric arithmetic.
pub trait OperationGenericExt<W, P>: Sized {
    fn decode_generic(instruction: WideInstruction) -> Option<Self>;

    /// Public through the trait (`pub trait`): a lane row calls `Operation::decode_int_case` directly.
    fn decode_int_case(instruction: WideInstruction) -> Option<Self>;

    fn decode_numeric_arithmetic(instruction: WideInstruction) -> Option<Self>;

    fn decode_binary(instruction: WideInstruction) -> Option<Self>;
}

impl<W, P> OperationGenericExt<W, P> for Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The `Binary`, `ToPropertyKey` and `JumpBinaryFalse` operations of `decode`; `None` where a `Binary`
    /// carries a result flag (`decode_completed` strips it first).
    #[inline(always)]
    fn decode_generic(instruction: WideInstruction) -> Option<Self> {
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
    /// `IntCaseIndex`: the case slot of register `c` among the `b` dense int labels from `imm`. A pure read of the
    /// record's fields, so a lane row calls it directly rather than dispatching on the opcode (`decode_generic`).
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
            Op::NumericAdd => crate::operator::BinaryOperator::Addition,
            _ => crate::operator::BinaryOperator::Multiplication,
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
        if let Some(number) = crate::staged::numeric::binary_operation(operator, destination, left, right) {
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

