use super::{exits_before_effects, WideInstruction};

/// Whether native code can run a whole iteration: the back-edge is reachable from the header
/// past no exit but a call's, which the lane resumes after (`LoopRegion::call_boundary_pcs`).
/// A loop every iteration of which leaves runs in its function's lane body instead; the
/// error is the first exit that blocks it.
pub(super) fn iterates_natively(instructions: &[WideInstruction], header: u32) -> Result<(), usize> {
    let last = instructions.len() - 1;
    let (mut seen, mut pending, mut blocked) = (crate::bitset::BitSet::new(instructions.len()), vec![0], usize::MAX);
    while let Some(offset) = pending.pop() {
        let op = instructions[offset].op();
        if !seen.insert(offset) {
            continue;
        }
        if exits_before_effects(op) && !op.is_ordinary_call() {
            blocked = blocked.min(offset);
            continue;
        }
        if offset == last {
            return Ok(());
        }
        crate::bytecode::for_each_instruction_successor(header as usize + offset, instructions[offset], |successor| {
            pending.extend(successor.checked_sub(header as usize).filter(|next| *next <= last));
        });
    }
    Err(blocked.min(last))
}
