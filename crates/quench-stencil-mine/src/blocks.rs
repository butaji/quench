//! Basic blocks from the successor lists the runtime derives from each opcode's control-flow
//! layout: a block starts at pc 0, at every jump target and handler entry, and after every
//! instruction that is not a plain fall-through.
use crate::bits::Bits;
use quench_runtime::FlowFunction;
use std::ops::Range;

pub fn split(function: &FlowFunction) -> Vec<Range<usize>> {
    let count = function.instructions.len();
    let mut leader = Bits::new(count + 1);
    leader.insert(0);
    for (pc, instruction) in function.instructions.iter().enumerate() {
        if instruction.successors != [pc as u32 + 1] {
            leader.insert(pc + 1);
            for &target in &instruction.successors {
                leader.insert((target as usize).min(count));
            }
        }
    }
    for &target in &function.handler_targets {
        leader.insert((target as usize).min(count));
    }
    let starts: Vec<usize> = (0..count).filter(|&pc| leader.contains(pc)).collect();
    starts
        .iter()
        .enumerate()
        .map(|(index, &start)| start..starts.get(index + 1).copied().unwrap_or(count))
        .collect()
}
