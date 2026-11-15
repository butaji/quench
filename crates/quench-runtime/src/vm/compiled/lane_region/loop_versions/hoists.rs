//! Loop-invariant code motion of the integer operations an index is made of: a sum, difference or
//! product of integers whose operands no instruction of a loop writes and whose result only an
//! index reads (`Lowering::hoistable`) is the same word on each iteration, so the entry of the
//! outermost such loop evaluates it once, with the exits its operation takes (a result passing
//! `IntRange::EXACT` leaves at the loop's header, before the loop ran), and its uses in the loop read
//! the held word (`Lowering::hoisted_read`). The operation has no effect to reorder: it reads its
//! operands, which are not written, and writes the register it is held for.

use super::super::*;
use super::plan::{arithmetic_of, Nest};
use super::Hoist;
use super::super::super::facts::Point;
use super::super::super::operations::NumberArithmetic;

impl Lowering<'_, '_> {
    /// The loops of `loops` the region lowers whole.
    pub(in super::super) fn lowered_loops(&self, loops: &[(usize, usize)]) -> Vec<(usize, usize)> {
        loops.iter().copied().filter(|&(header, back)| self.coverage.lowered(header) && self.coverage.lowered(back)).collect()
    }

    /// Whether the arithmetic at `pc` computes an index word from integers: its operands are integer
    /// registers or int32 constants, and every remaining use of its result reads an index.
    pub(in super::super) fn hoistable(&self, pc: usize, instruction: WideInstruction, point: &Point) -> bool {
        let integer = |operand: Operand| operand.register_index()
            .map_or_else(|| self.int32_constant(operand).is_some(), |register| point.int_range(register).is_some());
        arithmetic_of(instruction).is_some_and(|operation| operation != NumberArithmetic::Divide) && !self.generic.contains(&pc)
            && integer(instruction.operand_b()) && integer(instruction.operand_c())
            && self.demand_from(pc + 1, usize::from(instruction.result_register())).index_only()
    }

    /// The loop whose entry evaluates each of the `candidates`: the outermost one around it that
    /// writes none of its operands and that no previous body left at its entry.
    pub(in super::super) fn plan_hoists(&self, nest: Nest<'_>, candidates: &[usize]) -> Vec<Hoist> {
        candidates.iter().filter_map(|&pc| {
            let instruction = nest.instructions[pc];
            let registers: Vec<Register> = [instruction.operand_b(), instruction.operand_c()].into_iter().filter_map(Operand::register_index).collect();
            nest.around(pc).into_iter().find(|&span| !self.generic.contains(&span.0) && nest.unwritten(span, &registers))
                .map(|(header, _)| Hoist { pc, header })
        }).collect()
    }

    /// The operations whose loop is the one at `header`, evaluated on its entering edge from `state`.
    pub(super) fn hoist_pure(&mut self, state: &mut State, header: usize) {
        let pcs: Vec<usize> = self.ranges.hoisted().iter().filter(|hoist| hoist.header == header).map(|hoist| hoist.pc).collect();
        if pcs.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for pc in pcs {
            let instruction = self.unit.instructions[pc];
            let operation = arithmetic_of(instruction).expect("a hoisted operation is Number arithmetic");
            let range = self.ranges.index_range(pc).expect("a hoisted operation has an index range");
            let word = self.index_word(state, [instruction.operand_b(), instruction.operand_c()], (operation, range), exit)
                .expect("the operands of a hoisted operation are registers and constants");
            let value = self.builder.declare_var(types::I64);
            self.builder.def_var(value, word);
            self.hoisted.insert(pc, LoopRead { value, ty: Type::int(super::super::index_math::held(range)),
                representation: Representation::Int64, receiver: None, header: None, step: None });
        }
    }
}
