//! Reads a native OSR region takes from its entry record: a global name and `this`. A
//! `LoadName` becomes an entry capture of its binding: a native run never crosses a
//! `StoreName` (which exits before its effects) or a call boundary, and an inlined callee's
//! body admits no `StoreName`, so the value the entry materializes holds for the whole run.
//! `this` likewise cannot change during a run. A function with dynamic scope access keeps its
//! name loads.
use super::*;

/// The capture immediate depth that marks a rewritten global-name read. A real lexical depth
/// never reaches it, and a genuine capture at any other nonzero depth stays unadmitted.
pub(super) const GLOBAL_DEPTH: u16 = u16::MAX;
/// The global names one region may read, in entry-record order.
pub(super) const MAX_REGION_GLOBALS: usize = 8;

/// Rewrites each `LoadName` of `instructions` to a `LoadCapture` at `GLOBAL_DEPTH` reading the
/// binding materialized in the entry record, and each `LoadThis` to a `LoadCapture` of
/// `THIS_CAPTURE`. Returns the atoms by slot; a function with dynamic scope access keeps its
/// name loads and no atoms.
pub(super) fn rewrite_entry_reads(
    function: &Function,
    instructions: &[WideInstruction],
) -> (Vec<WideInstruction>, Vec<(crate::bytecode::Atom, u16)>) {
    if function.has_dynamic_scope_access() {
        return (instructions.to_vec(), Vec::new());
    }
    let mut globals = Vec::new();
    let rewritten = instructions.iter().map(|instruction| {
        if instruction.op().class() == Class::LoadThis && instruction.result_flags() == 0 {
            return WideInstruction::new(Op::LoadCapture, instruction.a(), 0, 0,
                ImmediateLayout::capture_immediate(0, THIS_CAPTURE));
        }
        if instruction.op().class() != Class::LoadName {
            return *instruction;
        }
        let site = (instruction.atom_index(), instruction.cache_site_index());
        let index = globals.iter().position(|global| *global == site).unwrap_or_else(|| {
            globals.push(site);
            globals.len() - 1
        });
        WideInstruction::new(Op::LoadCapture, instruction.a(), 0, site.1,
            ImmediateLayout::capture_immediate(usize::from(GLOBAL_DEPTH), u16::try_from(index).unwrap_or(u16::MAX)))
    }).collect();
    (rewritten, globals)
}

/// The entry-capture index a `LoadCapture` reads: an environment slot at depth 0, or a
/// rewritten global-name read at `GLOBAL_DEPTH`.
pub(super) fn capture_index(region: &LoopRegion, instruction: WideInstruction) -> Option<usize> {
    instruction.op().is_load_capture().then_some(())?;
    let depth = instruction.capture_depth();
    (depth == 0 || depth == GLOBAL_DEPTH).then_some(())?;
    let slot = instruction.capture_slot();
    (0..region.captures.len()).find(|index| {
        region.captures[*index] == slot
            && region.capture_globals[*index].is_some() == (depth == GLOBAL_DEPTH)
    })
}

impl Admission<'_> {
    /// Admits a `LoadCapture`: an environment slot at depth 0, or a rewritten `LoadName` at
    /// `GLOBAL_DEPTH` whose atom the entry record materializes.
    pub(super) fn admit_capture(&mut self, instruction: WideInstruction, tagged: &std::collections::BTreeSet<Register>) -> Option<()> {
        instruction.result_flags().eq(&0).then_some(())?;
        let depth = instruction.capture_depth();
        (depth == 0 || depth == GLOBAL_DEPTH).then_some(())?;
        if tagged.contains(&instruction.result_register()) {
            self.tagged(instruction.result_register())?;
        } else {
            self.result(instruction)?;
        }
        let slot = instruction.capture_slot();
        if depth == GLOBAL_DEPTH {
            (usize::from(slot) < MAX_REGION_GLOBALS).then_some(())?;
            let site = *self.globals.get(usize::from(slot))?;
            if !self.capture_globals.iter().any(|known| *known == Some(site)) {
                self.captures.push(slot);
                self.capture_globals.push(Some(site));
            }
        } else if !self.captures.iter().zip(&self.capture_globals)
            .any(|(capture, global)| *capture == slot && global.is_none())
        {
            self.captures.push(slot);
            self.capture_globals.push(None);
        }
        Some(())
    }
}
