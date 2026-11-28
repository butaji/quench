//! Loop-invariant code motion of the integer operations an index is made of: a sum, difference or
//! product of integers whose operands no instruction of a loop writes and whose result only an
//! index reads (`Lowering::hoistable`) is the same word on each iteration, so the entry of the
//! outermost such loop evaluates it once, with the exits its operation takes (a result passing
//! `IntRange::EXACT` leaves at the loop's header, before the loop ran), and its uses in the loop read
//! the held word (`Lowering::hoisted_read`). The operation has no effect to reorder: it reads its
//! operands, which are not written, and writes the register it is held for.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
use super::plan::{arithmetic_of, Nest};
use super::{Bound, Decision, Hoist};
use super::super::super::facts::Point;
use super::super::super::operations::NumberArithmetic;

impl Lowering<'_, '_> {
    /// The loops of `loops` the region lowers whole.
    pub(crate) fn lowered_loops(&self, loops: &[(usize, usize)]) -> Vec<(usize, usize)> {
        loops.iter().copied().filter(|&(header, back)| self.coverage.lowered(header) && self.coverage.lowered(back)).collect()
    }

    /// Whether the arithmetic at `pc` computes an index word from integers: its operands are integer
    /// registers or int32 constants, and every remaining use of its result reads an index.
    pub(crate) fn hoistable(&self, pc: usize, instruction: WideInstruction, point: &Point) -> bool {
        let integer = |operand: Operand| operand.register_index()
            .map_or_else(|| self.int32_constant(operand).is_some(), |register| point.int_range(register).is_some());
        arithmetic_of(instruction).is_some_and(|operation| operation != NumberArithmetic::Divide) && !self.generic.contains(&pc)
            && integer(instruction.operand_b()) && integer(instruction.operand_c())
            && self.demand_from(pc + 1, usize::from(instruction.result_register())).index_only()
    }

    /// The loop whose entry evaluates each of the `candidates`: the outermost one around it that
    /// writes none of its operands and that no previous body left at its entry.
    pub(crate) fn plan_hoists(&self, nest: Nest<'_>, candidates: &[usize]) -> Vec<Hoist> {
        candidates.iter().filter_map(|&pc| {
            let instruction = nest.instructions[pc];
            let registers: Vec<Register> = [instruction.operand_b(), instruction.operand_c()].into_iter().filter_map(Operand::register_index).collect();
            nest.around(pc).into_iter().find(|&span| !self.generic.contains(&span.0) && nest.unwritten(span, &registers))
                .map(|(header, _)| Hoist { pc, header })
        }).collect()
    }

    /// The loop whose entry decides each relation of `candidates` (`Decision`): the outermost one
    /// around it whose body writes neither source.
    pub(crate) fn plan_decisions(&self, nest: Nest<'_>, candidates: &[usize]) -> Vec<Decision> {
        candidates.iter().filter_map(|&pc| nest.around(pc).into_iter().filter(|&span| !self.generic.contains(&span.0))
            .find_map(|span| {
                let instruction = nest.instructions[pc];
                let sources = [instruction.operand_b(), instruction.operand_c()].map(|operand| operand.register_index()
                    .and_then(|register| self.loop_source(nest, span, pc, register)));
                let [Some(left), Some(right)] = sources else { return None };
                Some(Decision { pc, header: span.0, sources: [left, right] })
            })).collect()
    }

    /// The registers `candidates` order against an int32 counter, each with the innermost loop around
    /// its PC whose body writes it nowhere (`Bound`), once per loop and register.
    pub(crate) fn plan_bounds(&self, nest: Nest<'_>, candidates: &[(usize, Register)]) -> Vec<Bound> {
        let mut bounds: Vec<Bound> = candidates.iter().filter_map(|&(pc, register)| nest.around(pc).into_iter().rev()
            .filter(|&span| !self.generic.contains(&span.0))
            .find(|&span| nest.unwritten(span, &[register]))
            .map(|(header, _)| Bound { header, register })).collect();
        bounds.sort_unstable_by_key(|bound| (bound.header, bound.register));
        bounds.dedup_by_key(|bound| (bound.header, bound.register));
        bounds
    }

