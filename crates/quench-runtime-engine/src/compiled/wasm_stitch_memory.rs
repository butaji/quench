//! The bulk memory steps of the b-jit's Wasm functions: `memory.size`, `memory.fill` and
//! `memory.copy` on the frame's memory, as the rows of `quench-bjit/stencils/wasm_rows_memory.rs`
//! (`MEMORY_SIZE`, `MEMORY_FILL`, `MEMORY_COPY`). The loads and stores are `memory()` in
//! `wasm_stitch.rs`; `plan()` tries it, then this.
//!
//! Only the frame's memory has rows. A stitched function binds no other memory (`plan` rejects
//! such a binding, and a binding of the frame's memory places nothing), so every memory register
//! a bulk step names holds the frame's memory. `memory.grow` has no row: it calls the VM.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{Planned, codes, row, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_memory::Memory;

/// The rows of a bulk step on the frame's memory, or `None` for a step these rows do not run:
/// `memory.grow`, a load or a store.
pub(crate) fn bulk(instruction: WideInstruction, step: Memory) -> Option<Vec<Planned>> {
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    let planned = match step {
        // `a = memory.size b`: the memory in `b` is not read; the result goes to hole 0.
        Memory::Size => row((codes::MEMORY_SIZE, 0, codes::W64, codes::REGISTER), [slot(a), 0, 0], None),
        // `memory.fill`: the memory in `a`, the byte in `b`, the length in `c`, the destination in
        // the pair's first register. The row's holes: destination (A), byte (B), length (fourth).
        Memory::Fill => {
            let (destination, _) = instruction.register_pair();
            let mut planned = row((codes::MEMORY_FILL, 0, codes::W64, codes::REGISTER), [0, slot(destination), slot(b)], None);
            planned.holes[3] = slot(c);
            planned
        }
        // `memory.copy`: the memories in `a` and `b` (both the frame's), the length in `c`, the
        // destination and source in the pair. The row's holes: destination (A), source (B), length.
        Memory::Copy => {
            let (destination, source) = instruction.register_pair();
            let mut planned = row((codes::MEMORY_COPY, 0, codes::W64, codes::REGISTER), [0, slot(destination), slot(source)], None);
            planned.holes[3] = slot(c);
            planned
        }
        Memory::Load(_) | Memory::Store(_) | Memory::Grow => return None,
    };
    Some(vec![planned])
}
