use super::{exits_before_effects, MethodCalls, WideInstruction};

/// Whether native code can run a whole iteration: the back-edge is reachable from the header
/// past no exit but a call's, which the lane resumes after (`LoopRegion::call_boundary_pcs`),
/// or the inliner may still run inlined. A region whose method calls the inliner declined
/// (`MethodCalls::Exit`) leaves at each, so a call blocks it as any exit does. A loop every
/// iteration of which leaves runs in its function's lane body instead; the error is the
/// first exit that blocks it. A method call the lane runs in place (`native_call`, by absolute PC) is no
/// exit even under `MethodCalls::Exit`: the lane lowers it as an intrinsic and guards its callee.
pub(crate) fn iterates_natively(instructions: &[WideInstruction], header: u32, method_calls: MethodCalls,
    native_call: &dyn Fn(usize) -> bool) -> Result<(), usize> {
    let last = instructions.len() - 1;
    let (mut seen, mut pending, mut blocked) = (quench_runtime::bitset::BitSet::new(instructions.len()), vec![0], usize::MAX);
    while let Some(offset) = pending.pop() {
        let instruction = instructions[offset];
        if !seen.insert(offset) {
            continue;
        }
        let call = quench_runtime::staged::calls_ordinary(instruction.op());
        if exits_before_effects(instruction) && !(call && (method_calls == MethodCalls::Inliner || native_call(header as usize + offset))) {
            blocked = blocked.min(offset);
            continue;
        }
        if offset == last {
            return Ok(());
        }
        quench_runtime::bytecode::for_each_instruction_successor(header as usize + offset, instruction, |successor| {
            pending.extend(successor.checked_sub(header as usize).filter(|next| *next <= last));
        });
    }
    Err(blocked.min(last))
}