    /// The register `register` holds at `pc` the same value of on each iteration of the loop `span`:
    /// itself where the loop does not write it, else the register the one write of it, a copy in
    /// `pc`'s own block before it, copies where the loop writes that one nowhere.
    fn loop_source(&self, nest: Nest<'_>, span: (usize, usize), pc: usize, register: Register) -> Option<Register> {
        if nest.unwritten(span, &[register]) { return Some(register); }
        let writes = |at: &usize| nest.instructions[*at].written_registers().any(|written| written == register);
        if (span.0..=span.1).filter(writes).count() != 1 { return None; }
        // Back from the jump to its block's leader: the copy is there or nowhere.
        let block = (span.0..pc).rev().take_while(|at| self.blocks.get(at + 1).is_none_or(Option::is_none));
        let copy = nest.instructions[block.into_iter().find(writes)?];
        let Some(quench_runtime::staged::Operation::<(), ()>::Move { source, .. }) = quench_runtime::staged::Operation::decode(copy, quench_runtime::staged::DecodeContext::EMPTY) else { return None };
        nest.unwritten(span, &[source]).then_some(source)
    }

    /// The operations whose loop is the one at `header`, evaluated on its entering edge from `state`.
    pub(crate) fn hoist_pure(&mut self, state: &mut State, header: usize) {
        let pcs: Vec<usize> = self.ranges.hoisted().iter().filter(|hoist| hoist.header == header).map(|hoist| hoist.pc).collect();
        if pcs.is_empty() { return; }
        let exit = self.guard_exit_for(state, header as u32, ExitFact::Element);
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

    /// The relations the VM decides (`Comparison::Decided`) whose loop is the one at `header`,
    /// decided once on its entering edge from `state` by the pure `Decide` helper: their operands
    /// are values the loop does not write (a string the loop compares with a constant, say), so
    /// each iteration's decision is this one. A pair only the full step decides leaves at the
    /// header, before the loop ran, as each iteration would have left at the test. The jump
    /// reads the held word (`Lowering::decided_jump`).
    pub(crate) fn hoist_decisions(&mut self, state: &mut State, header: usize) {
        let decisions: Vec<Decision> = self.ranges.decisions().iter().filter(|decision| decision.header == header).copied().collect();
        if decisions.is_empty() { return; }
        let exit = self.guard_exit_for(state, header as u32, ExitFact::Element);
        for Decision { pc, sources, .. } in decisions {
            let instruction = self.unit.instructions[pc];
            let [left, right] = sources.map(|register| self.tagged(state, register));
            let operator = self.raw_const(Lane::I64, i64::from(instruction.binary_operator_field()));
            let word = self.helper_call(LaneHelper::Decide, &[left, right, operator]);
            let decided = self.raw_compare_imm(Relation::NotEqual, word, Value::DELETED.to_bits() as i64);
            emit_js_guard(&mut self.builder, decided, exit);
            let value = self.builder.declare_var(types::I64);
            self.builder.def_var(value, word);
            self.hoisted.insert(pc, LoopRead { value, ty: Type::language(quench_runtime::ValueType::Boolean),
                representation: Representation::Tagged, receiver: None, header: None, step: None });
        }
    }

    /// The registers the loop at `header` orders against its int32 counters (`plan_bounds`), each
    /// proved int32 on its entering edge from `state`: a register that is no int32 leaves at the
    /// header, before the loop ran (the interpreter runs it, and a later compile of it keeps its
    /// relations on the Number path, since the header is then generic). The loop's relations then
    /// read the int32 view, which no instruction of the loop writes.
    pub(crate) fn hoist_bounds(&mut self, state: &mut State, header: usize) {
        let registers: Vec<Register> = self.ranges.bounds().iter().filter(|bound| bound.header == header).map(|bound| bound.register).collect();
        if registers.is_empty() { return; }
        let exit = self.guard_exit_for(state, header as u32, ExitFact::Element);
        for register in registers {
            self.int(state, register, exit);
        }
    }
}
