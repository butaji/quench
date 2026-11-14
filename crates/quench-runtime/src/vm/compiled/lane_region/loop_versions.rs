//! Bounds by loop versioning: an element access in a counted loop whose index is below a bound
//! the loop never writes (`i < n`) is within its array when the bound is, so the loop's entry
//! compares the bound with the array's length once (`n <= length`) and the accesses of its body
//! lower with their index proved (`IntRanges::in_bounds`), where each tested it per iteration.
//!
//! The comparison is the entry's, so a failed one is the entry's own exact-PC leave: the
//! interpreter runs the loop, which tests every access. A loop that can change an array's length
//! (any instruction `Model::disturbs_lengths`) keeps its per-access tests, and one that a previous
//! body left at its entry (`generic`) compiles again without versioning: a bound that exceeds its
//! array on most entries repays no guard.

use super::*;
use super::super::facts::Model;
use crate::staged::Stage;

impl Lowering<'_, '_> {
    /// The accesses of `candidates` (PC, the register holding the array, the bound) whose loop
    /// can be versioned, by PC with the bound: the array is a register's (hoisted on the loop's
    /// entering edge), no instruction of the loop writes the bound or may change a length, and
    /// the bound is an int32 there.
    pub(super) fn versioned_bounds(&self, model: &Model<'_>, instructions: &[WideInstruction],
        candidates: &[(usize, Register, Register)]) -> Vec<(usize, Register)> {
        candidates.iter().filter(|&&(pc, array, bound)| self.version_holds(model, instructions, pc, (array, bound)))
            .map(|&(pc, _, bound)| (pc, bound)).collect()
    }

    /// Whether the access at `pc` of the array held in `base` below `bound` can be versioned.
    fn version_holds(&self, model: &Model<'_>, instructions: &[WideInstruction], pc: usize, (base, bound): (Register, Register)) -> bool {
        let Some(array) = self.loop_array(pc, Some(base)).filter(|array| matches!(array.source, ArraySource::Register(_))) else { return false };
        let span = array.header..=array.back;
        !self.generic.contains(&array.header)
            && !instructions[span.clone()].iter().any(|instruction| writes(*instruction).contains(&bound))
            && span.into_iter().all(|at| !model.disturbs_lengths(at, instructions[at]))
            && self.facts.at(array.header).and_then(|types| types.get(usize::from(bound))).is_some_and(|ty| ty.is_subtype(Type::INT32))
    }

    /// The comparison of each bound of the versioned accesses of the loop at `header` with the
    /// length of the array they index, on its entering edge: leaves at `exit` where a bound passes
    /// its array's length (a bound below zero leaves no iteration to test).
    pub(super) fn version_bounds(&mut self, state: &mut State, header: usize, exit: ir::Block) {
        let mut compared: Vec<(ArraySource, Register)> = Vec::new();
        for (pc, bound) in self.ranges.versioned().to_vec() {
            let instruction = self.unit.instructions[pc];
            let base = if instruction.op().is_get_index() { instruction.operand_b().register_index() } else { Some(instruction.register_b()) };
            let Some(array) = self.loop_array(pc, base).filter(|array| array.header == header) else { continue };
            if compared.contains(&(array.source, bound)) { continue; }
            compared.push((array.source, bound));
            let (_, length) = self.hoisted_view(array);
            let bound = self.int(state, bound, exit);
            let (bound, length) = (self.raw_unary(Unary::SignExtend(Lane::I64), bound), emit(&mut self.builder).uextend32(length));
            let within = self.raw_compare(Relation::SignedLessEqual, bound, length);
            emit_js_guard(&mut self.builder, within, exit);
        }
    }
}
