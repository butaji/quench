//! Loop inputs an entry observed to be int32s: the invariants of an OSR loop that its index
//! arithmetic reads, which the entry proves once.
//!
//! An index is the sum, difference or product of such inputs. Proved int32 at the entry, they
//! make the arithmetic integer words (`index_math`); proved within `INDEX_INPUT_BITS` too, a
//! product of two and a sum with a third lie within what a Number holds exactly, which is
//! all the arithmetic leaves for (`IntRange::EXACT`), so it checks nothing per iteration.

use super::*;
use super::super::operations::NumberArithmetic;
use super::super::tier_policy::INDEX_INPUT_BITS;

impl Lowering<'_, '_> {
    /// The registers the loop entered at the region's header reads as operands of index
    /// arithmetic (an add, subtraction or product only an index reads), never writes, and the
    /// entry observed to hold an int32.
    fn index_inputs(&self, loops: &[(usize, usize)]) -> Vec<Register> {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return Vec::new() };
        let body = &self.unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|instruction| writes(*instruction)).collect();
        let mut inputs: Vec<Register> = (header..).zip(body).filter(|(pc, instruction)| is_integral_arithmetic(**instruction)
            && self.demand_from(pc + 1, usize::from(instruction.result_register())).index_only())
            .flat_map(|(_, instruction)| [instruction.operand_b(), instruction.operand_c()])
            .filter_map(Operand::register_index)
            .filter(|register| *register < self.unit.caller_registers && !written.contains(register)
                && self.int_entries.iter().any(|(entry, _)| entry == register) && self.is_live(header, *register)).collect();
        inputs.sort_unstable();
        inputs.dedup();
        inputs
    }

    /// Takes each input of the loop's index arithmetic (`index_inputs`) for an int32 contract of
    /// its entering edge: proved once there, the loop's index arithmetic runs on integers instead
    /// of converting a Number per use. Whether it took any.
    pub(super) fn seed_loop_ints(&self, initial: &mut [Type], loops: &[(usize, usize)]) -> bool {
        let mut seeded = false;
        for register in self.index_inputs(loops) {
            let slot = &mut initial[usize::from(register)];
            if matches!(*slot, Type::ANY | Type::NUMBER) {
                *slot = Type::INT32;
                seeded = true;
            }
        }
        seeded
    }

    /// The observed int32 entries live at `entry` (a dead register's value is stale, and nothing
    /// conforms it) whose range is narrower than int32 and whose entry fact is int32: what the
    /// entry guards once (`guard_entering_ranges`) and the ranges pass assumes. An input of the
    /// loop's index arithmetic is within `INDEX_INPUT_BITS` as well, where no earlier body left
    /// at the entry (`generic`): an entry that left for it recompiles without the assumption.
    pub(super) fn entering_ranges(&self, entry: usize, loops: &[(usize, usize)]) -> Vec<(Register, IntRange)> {
        let types = self.facts.at(entry);
        let inputs = if self.generic.contains(&entry) { Vec::new() } else { self.index_inputs(loops) };
        let span = IntRange { min: -(1 << INDEX_INPUT_BITS), max: 1 << INDEX_INPUT_BITS };
        self.int_entries.iter().copied().map(|(register, range)| {
            let bounded = inputs.contains(&register).then(|| range.intersect(span)).flatten();
            (register, bounded.unwrap_or(range))
        }).filter(|&(register, range)| range != IntRange::FULL && self.is_live(entry, register)
            && types.and_then(|types| types.get(usize::from(register))).is_some_and(|ty| ty.is_subtype(Type::INT32))).collect()
    }

    /// Leaves at `pc` where an entering register is outside its range: the guard the ranges pass's
    /// entry state relies on, of each bound tighter than int32's.
    pub(super) fn guard_entering_ranges(&mut self, state: &mut State, pc: usize) {
        for (register, range) in self.entering.clone() {
            let exit = self.guard_exit(state, pc as u32);
            let value = self.int(state, register, exit);
            if range.min > IntRange::FULL.min {
                let above = self.raw_compare_imm(Relation::SignedGreaterEqual, value, range.min);
                emit_js_guard(&mut self.builder, above, exit);
            }
            if range.max < IntRange::FULL.max {
                let below = self.raw_compare_imm(Relation::SignedLessEqual, value, range.max);
                emit_js_guard(&mut self.builder, below, exit);
            }
        }
    }
}

/// Whether `instruction` is an addition, subtraction or multiplication of Numbers.
fn is_integral_arithmetic(instruction: WideInstruction) -> bool {
    instruction.op().js_denotation() == JsDenotation::NumericBinary && !instruction.returns_from_frame()
        && instruction.result_flags() == 0
        && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())
            .is_some_and(NumberArithmetic::integral)
}
