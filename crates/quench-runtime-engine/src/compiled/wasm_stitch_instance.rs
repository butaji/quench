//! The instance operations of the b-jit's Wasm functions that call the VM: `memory.grow`
//! (`Memory::Grow`, `staged/wasm_memory.rs`), the row `quench-bjit/stencils/wasm_rows_instance.rs`
//! (`MEMORY_GROW`). The u-jit's callout is `memory_grow_callout` (`vm/compiled/wasm_memory_stage.rs`),
//! reached through the context's `memory_grow` slot (`CONTEXT_MEMORY_GROW`), which the VM fills
//! when it builds the run's context. The row's offsets are `compiled.rs`'s constants, so no
//! `World` field is needed for them.
//!
//! Not planned (`None`), each for a reason the u-jit or the row cannot meet:
//! - `table.get`, `table.set`, `table.size`, `table.grow`, `table.fill`, `table.copy`,
//!   `table.init`: no `Step` variant reads them. Their opcodes carry no `class` (`bytecode.rs`
//!   1034-1039), so `Step::of` is `None` and `plan` refuses the function. The u-jit has no lowering
//!   for them either (no `Op::WasmTable*` in `vm/compiled/`); `table.size` is interpreter-only
//!   (`vm/wasm.rs`, the comment at ~3130), so there is no inline code to mirror. The interpreter
//!   (`vm/dispatch_wasm.rs` 31-80) reads the table as a `Value` (a heap reference) from a register;
//!   the stitched frame holds raw words only (`wasm_rows.rs` header), so a row cannot hold the
//!   operand, and a callout would need a `Value`-carrying ABI that no existing callout has.
//! - `memory.init`: no class (`bytecode.rs:1040`), so `Step::of` is `None`; the interpreter
//!   (`dispatch_wasm.rs` 83-120) reads the data segment's binding as a `Value`, the same obstacle.
//! - `data.drop`, `elem.drop`: lowered as `LoadConst` + `StoreCapture` of a void value into the
//!   environment slot (`wasm/memory.rs:731`, `wasm/element.rs:87-102`). `StoreCapture` is no
//!   `Step`, so such a function has no plan. Writing the environment slot is a heap write of a
//!   `Value`, which raw words cannot do.
//!
//! `memory.grow` is held off (`PLANNED`). Its row is complete, but a growth of an unguarded memory
//! moves the memory's base (`wasm/memory.rs`, `LinearRepr::Heap` resizes its vector), and the
//! stitched frame's base word (`s.p[1]`) would then be stale. The u-jit admits its callout only
//! where the memory is a guarded reservation, which grows in place (`compiled.rs`, the
//! `analysis.callouts` loop: `stable_memory`). Enabling the row needs `World` to say the same
//! (`memory_stable`, computed in `stitched_entry` from `guarded_memory`) and this plan to refuse
//! an unstable memory. Nothing in the row reads the base from the context, so that is the only
//! way to keep it sound.
//!
//! Caller staleness: after a `call()` or a `call_indirect`, the caller reloads its length from the
//! context (`reload_memory`, `wasm_rows.rs`), which a callee's growth refreshes
//! (`refresh_memory_length`), so the caller's header and pins agree with the memory.

#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::{CONTEXT_MEMORY_GROW, CONTEXT_MEMORY_LENGTH};
use super::{Planned, World, codes, row, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_memory::Memory;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The rows of the instance operation `step` at `pc`, or `None` where it has none (every operation
/// but `memory.grow`, see the module docs). A growth must leave the memory's base in place
/// (`World::memory_stable`): the row reloads only the length.
///
/// `memory.grow`: `a = memory.grow b by c`. Holes: D = a (hole 0), the pages = c (HOLE_A), the
/// offset of `memory_grow` (HOLE_B), the offset of `memory_length` (HOLE_FOURTH). The memory in
/// `b` is not read: as the bulk rows assume, a stitched function binds only the frame's memory
/// (`wasm_stitch_memory.rs` header), which the callout grows.
pub(crate) fn instance_op(
    _pc: usize,
    instruction: WideInstruction,
    step: Memory,
    world: &World<'_>,
) -> Option<Vec<Planned>> {
    let Memory::Grow = step else {
        return None;
    };
    if !world.memory_stable {
        return None;
    }
    let grow = u64::try_from(CONTEXT_MEMORY_GROW).ok()?;
    let length = u64::try_from(CONTEXT_MEMORY_LENGTH).ok()?;
    let mut planned = row((codes::MEMORY_GROW, 0, codes::W64, codes::REGISTER), [slot(instruction.a()), slot(instruction.c()), grow], None);
    planned.holes[3] = length;
    Some(vec![planned])
}
