//! The growth fact of a module's defined functions (`growing_functions`): whether a function may
//! grow the linear memory it runs over, which may move or resize it. A caller reads the fact after
//! a direct call: a callee that cannot grow the memory leaves the caller's base and length as they
//! are, so the caller need not reload them (the b-jit's `CALL_KEEP` row, `wasm_frame.rs`, and the
//! u-jit's `memory_length`, `wasm_memory_stage.rs`).
//!
//! A function may grow when it runs `memory.grow`, makes a reference call (an import, or an
//! indirect call whose target the program does not name), runs a table, GC, exception or atomic
//! step (each may reach a VM callout, which is not audited here), or calls a function that may
//! grow. Only a direct call to a defined function is resolved; the fixpoint over those calls carries
//! growth up the call graph. A shared memory may grow in another thread at any point, so every
//! function may grow over one.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bitset::BitSet;
use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::staged::wasm_call::Call;
use quench_runtime::staged::wasm_memory::Memory;
use quench_runtime::staged::wasm_step::Step;

/// Whether a step by itself may grow the memory: a memory growth, a reference call, or a step that
/// may reach a VM callout. A direct call's growth is its callee's (`growing_functions`).
fn grows_directly(step: Step) -> bool {
    matches!(
        step,
        Step::Memory(Memory::Grow)
            | Step::Call(Call::Reference)
            | Step::Table(_)
            | Step::Gc(_)
            | Step::Exception(_)
            | Step::Atomic(_)
    )
}

/// The defined functions `0..count` of `code` that may grow the memory (see the module's docs).
/// `shared` says the memory is shared between threads, which makes every function one that may.
pub(crate) fn growing_functions(code: &ResidualProgram, count: usize, shared: bool) -> BitSet {
    if shared {
        return BitSet::from_fn(count, |_| true);
    }
    let mut grows = BitSet::new(count);
    // The functions that directly call each function, to propagate growth back to them.
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); count];
    for index in 0..count {
        let function = code.function(index);
        for packed in &function.code {
            let instruction = function.widened(*packed);
            let Some(step) = Step::of(instruction.op(), instruction.imm()) else {
                continue;
            };
            if grows_directly(step) {
                grows.insert(index);
            }
            if let Step::Call(Call::Known) = step {
                match callers.get_mut(usize::from(instruction.known_function_index())) {
                    Some(list) => list.push(index),
                    None => {
                        grows.insert(index);
                    }
                }
            }
        }
    }
    let mut pending: Vec<usize> = grows.iter().collect();
    while let Some(callee) = pending.pop() {
        for &caller in &callers[callee] {
            if grows.insert(caller) {
                pending.push(caller);
            }
        }
    }
    grows
}
