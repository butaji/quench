//! The set of Wasm functions a module compile admits: the greatest fixpoint of "every
//! known callee compiles". Each function is planned once; a function that drops sends
//! only its known callers back to be planned again, not the whole module.
use super::{Analysis, Host, Plan};
use crate::bitset::BitSet;
use crate::bytecode::Op;

impl<H: Host> Analysis<'_, H> {
    /// Each function's full plan from `allowed`, `None` outside the callable set, and the
    /// memory slot those plans compile against.
    pub(super) fn settle_callable(&self, allowed: &BitSet) -> (Vec<Option<Plan>>, Option<u32>) {
        let mut memory_slot = None;
        let mut plans: Vec<Option<Plan>> = (0..allowed.len())
            .map(|index| allowed.contains(index).then(|| self.plan(index, allowed, &mut memory_slot)).flatten())
            .collect();
        let mut callable = BitSet::from_fn(plans.len(), |index| plans[index].is_some());
        let edges = self.known_call_edges();
        let mut dropped: Vec<u32> = allowed.iter().filter(|&index| plans[index].is_none()).map(|index| index as u32).collect();
        while let Some(callee) = dropped.pop() {
            let start = edges.partition_point(|&(to, _)| to < callee);
            for &(_, caller) in edges[start..].iter().take_while(|&&(to, _)| to == callee) {
                let caller = caller as usize;
                if callable.contains(caller) {
                    plans[caller] = self.plan(caller, &callable, &mut memory_slot);
                    if plans[caller].is_none() {
                        callable.remove(caller);
                        dropped.push(caller as u32);
                    }
                }
            }
        }
        (plans, memory_slot)
    }

    /// Every statically known call as a `(callee, caller)` pair, sorted by callee.
    fn known_call_edges(&self) -> Vec<(u32, u32)> {
        let mut edges = Vec::new();
        for caller in 0..self.code.functions.len() {
            let function = self.code.function(caller);
            for packed in &function.code {
                let instruction = super::super::dispatch_fast::decoded_instruction(function, *packed);
                if instruction.op() == Op::CallKnown {
                    edges.push((u32::from(instruction.known_function_index()), caller as u32));
                }
            }
        }
        edges.sort_unstable();
        edges
    }
}
