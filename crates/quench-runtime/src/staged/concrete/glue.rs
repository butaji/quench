//! The runtime's side of `Concrete`'s host hooks (spec 4.4), and the stage's constructors that name runtime
//! types. The hooks take raw pointers and words, so `Concrete` names no heap or VM type; each hook is the body
//! of the call the stage made before it became a hook.

use super::Concrete;
use crate::code_dependencies::PublishedCode;
use crate::heap::Heap;
use crate::staged::StagedRecordSpace;
use crate::staged::spans::{UnitSpan, concrete_units};
use crate::Value;

/// `Concrete::over` for the running VM's heap.
#[inline(always)]
pub fn concrete_over_vm(vm: &crate::vm::Vm) -> Concrete { Concrete::over(&raw const vm.heap) }

/// `Concrete::over_arena_values` of one property arena's base.
#[inline(always)]
pub fn concrete_over_arena(arena: &crate::value_vec::ValueArena) -> Concrete {
    Concrete::over_arena_values(arena.values_base() as u64)
}

/// # Safety
/// `heap` is the live `Heap` a stage was `over`.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_arena_values_base(heap: *const ()) -> u64 {
    // SAFETY: the caller's contract.
    unsafe { (*heap.cast::<Heap>()).arena_values_base() as u64 }
}

/// # Safety
/// `heap` is the live `Heap` a stage was `over`.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_class_space_base(heap: *const ()) -> u64 {
    // SAFETY: the caller's contract.
    unsafe { (*heap.cast::<Heap>()).class_space_base() as u64 }
}

/// # Safety
/// `heap` is the live `Heap` of a stage that stores.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_store_unlogged(heap: *const (), space: StagedRecordSpace, record_id: u32, value: u64) -> bool {
    // SAFETY: the caller's contract.
    unsafe { (*heap.cast::<Heap>()).store_unlogged(space, record_id, Value::from_bits(value)) }
}

/// # Safety
/// `heap` is the live `Heap` of a stage that stores.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_held_store_unlogged(heap: *const (), space: StagedRecordSpace, record_id: u32, value: u64) -> bool {
    // SAFETY: the caller's contract.
    unsafe { (*heap.cast::<Heap>()).held_store_unlogged(space, record_id, Value::from_bits(value)) }
}

/// # Safety
/// `heap` is the live `Heap` of a stage that stores.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_immediate_store_unlogged(heap: *const ()) -> bool {
    // SAFETY: the caller's contract.
    unsafe { (*heap.cast::<Heap>()).immediate_store_unlogged() }
}

#[unsafe(no_mangle)]
pub fn quench_staged_rc_counts_seen() -> bool {
    crate::heap::rc::COUNTS_SEEN.load(core::sync::atomic::Ordering::Relaxed)
}

/// # Safety
/// `heap` is the live `Heap` of a stage that reads the owner tables.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_owner_table_bases(heap: *const ()) -> [u64; 3] {
    // SAFETY: the caller's contract.
    unsafe { (*heap.cast::<Heap>()).owner_table_bases() }
}

/// # Safety
/// `watch` is the address of a live `PublishedCode`.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_published(watch: u64) -> bool {
    // SAFETY: the caller's contract.
    unsafe { (*(watch as *const PublishedCode<()>)).is_published() }
}

/// # Safety
/// The `count` words at `top` are the window's, none live.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_fill_registers(top: *mut u64, count: usize, undefined: u64) {
    // SAFETY: the caller's contract.
    unsafe { crate::vm::fill_registers(top.cast::<Value>(), count, Value::from_bits(undefined)) }
}

/// # Safety
/// The `count` words at `top` are the window's, and `arguments` lie outside it.
#[unsafe(no_mangle)]
pub unsafe fn quench_staged_copy_arguments(top: *mut u64, arguments: *const u64, count: u16) {
    // SAFETY: the caller's contract.
    unsafe { crate::vm::copy_arguments(top.cast::<Value>(), arguments.cast::<Value>(), count) }
}

/// The tuned forward search of `vm::wtf16::search` (memchr, the block filter, memmem).
#[unsafe(no_mangle)]
pub fn quench_staged_find_span(text: UnitSpan<u64>, needle: UnitSpan<u64>, from: u64) -> (bool, u64) {
    let found = concrete_units(text).find(concrete_units(needle), from as usize);
    (found.is_some(), found.map_or(u64::MAX, |index| index as u64))
}

/// The tuned reverse search: memrchr or memmem's reverse search for ASCII, `Units::rfind` otherwise.
#[unsafe(no_mangle)]
pub fn quench_staged_rfind_span(text: UnitSpan<u64>, needle: UnitSpan<u64>, from: u64) -> (bool, u64) {
    let found = concrete_units(text).rfind(concrete_units(needle), from as usize);
    (found.is_some(), found.map_or(u64::MAX, |index| index as u64))
}

/// Slice equality of the units from `start`.
#[unsafe(no_mangle)]
pub fn quench_staged_span_equal_at(text: UnitSpan<u64>, needle: UnitSpan<u64>, start: u64) -> bool {
    let start = start as usize;
    concrete_units(text).slice(start..start + needle.length as usize) == concrete_units(needle)
}
