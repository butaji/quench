//! Basic blocks from the successor lists the runtime derives from each opcode's control-flow
//! layout: a block starts at pc 0, at every jump target and handler entry, and after every
//! instruction that is not a plain fall-through.
use quench_runtime::FlowFunction;
use std::ops::Range;

pub fn split(function: &FlowFunction) -> Vec<Range<usize>> {
    let count = function.instructions.len();
    let mut leader = vec![false; count + 1];
    leader[0] = true;
    for (pc, instruction) in function.instructions.iter().enumerate() {
        if instruction.successors != [pc as u32 + 1] {
            leader[pc + 1] = true;
            for &target in &instruction.successors {
                leader[(target as usize).min(count)] = true;
            }
        }
    }
    for &target in &function.handler_targets {
        leader[(target as usize).min(count)] = true;
    }
    let starts: Vec<usize> = (0..count).filter(|&pc| leader[pc]).collect();
    starts
        .iter()
        .enumerate()
        .map(|(index, &start)| start..starts.get(index + 1).copied().unwrap_or(count))
        .collect()
}
