//! The integer proofs of a lane unit, computed once before lowering: the int32 arithmetic whose
//! exact result fits (no overflow exit) and the hoisted dense element accesses whose index is
//! non-negative and below their array's length (no bounds guard).
//!
//! A thin view over the facts analysis: one run of it models the unit (`facts::Model`: its
//! inlined frames, its dense accesses, the PCs a previous body left at) and takes the entry's
//! guarded ranges (`Lowering::entering`), so the ranges, the length relations and the proofs
//! are rows of the same fixpoint that types every register, which widens a range that keeps
//! growing instead of assuming one and checking it on the back edge.

use super::*;
use super::super::facts::{self, Model, Seeds, Text};

/// The facts one analysis proves, by unit PC.
pub(super) struct IntRanges {
    overflow_free: CompoundBitSet,
    in_bounds: CompoundBitSet,
}

impl IntRanges {
    pub(super) fn new(instructions: usize) -> Self {
        Self { overflow_free: CompoundBitSet::with_capacity(instructions), in_bounds: CompoundBitSet::with_capacity(instructions) }
    }

    /// Whether the int32 addition, subtraction, product or step at `pc` has an exact int32
    /// result, whatever int32 operands reach it.
    pub(super) fn overflow_free(&self, pc: usize) -> bool { self.overflow_free.contains(pc) }

    /// Whether the index of the element access at `pc` lies in `0..length` of its array.
    pub(super) fn in_bounds(&self, pc: usize) -> bool { self.in_bounds.contains(pc) }
}

impl Lowering<'_, '_> {
    /// The proofs of the unit entered at `entry` with the checked inputs `initial`.
    pub(super) fn prove_ranges(&self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        leaders: &CompoundBitSet, own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) -> IntRanges {
        let dense = |pc: usize| self.dense_loop_access(pc);
        let model = Model { program: self.program, unit: self.unit, generic: self.generic, dense: &dense };
        let seeds = Seeds { contracts: self.facts.contracts(), observed: self.observed, entering: &self.entering, model: Some(model) };
        let text = Text { entry, instructions, leaders, constants: &self.program.constants, written: &writes, own_shape,
            native_calls: self.native_calls };
        let mut proved = IntRanges::new(instructions.len());
        facts::analyze_text(&text, initial, seeds).replay(&text, seeds, |pc, point| {
            if point.overflow_free(&self.program.constants, instructions[pc]) { proved.overflow_free.insert(pc); }
            if point.in_bounds(&model, pc, instructions[pc]) { proved.in_bounds.insert(pc); }
        });
        proved
    }
}
