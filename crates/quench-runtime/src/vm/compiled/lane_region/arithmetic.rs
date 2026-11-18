//! The lane's Number arithmetic (`+`, `-`, `*`, `/` of Numbers): the representation the
//! operands' proved facts choose for it. Operands proved int32 stay int32 as C keeps them, an
//! integer result an index is made of is computed in 64-bit words (`index_math`), any other
//! operand takes binary64, and an operand only a tag types is taken for an int32 and tested.

use super::*;
use super::super::operations::NumberArithmetic;
use crate::staged::number_machine;
use crate::staged::{Domain, NegativeZero, NumberOperation, Operation, Overflow};

/// What a Number arithmetic step reads and writes: its result register and its operands.
type Operands = (Register, [Operand; 2]);

impl Lowering<'_, '_> {
    /// The arithmetic `instruction` at `pc` runs, into its result register; the exit it leaves at
    /// is `exit`. `None` where an operand has no proof the lowering can make.
    pub(super) fn arithmetic(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        operation: NumberArithmetic, exit: ir::Block) -> Option<()> {
        let operands = (instruction.result_register(), [instruction.operand_b(), instruction.operand_c()]);
        let words = if operation == NumberArithmetic::Divide { None } else { self.index_math(state, pc, instruction) };
        if let Some(range) = words {
            self.index_arithmetic(state, operands, (operation, range), exit)?;
        } else if operation == NumberArithmetic::Divide || self.float_operand(state, operands.1) {
            self.arithmetic_step(state, (operands, operation), (Domain::Float64, Overflow::Exit, NegativeZero::Exit), exit);
        } else if self.proved_int(state, operands.1) {
            self.int32_arithmetic(state, pc, operands, (operation, exit));
        } else if operation.int32_projection_is_wrapping() && self.truncated(pc, operands.0) {
            self.truncated_sum(state, pc, operands, (operation, exit))?;
        } else {
            self.checked_arithmetic(state, pc, operands, (operation, exit))?;
        }
        Some(())
    }

