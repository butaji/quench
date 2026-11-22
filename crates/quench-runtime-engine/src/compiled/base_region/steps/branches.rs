//! The branches of a helper step's run: the jump to a PC and the landing at a target that restores
//! the registers its destination expects.

use super::*;

/// The jump to `pc`, a part of a run whose NEXT is not where the run goes on.
pub(crate) fn jump_to(pc: u32) -> Result<Planned, String> {
    Ok(Planned::single(control(Kind::Jump)?, [0; HOLES], Some(pc), None))
}

/// The reloads of the pinned registers live at `pc`, then the jump to `pc`: the branch of a run
/// that restores the registers its destination expects.
fn landing(layout: &Layout, pc: u32) -> Result<Vec<Planned>, String> {
    let reloads = layout.spills(pc).into_iter().map(|(place, register)| call_plan::reload_part(place, register).map(Planned::of_part));
    reloads.chain(std::iter::once(jump_to(pc))).collect()
}

/// A helper step's run made to branch: its fall-through (the reloads of the next instruction's
/// registers) jumps to the next instruction, and the TAKEN exit of the helper row (a step that took
/// its jump, `LANE_CALL_BRANCHED`) lands at `target` after the fall-through, as a part's NEXT is the
/// part laid out after it.
pub(crate) fn branching(mut row: Planned, layout: &Layout, (pc, target): (u32, u32)) -> Result<Planned, String> {
    row.post.push(jump_to(pc + 1)?);
    let taken = row.pre.len() + 1 + row.post.len();
    row.post.extend(landing(layout, target)?);
    row.branch = Some(Branch::Part(taken));
    Ok(row)
}
