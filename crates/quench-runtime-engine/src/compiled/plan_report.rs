//! The planning report of a function (`QUENCH_WASM_JIT_PLAN_MEMORY`, diagnostic runs only): the
//! states the plan held, and the time and memory planning took.

use super::{AnalysisState, Slot, jit_memory_snapshot};

/// Reports the plan of function `index` that started at `started`: its bytecodes, registers, the
/// reachable states and their stored slots, and the memory before and after (`memory_before`).
pub(crate) fn report(
    index: usize,
    started: std::time::Instant,
    memory_before: Option<(u64, u64)>,
    bytecodes: usize,
    registers: usize,
    states: &[Option<AnalysisState>],
) {
    let reachable = states.iter().flatten().count();
    let stored_slots: usize = states.iter().flatten().map(|state| state.slots.len()).sum();
    let slot_capacity: usize = states.iter().flatten().map(|state| state.slots.capacity()).sum();
    let dense_state_bytes = reachable.saturating_mul(registers).saturating_mul(std::mem::size_of::<Slot>());
    let sparse_state_bytes = states
        .len()
        .saturating_mul(std::mem::size_of::<Option<AnalysisState>>())
        .saturating_add(slot_capacity.saturating_mul(std::mem::size_of::<(u16, Slot)>()));
    let memory_after = jit_memory_snapshot();
    eprintln!(
        "QUENCH_WASM_JIT_PLAN_MEMORY function={index} bytecodes={bytecodes} registers={registers} reachable={reachable} stored_slots={stored_slots} dense_state_bytes={dense_state_bytes} sparse_state_bytes={sparse_state_bytes} elapsed_us={} rss_kib_before={:?} rss_kib_after={:?} hwm_kib_before={:?} hwm_kib_after={:?}",
        started.elapsed().as_micros(),
        memory_before.map(|memory| memory.0),
        memory_after.map(|memory| memory.0),
        memory_before.map(|memory| memory.1),
        memory_after.map(|memory| memory.1),
    );
}
