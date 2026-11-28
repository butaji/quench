//! Index arithmetic in 64-bit words: the sum, difference or product of integers an element index
//! is made of, whose result leaves int32 within `IntRange::WORD`.
//!
//! A binary64 index must round trip through an exact int32 before it addresses an element; the
//! integer word is the Number's own value (`Representation::Int64`), as an element index is below
//! its array's length (`IntRange::EXACT`) and a word and its Number agree on every integer of that
//! span. The word keeps that agreement by leaving where a result passes it, so the register an
//! exit hands the interpreter holds exactly the Number it would have computed.
//!
//! Only results every remaining use of which reads an index (`Demand::index_only`) take the word:
//! a Number that is read as one costs a conversion per read, and the exit it adds is no loss where
//! the index it computes would address no element.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::operations::NumberArithmetic;
use quench_runtime::staged::number_machine;

/// The span of integers an index word holds of an arithmetic result of `range`: the Numbers it agrees with.
pub(crate) fn held(range: IntRange) -> IntRange { range.intersect(IntRange::EXACT).unwrap_or(IntRange::EXACT) }

impl Lowering<'_, '_> {
    /// The range of the arithmetic `instruction` at `pc` where it runs in 64-bit words: its proved
    /// result leaves int32 within `IntRange::WORD`, its operands are int32 or already words, and
    /// only an index reads it.
    pub(crate) fn index_math(&self, state: &State, pc: usize, instruction: WideInstruction) -> Option<IntRange> {
        let range = self.ranges.index_range(pc)?;
        let integral = |operand: Operand| match operand.kind() {
            Some(OperandKind::Register) => matches!(state.values[usize::from(operand.payload())].value.representation,
                Representation::Int32 | Representation::Int64),
            Some(OperandKind::Constant) => true,
            _ => false,
        };
        let read = self.demand_from(pc + 1, usize::from(instruction.result_register()));
        (!self.left_here && !instruction.returns_from_frame() && integral(instruction.operand_b())
            && integral(instruction.operand_c()) && read.index_only()).then_some(range)
    }

    /// `operation` of the integer `operands` in 64-bit words, into the result register: where
    /// its `range` passes what a Number holds exactly, it leaves at `exit` before the register is
    /// defined.
    pub(crate) fn index_arithmetic(&mut self, state: &mut State, (destination, operands): (Register, [Operand; 2]),
        (operation, range): (NumberArithmetic, IntRange), exit: ir::Block) -> Option<()> {
        let result = self.index_word(state, operands, (operation, range), exit)?;
        self.define(state, destination, result, RegisterFact::definition(Type::int(held(range)), Representation::Int64));
        Some(())
    }

    /// The word `operation` of the integer `operands` computes: where its `range` passes what a
    /// Number holds exactly, it leaves at `exit`.
    pub(crate) fn index_word(&mut self, state: &mut State, [left, right]: [Operand; 2],
        (operation, range): (NumberArithmetic, IntRange), exit: ir::Block) -> Option<ir::Value> {
        let left = self.operand_long(state, left, exit)?;
        let right = self.operand_long(state, right, exit)?;
        let result = number_machine::wrapping_int32_arithmetic(&mut emit(&mut self.builder), operation, left, right);
        if !IntRange::EXACT.contains(range) { self.guard_exact(result, exit); }
        Some(result)
    }

    /// An element index's word: a register's held 64-bit word, else its int32 (`operand_int`),
    /// which the element rows zero-extend.
    pub(crate) fn operand_index(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        let long = operand.register_index()
            .and_then(|register| frame::held_long(&mut self.builder, &mut self.registers, state, register));
        long.or_else(|| self.operand_int(state, operand, otherwise))
    }

    /// The int32 index of the access at `pc` read from `register`, where its loop's entry proved the index integral
    /// wherever it is not NaN (`IntRanges::integral_index`): the truncation of the register's binary64, with no test.
    /// `None` where the access is not one, or the register is no untagged binary64 (its guarded read applies).
    pub(crate) fn integral_index(&mut self, state: &State, pc: usize, register: Register) -> Option<ir::Value> {
        if !self.ranges.integral_index(pc) { return None; }
        frame::integral_index_value(&mut self.builder, &mut self.registers, state, register)
    }

    /// The index word of the access at `pc` (`operand_index`), taking its integral form where `integral_index` gives one.
    pub(crate) fn access_index(&mut self, state: &mut State, pc: usize, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        let integral = operand.register_index().and_then(|register| self.integral_index(state, pc, register));
        integral.or_else(|| self.operand_index(state, operand, otherwise))
    }

    /// An operand's integer value as a signed 64-bit word (`frame::operand_long`).
    fn operand_long(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        frame::operand_long(&mut self.builder, &mut self.registers, state, &self.program.constants, operand, otherwise)
    }

    /// Continues where the word `result` lies within `IntRange::EXACT`, else at `exit`.
    fn guard_exact(&mut self, result: ir::Value, exit: ir::Block) {
        let within = self.within_exact(result);
        emit_js_guard(&mut self.builder, within, exit);
    }

    /// Whether the word lies within `IntRange::EXACT` (`number_machine::within_word`).
    pub(crate) fn within_exact(&mut self, word: ir::Value) -> ir::Value {
        number_machine::within_word(&mut emit(&mut self.builder), word, IntRange::EXACT.min, IntRange::EXACT.max)
    }
}
