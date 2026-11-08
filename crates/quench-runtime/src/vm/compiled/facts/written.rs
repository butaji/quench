//! The registers a lane body's entry finds written (`Facts::written`).
use super::Facts;
use crate::bytecode::{Register, WideInstruction};
use cranelift_codegen::bitset::CompoundBitSet;

impl Facts {
    /// The registers written by what the analysis reached and by all that runs past an
    /// instruction it left at, not by a branch it decided (the exact step's code, until the
    /// body enters again).
    pub(in crate::vm::compiled) fn written(&self, instructions: &[WideInstruction], writes: &dyn Fn(WideInstruction) -> Vec<Register>, count: usize) -> CompoundBitSet {
        let mut left = Vec::new();
        for pc in self.reached.iter().filter(|pc| !instructions[*pc].op().control_flow_layout().jumps()) {
            crate::bytecode::for_each_instruction_successor(pc, instructions[pc], |next| left.push(next));
        }
        let past = crate::bytecode::reachable_from(instructions, left.into_iter().filter(|pc| !self.reached(*pc)));
        let mut written = CompoundBitSet::with_capacity(count);
        for pc in self.reached.iter().chain(past.iter()) {
            for register in writes(instructions[pc]) { written.insert(usize::from(register)); }
        }
        written
    }
}
