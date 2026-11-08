//! The edges into a lane body's block leaders, from its bytecode: which edges each block's
//! entry proofs come from, and which blocks every edge into reaches before they are lowered.

use super::*;

/// Per block leader, its forward edges (`forward_edges`), and the leaders lowering may seal
/// on arrival: those only forward edges enter, but a loop body its rotated header test
/// enters from the latch (`rotated_bodies`).
pub(super) fn leader_edges(instructions: &[WideInstruction], leaders: &CompoundBitSet, starts: &[usize])
    -> (Vec<Option<usize>>, CompoundBitSet) {
    let forward = forward_edges(instructions, leaders, starts);
    let rotated = rotated_bodies(instructions);
    let mut sealable = CompoundBitSet::with_capacity(instructions.len());
    for pc in (0..instructions.len()).filter(|pc| forward[*pc].is_some() && !rotated.contains(*pc)) {
        sealable.insert(pc);
    }
    (forward, sealable)
}

/// Per block leader, the edges into it, all from before it; `None` for one the entry
/// (`start`) or a later PC enters, whose proofs no earlier edge can give.
fn forward_edges(instructions: &[WideInstruction], leaders: &CompoundBitSet, starts: &[usize]) -> Vec<Option<usize>> {
    let mut edges: Vec<Option<usize>> = (0..instructions.len()).map(|pc| leaders.contains(pc).then_some(0)).collect();
    for start in starts { if let Some(slot) = edges.get_mut(*start) { *slot = None; } }
    for (pc, instruction) in instructions.iter().enumerate() {
        for target in successors(pc, *instruction).into_iter().flatten() {
            let Some(slot) = edges.get_mut(target) else { continue };
            *slot = if target > pc { slot.map(|count| count + 1) } else { None };
        }
    }
    edges
}

/// The first body PC of each loop whose header test a backedge repeats (`Lowering::
/// instruction` rotates it): the repeated test enters it from the latch, an edge
/// `forward_edges` does not see.
fn rotated_bodies(instructions: &[WideInstruction]) -> CompoundBitSet {
    let mut bodies = CompoundBitSet::with_capacity(instructions.len() + 1);
    for (pc, instruction) in instructions.iter().enumerate() {
        let [_, target] = successors(pc, *instruction);
        if let Some(header) = target.filter(|target| *target <= pc)
            && instructions.get(header).is_some_and(|test| test.op().is_js_conditional_jump()) {
            bodies.insert(header + 1);
        }
    }
    bodies
}
