//! Register aliases of kept receivers. A kept receiver is held by every register that aliases it: the
//! register its construction defined, and each copy of it made while it is kept. A copy adds an alias
//! (`alias_kept`); a definition removes only the alias it names (`release_alias`, run by
//! `register_define`); the receiver dies when no alias is left. Split from `virtual_objects` along the
//! alias seam.

use super::*;

/// Releases `register` from every kept receiver's aliases: a definition of it names another value. A
/// receiver with no alias left dies.
pub(crate) fn release_alias(virtuals: &mut smallvec::SmallVec<[VirtualObject; 1]>, register: Register) {
    for kept in virtuals.iter_mut() { kept.aliases.retain(|alias| *alias != register); }
    virtuals.retain(|kept| !kept.aliases.is_empty());
}

impl Lowering<'_, '_> {
    /// Makes `destination` an alias of the kept receiver `source` names. The definition releases
    /// `destination` from any other receiver first; `source` keeps its receiver. False, with no effect,
    /// where `source` is no kept receiver's alias.
    pub(crate) fn alias_kept(&mut self, state: &mut State, destination: Register, source: Register) -> bool {
        if !state.virtuals.iter().any(|kept| kept.aliases.contains(&source)) { return false; }
        if destination == source { return true; }
        let placeholder = self.raw_const(Lane::I64, 0);
        self.define(state, destination, placeholder, RegisterFact::definition(Type::ANY, Representation::Tagged));
        if let Some(kept) = state.virtuals.iter_mut().find(|kept| kept.aliases.contains(&source)) { kept.aliases.push(destination); }
        true
    }

    /// A plain `Move` of a kept receiver's alias: its destination becomes another alias, and nothing is
    /// allocated. Any other copy of a kept receiver materializes it (`keeps_virtuals`).
    pub(crate) fn virtual_copy(&mut self, state: &mut State, instruction: WideInstruction) -> bool {
        matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. }))
            && instruction.result_flags() == 0
            && self.alias_kept(state, instruction.result_register(), instruction.register_b())
    }

    /// Drops each alias dead at `pc`, and each receiver left with none.
    pub(crate) fn prune_virtuals(&self, state: &mut State, pc: usize) {
        for kept in state.virtuals.iter_mut() { kept.aliases.retain(|register| self.is_live(pc, *register)); }
        state.virtuals.retain(|kept| !kept.aliases.is_empty());
    }

    /// Whether any alias of `kept` is live at `pc`, which keeps the receiver live there.
    pub(crate) fn live_at(&self, pc: usize, kept: &VirtualObject) -> bool {
        kept.aliases.iter().any(|register| self.is_live(pc, *register))
    }

    /// `kept` with only its aliases live at `pc`; `None` where none is.
    pub(crate) fn live_part(&self, pc: usize, kept: &VirtualObject) -> Option<VirtualObject> {
        let mut part = kept.clone();
        part.aliases.retain(|register| self.is_live(pc, *register));
        (!part.aliases.is_empty()).then_some(part)
    }

    /// Drops each alias the instruction, which reads none of the kept receivers as an object, defines,
    /// and each receiver left with none. A definition that reaches `register_define` releases its alias
    /// there; this covers the instructions whose definitions are made outside it.
    pub(crate) fn drop_overwritten_virtuals(&self, state: &mut State, instruction: WideInstruction) {
        let (_, definitions) = quench_runtime::compile::liveness::transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
        for kept in state.virtuals.iter_mut() { kept.aliases.retain(|register| !definitions.contains(usize::from(*register))); }
        state.virtuals.retain(|kept| !kept.aliases.is_empty());
        state.virtual_arrays.retain(|kept| !definitions.contains(usize::from(kept.register)));
    }

    /// Whether the call marker at `pc` of an inlined frame keeps the kept receivers: `inline_call` passes
    /// its arguments and receiver as aliases and answers the receiver guard from the kept object, so the
    /// marker neither allocates nor calls. A callee that is a kept receiver (its identity test reads the
    /// object) or any kept array still materializes there.
    pub(crate) fn inlined_call_passes(&self, state: &State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc) else { return false };
        matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Nop))
            && instruction.result_flags() == 0
            && state.virtual_arrays.is_empty()
            && !state.virtuals.iter().any(|kept| kept.aliases.contains(&self.unit.frames[frame].callee_register))
    }
}
