//! Global-name reads in a native OSR region: a `LoadName` becomes an entry capture of its
//! binding, resolved once at the entry (`lane_loop_entry`). A native run crosses no
//! `StoreName`, so the value the entry materializes holds for the whole run.
use super::*;

/// The capture immediate depth that marks a rewritten global-name read.
pub(crate) const GLOBAL_DEPTH: u16 = u16::MAX;
/// The global names one region may read.
pub(super) const MAX_REGION_GLOBALS: usize = 8;

/// Rewrites each `LoadName` to a `LoadCapture` at `GLOBAL_DEPTH` whose slot is the global's
/// index, and each `LoadThis` to a `LoadCapture` of `THIS_CAPTURE`.
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

impl Admission<'_> {
    /// Admits a `LoadCapture`: an environment slot at depth 0, or a rewritten global read at
    /// `GLOBAL_DEPTH`, whose value the lowering unboxes as a Number.
    pub(super) fn admit_capture(&mut self, instruction: WideInstruction, tagged: &std::collections::BTreeSet<Register>) -> Option<()> {
        instruction.result_flags().eq(&0).then_some(())?;
        let depth = instruction.capture_depth();
        (depth == 0 || depth == GLOBAL_DEPTH).then_some(())?;
        let slot = instruction.capture_slot();
        if depth == GLOBAL_DEPTH {
            (usize::from(slot) < MAX_REGION_GLOBALS).then_some(())?;
            let (atom, site) = *self.globals.get(usize::from(slot))?;
            self.result(instruction)?;
            if !self.capture_globals.iter().any(|known| known.is_some_and(|(index, _, _)| index == slot)) {
                self.captures.push(slot);
                self.capture_globals.push(Some((slot, atom, site)));
            }
        } else {
            if tagged.contains(&instruction.result_register()) {
                self.tagged(instruction.result_register())?;
            } else {
                self.result(instruction)?;
            }
            if !self.captures.iter().zip(&self.capture_globals)
                .any(|(capture, global)| *capture == slot && global.is_none())
            {
                self.captures.push(slot);
                self.capture_globals.push(None);
            }
        }
        Some(())
    }
}

/// Gives each global capture a slot past the environment's, which the entry's dense capture
/// view holds its value in.
pub(super) fn assign_global_captures(region: &mut LoopRegion) {
    if region.capture_globals.iter().all(Option::is_none) {
        return;
    }
    let mut next = region.captures.iter().filter(|slot| **slot < CALLEE_CAPTURE).max().map_or(0, |slot| slot + 1);
    let mut assigned: Vec<(u16, u16)> = Vec::new();
    for index in 0..region.capture_globals.len() {
        let Some((read, _, _)) = region.capture_globals[index] else { continue };
        region.captures[index] = next;
        assigned.push((read, next));
        next += 1;
    }
    for instruction in &mut region.instructions {
        if instruction.op().class() != Class::LoadCapture || instruction.capture_depth() != GLOBAL_DEPTH {
            continue;
        }
        let read = instruction.capture_slot();
        if let Some(&(_, slot)) = assigned.iter().find(|(known, _)| *known == read) {
            *instruction = WideInstruction::new(Op::LoadCapture, instruction.a(), 0, instruction.c(),
                ImmediateLayout::capture_immediate(usize::from(GLOBAL_DEPTH), slot));
        }
    }
}
