//! The registers a lane body's entry finds written (`Facts::written`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::Facts;
use quench_runtime::bytecode::WideInstruction;

impl Facts {
    /// The registers written by what the analysis reached and by all that runs past an
    /// instruction it left at, not by a branch it decided (the exact step's code, until the
    /// body enters again).
    pub(crate) fn written(&self, instructions: &[WideInstruction], writes: &dyn Fn(WideInstruction) -> super::Writes, count: usize) -> quench_runtime::dataflow::BitSet {
        let mut left = Vec::new();
        for pc in self.reached.iter().filter(|pc| !instructions[*pc].op().control_flow_layout().jumps()) {
            quench_runtime::bytecode::for_each_instruction_successor(pc, instructions[pc], |next| left.push(next));
        }
        let past = quench_runtime::bytecode::reachable_from(instructions, left.into_iter().filter(|pc| !self.reached(*pc)));
        let mut written = quench_runtime::dataflow::BitSet::with_capacity(count);
        for pc in self.reached.iter().chain(past.iter()) {
            for register in writes(instructions[pc]) { written.insert(usize::from(register)); }
        }
        written
    }
}