    /// Whether an operand is held as a binary64 or a wide integer, or is a constant that is no
    /// int32: the operation then runs in binary64, or is a loop's own (`left_here`).
    fn float_operand(&self, state: &State, operands: [Operand; 2]) -> bool {
        operands.iter().any(|operand| match operand.kind() {
            Some(OperandKind::Register) => matches!(state.values[usize::from(operand.payload())].value.representation,
                Representation::Float64 | Representation::Int64),
            Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate).is_some_and(|value| !super::super::facts::is_int32(value)),
            _ => false,
        }) || self.left_here
    }

    /// Operands proved int32: the int32 result, which leaves where it overflows. A result the
    /// unit's ranges keep inside int32, or a sum or difference only `| 0` reads (its ToInt32), is
    /// the wrapping int32 result: no overflow exit.
    fn int32_arithmetic(&mut self, state: &mut State, pc: usize, operands: Operands,
        (operation, exit): (NumberArithmetic, ir::Block)) {
        let wrapping = self.ranges.overflow_free(pc)
            || (operation.int32_projection_is_wrapping() && self.truncated(pc, operands.0))
            // A product the unit's ranges do not bound is still exact modulo 2^32 when the sum
            // it feeds is truncated: `(a * 3 + i) | 0` never observes the product untruncated.
            || (operation == NumberArithmetic::Multiply && self.truncated_through_int32_sum(state, pc, operands.0));
        let overflow = if wrapping { Overflow::Wrap } else { Overflow::Exit };
        let zero = if self.ranges.ignores_negative_zero(pc) { NegativeZero::Ignore } else { NegativeZero::Exit };
        self.arithmetic_step(state, (operands, operation), (Domain::Int32, overflow, zero), exit);
    }

    /// The staged Number arithmetic of `operation` over `operands`, in a domain with its overflow
    /// and negative-zero policies.
    fn arithmetic_step(&mut self, state: &mut State, ((destination, [left, right]), operation): (Operands, NumberArithmetic),
        (domain, overflow, zero): (Domain, Overflow, NegativeZero), exit: ir::Block) {
        let operation = Operation::Number(NumberOperation::Arithmetic { destination, left, right, operation, domain, overflow, zero });
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
    }

    /// A sum or difference only its ToInt32 is read of: int32 operands wrap, any other numbers
    /// take ToInt32 of their double result, and both join as an int32.
    fn truncated_sum(&mut self, state: &mut State, pc: usize, (destination, [left, right]): Operands,
        (operation, exit): (NumberArithmetic, ir::Block)) -> Option<()> {
        let float = self.builder.create_block();
        self.builder.set_cold_block(float);
        let mut float_state = state.clone();
        let left_int = self.operand_int(state, left, float).expect("supported operand");
        let right_int = self.operand_int(state, right, float).expect("supported operand");
        let wrapped = number_machine::wrapping_int32_arithmetic(&mut emit(&mut self.builder), operation, left_int, right_int);
        let join = self.builder.create_block();
        let int = self.builder.append_block_param(join, types::I32);
        self.goto(join, &[wrapped]);
        self.builder.switch_to_block(float);
        let left_number = self.operand_number(&mut float_state, left, exit)?;
        let right_number = self.operand_number(&mut float_state, right, exit)?;
        let number = number_machine::float_arithmetic(&mut emit(&mut self.builder), operation, left_number, right_number);
        let truncated = self.number_to_int32(number, exit);
        self.goto(join, &[truncated]);
        self.builder.switch_to_block(join);
        self.join_states(state, &float_state, pc);
        self.set_int(state, destination, int);
        Some(())
    }

    /// Operands a tag only types: the int32 arithmetic where both are int32 and its result is
    /// one (an overflow, or a product's negative zero, takes the binary64 arm), else binary64.
    fn checked_arithmetic(&mut self, state: &mut State, pc: usize, (destination, [left, right]): Operands,
        (operation, exit): (NumberArithmetic, ir::Block)) -> Option<()> {
        let float = self.builder.create_block();
        self.builder.set_cold_block(float);
        let mut float_state = state.clone();
        let left_int = self.operand_int(state, left, float).expect("supported operand");
        let right_int = self.operand_int(state, right, float).expect("supported operand");
        let (result, overflow) = number_machine::checked_int32_arithmetic(&mut emit(&mut self.builder), operation, left_int, right_int);
        let int_done = self.builder.create_block();
        self.branch_if(overflow, (float, &[]), (int_done, &[]));
        self.builder.switch_to_block(int_done);
        if operation == NumberArithmetic::Multiply { self.guard_not_negative_zero(result, [left_int, right_int], float); }
        let join = self.builder.create_block();
        let number = self.builder.append_block_param(join, types::F64);
        let result = self.raw_unary(Unary::FromSigned(Lane::F64), result);
        self.goto(join, &[result]);
        self.builder.switch_to_block(float);
        let left_number = self.operand_number(&mut float_state, left, exit)?;
        let right_number = self.operand_number(&mut float_state, right, exit)?;
        let result = number_machine::float_arithmetic(&mut emit(&mut self.builder), operation, left_number, right_number);
        self.goto(join, &[result]);
        self.builder.switch_to_block(join);
        self.join_states(state, &float_state, pc);
        self.set_number(state, destination, number);
        Some(())
    }

    /// Continues where the int32 `product` of `operands` is no negative zero, else at `float`.
    fn guard_not_negative_zero(&mut self, product: ir::Value, operands: [ir::Value; 2], float: ir::Block) {
        let zero = self.raw_compare_imm(Relation::Equal, product, 0);
        let signs = self.raw_binary(Binary::Or, operands[0], operands[1]);
        let negative = self.raw_compare_imm(Relation::SignedLess, signs, 0);
        let negative_zero = self.raw_binary(Binary::And, zero, negative);
        emit_js_guard_not(&mut self.builder, negative_zero, float);
    }
}
