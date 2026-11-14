//! Loop inputs an entry observed to be int32s: invariants of an OSR loop that its arithmetic reads.

use super::*;
use super::super::operations::NumberArithmetic;

impl Lowering<'_, '_> {
    /// Takes each register the entry observed to hold an int32 (`int_entries`), which the loop
    /// entered at the region's header reads as an operand of integer arithmetic and never writes,
    /// for an int32 contract of its entering edge: proved once there, the loop's index arithmetic
    /// then runs on integers instead of converting a Number per use. Whether it took any.
    pub(super) fn seed_loop_ints(&self, initial: &mut [Type], loops: &[(usize, usize)]) -> bool {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return false };
        let body = &self.unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|instruction| writes(*instruction)).collect();
        let mut seeded = false;
        for instruction in body.iter().filter(|instruction| is_integral_arithmetic(**instruction)) {
            for register in [instruction.operand_b(), instruction.operand_c()].into_iter().filter_map(Operand::register_index) {
                let observed = self.int_entries.iter().any(|(entry, _)| *entry == register);
                let slot = &mut initial[usize::from(register)];
                if observed && register < self.unit.caller_registers && !written.contains(&register)
                    && matches!(*slot, Type::ANY | Type::NUMBER) && self.is_live(header, register) {
                    *slot = Type::INT32;
                    seeded = true;
                }
            }
        }
        seeded
    }
}

/// Whether `instruction` is an addition, subtraction or multiplication of Numbers.
fn is_integral_arithmetic(instruction: WideInstruction) -> bool {
    instruction.op().js_denotation() == JsDenotation::NumericBinary && !instruction.returns_from_frame()
        && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())
            .is_some_and(NumberArithmetic::integral)
}
