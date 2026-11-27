//! The interpreter's stage: every staged operation run on machine words of live memory.

use super::{Stage, Table};
use super::region::Access;
use super::spans::UnitSpan;
use super::StagedRecordSpace;

mod glue;
pub use glue::{concrete_over_arena, concrete_over_vm};

// Host hooks (spec 4.4): the runtime defines each `#[unsafe(no_mangle)]` in `concrete/glue.rs`. They take
// raw pointers and words, so the stage names no heap or VM type; `heap` is the `Heap` the stage was `over`.
unsafe extern "Rust" {
    fn quench_staged_arena_values_base(heap: *const ()) -> u64;
    fn quench_staged_class_space_base(heap: *const ()) -> u64;
    fn quench_staged_store_unlogged(heap: *const (), space: StagedRecordSpace, record_id: u32, value: u64) -> bool;
    fn quench_staged_held_store_unlogged(heap: *const (), space: StagedRecordSpace, record_id: u32, value: u64) -> bool;
    fn quench_staged_immediate_store_unlogged(heap: *const ()) -> bool;
    safe fn quench_staged_rc_counts_seen() -> bool;
    fn quench_staged_owner_table_bases(heap: *const ()) -> [u64; 3];
    fn quench_staged_published(watch: u64) -> bool;
    fn quench_staged_fill_registers(top: *mut u64, count: usize, undefined: u64);
    fn quench_staged_copy_arguments(top: *mut u64, arguments: *const u64, count: u16);
    fn quench_staged_find_span(text: UnitSpan<u64>, needle: UnitSpan<u64>, from: u64) -> (bool, u64);
    fn quench_staged_rfind_span(text: UnitSpan<u64>, needle: UnitSpan<u64>, from: u64) -> (bool, u64);
    fn quench_staged_span_equal_at(text: UnitSpan<u64>, needle: UnitSpan<u64>, start: u64) -> bool;
}

/// The interpreter's stage: machine words, read from live memory. It knows every condition,
/// so a guard never fails and each `choose` runs one arm.
#[derive(Clone, Copy)]
pub struct Concrete {
    /// The heap's table bases, read once by `over`: the property arena's values (`Table::PropertyValues`)
    /// and the class space's base (`Table::ClassSpace`). Zero in a `pure` stage.
    tables: [u64; 2],
    /// The heap whose tables and store barrier the stage reads; null in a `pure` stage.
    heap: *const (),
}

impl Concrete {
    /// The stage over `heap`: the one heap-bearing constructor. Every table base a stage reads is taken
    /// from `heap` here, so no stage holds a base of its own. A stage that reads an object record, a
    /// property run, a slot owner table or the store barrier is over its heap.
    #[inline(always)]
    pub fn over<H>(heap: *const H) -> Self {
        let heap = heap.cast::<()>();
        // SAFETY: the caller's heap is live.
        Self { tables: unsafe { [quench_staged_arena_values_base(heap), quench_staged_class_space_base(heap)] }, heap }
    }

    /// A stage over one property arena's base, for a read of a record the caller holds, inside a heap operation
    /// that has split the heap's borrow (so no `&Heap` exists). It reads `Table::PropertyValues` only; the
    /// class space and the store barrier are the heap's, and reading either asserts (`table`).
    #[inline(always)]
    pub fn over_arena_values(values: u64) -> Self {
        Self { tables: [values, 0], heap: std::ptr::null() }
    }

    /// A stage with no heap, for a row set that reads only the words it is given (numbers, units, bytes,
    /// tags, frame words) and never a heap table: `table` asserts that no heap table is read.
    #[inline(always)]
    pub fn pure() -> Self { Self { tables: [0; 2], heap: std::ptr::null() } }

    /// The `T` at `base + offset`, which need not be aligned (a typed array's bytes).
    #[inline(always)]
    fn read<T>(&self, base: u64, offset: i32) -> T {
        // SAFETY: a staged operation loads only from a live record, backing or buffer at a layout offset.
        unsafe { ((base as *const u8).offset(offset as isize) as *const T).read_unaligned() }
    }

    /// Writes `value` at `base + offset`, which need not be aligned.
    #[inline(always)]
    fn write<T>(&self, base: u64, offset: i32, value: T) {
        // SAFETY: as `read`; a store writes only a live backing or buffer.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut T).write_unaligned(value) }
    }

}

crate::projection!(Concrete => Concrete);

impl Stage for Concrete {
    type Word = u64;
    type Proof = ();
    type Cond = bool;
    type Miss = ();
    type Imm = u64;

    #[inline(always)]
    fn site(&mut self, v: u64) -> u64 { v }
    #[inline(always)]
    fn load_site(&mut self, _at: Access, base: u64, offset: u64) -> u64 {
        // SAFETY: as `load_u64`; the offset is a layout offset the site proved.
        unsafe { ((base as *const u8).offset(offset as isize) as *const u64).read() }
    }
    #[inline(always)]
    fn shape_in_site(&mut self, shape: u64, shapes: &[u32]) -> bool { self.shape_in(shape, shapes) }

    #[inline(always)]
    fn constant(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn load_u32(&mut self, _at: Access, base: u64, offset: i32) -> u64 {
        // SAFETY: a staged operation loads only from a live record or table at a layout offset.
        u64::from(unsafe { ((base as *const u8).offset(offset as isize) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, _at: Access, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`; records and arena values are 8-byte aligned.
        unsafe { ((base as *const u8).offset(offset as isize) as *const u64).read() }
    }
    #[inline(always)]
    fn load_table(&mut self, at: Access, address: u64) -> u64 { self.load_u64(at, address, 0) }
    super::word_ops::word_stage_ops!();
    /// A plain Rust loop: the body runs on the state until it says the loop ends.
    #[inline(always)]
    fn loop_words<const N: usize>(&mut self, init: [u64; N], mut body: impl FnMut(&mut Self, [u64; N]) -> (bool, [u64; N]))
        -> [u64; N] {
        let mut state = init;
        loop {
            let (go_on, next) = body(self, state);
            state = next;
            if !go_on {
                break;
            }
        }
        state
    }
    #[inline(always)]
    fn uextend32(&mut self, a: u64) -> u64 { a }
    #[inline(always)]
    fn store_u32(&mut self, _at: Access, base: u64, offset: i32, value: u64) {
        // SAFETY: a staged operation stores only into a live record or backing at a layout offset.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, _at: Access, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`; values and backings are 8-byte aligned.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn load_u8(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<u8>(base, offset)) }
    #[inline(always)]
    fn load_i8(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<i8>(base, offset) as i32 as u32) }
    #[inline(always)]
    fn load_u16(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<u16>(base, offset)) }
    #[inline(always)]
    fn load_i16(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<i16>(base, offset) as i32 as u32) }
    #[inline(always)]
    fn load_i32(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<u32>(base, offset)) }
    #[inline(always)]
    fn load_f32(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<u32>(base, offset)) }
    #[inline(always)]
    fn load_f64(&mut self, _at: Access, base: u64, offset: i32) -> u64 { self.read::<u64>(base, offset) }
    #[inline(always)]
    // `Value::number` without its call (`Value::number_inline`): an element or slot read boxes its Number in
    // place, as the row's other arms make their words.
    fn box_number(&mut self, number: super::Float64<u64>) -> u64 { crate::Value::number_inline(f64::from_bits(number.word())).to_bits() }
    #[inline(always)]
    fn store_u8(&mut self, _at: Access, base: u64, offset: i32, value: u64) { self.write(base, offset, value as u8) }
    #[inline(always)]
    fn store_u16(&mut self, _at: Access, base: u64, offset: i32, value: u64) { self.write(base, offset, value as u16) }
    #[inline(always)]
    fn store_f32(&mut self, _at: Access, base: u64, offset: i32, value: u64) { self.write(base, offset, value as u32) }
    #[inline(always)]
    fn store_f64(&mut self, _at: Access, base: u64, offset: i32, value: u64) { self.write(base, offset, value) }
    #[inline(always)]
    fn f32_to_f64(&mut self, a: u64) -> u64 { f64::from(f32::from_bits(a as u32)).to_bits() }
    #[inline(always)]
    fn f64_to_f32(&mut self, a: u64) -> u64 { u64::from((f64::from_bits(a) as f32).to_bits()) }
    #[inline(always)]
    fn clamp_u8(&mut self, a: u64) -> u64 {
        u64::from(super::math::clamp_u8(f64::from_bits(a)))
    }
    #[inline(always)]
    fn fill_window(&mut self, top: u64, undefined: u64, prefix: &[u64], passed: super::Passed<'_, u64>, registers: u16) {
        match passed {
            // SAFETY: the window at `top` holds `registers` words, none live, and the `count` arguments lie
            // outside it (`protocol::fill_window`): the interpreter's own fill and argument copy.
            super::Passed::Window { arguments, count } if prefix.is_empty() => unsafe {
                quench_staged_fill_registers(top as *mut u64, usize::from(registers), undefined);
                quench_staged_copy_arguments(top as *mut u64, arguments as *const u64, count.min(registers));
            },
            _ => super::fill_by_rows(self, top, undefined, prefix, passed, registers),
        }
    }
    #[inline(always)]
    fn shape_in(&mut self, shape: u64, shapes: &[u32]) -> bool {
        u32::try_from(shape).is_ok_and(|shape| shapes.binary_search(&shape).is_ok())
    }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
    /// The branch `cond` takes, with its leaving kept as its `None`: the default's
    /// `Cell` flag and closure frame cost each staged `pop()` a call.
    #[inline(always)]
    fn choose_leaving(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> Option<u64>,
        otherwise: impl FnOnce(&mut Self) -> Option<u64>) -> Option<u64> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _miss: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn known(&self, cond: bool) -> Option<bool> { Some(cond) }
    #[inline(always)]
    fn store_gate(&mut self, space: StagedRecordSpace, record_id: u64, value: u64) -> bool {
        // SAFETY: a stage that stores holds its live heap.
        !self.heap.is_null() && unsafe { quench_staged_store_unlogged(self.heap, space, record_id as u32, value) }
    }
    #[inline(always)]
    fn held_store_gate(&mut self, space: StagedRecordSpace, record_id: u64, value: u64) -> bool {
        // SAFETY: a stage that stores holds its live heap.
        !self.heap.is_null() && unsafe { quench_staged_held_store_unlogged(self.heap, space, record_id as u32, value) }
    }
    #[inline(always)]
    fn immediate_store_open(&mut self) -> bool {
        if self.heap.is_null() {
            // A stage that holds no heap (the interpreter's rows) reads whether any heap counts.
            return !quench_staged_rc_counts_seen();
        }
        // SAFETY: a stage that stores holds its live heap.
        unsafe { quench_staged_immediate_store_unlogged(self.heap) }
    }
    #[inline(always)]
    fn assumption(&mut self, watch: u64) -> bool {
        // SAFETY: the operand is the address of a live watch (`staged::holder::concrete_holder_read`).
        unsafe { quench_staged_published(watch) }
    }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 {
        match table {
            // A static's address: no field to load.
            Table::Layouts => crate::heap::layouts::table_base() as u64,
            // The heap's bases, taken once by `over` (or `concrete_over_arena`): a pure stage holds none of them, and an
            // arena's base is never null (a `Vec`'s pointer, dangling when empty, is not).
            Table::PropertyValues => {
                debug_assert!(self.tables[0] != 0, "the property arena is read by a stage over an arena or a heap");
                self.tables[0]
            }
            Table::ClassSpace => {
                debug_assert!(!self.heap.is_null(), "the class space is read by a stage over its heap");
                self.tables[1]
            }
            // The owner tables are the heap's: a stage that reads them is a store's (`storing`).
            Table::SlotChains | Table::ChainOwners | Table::OwnerTypes => {
                debug_assert!(!self.heap.is_null(), "a slot owner table is read by a stage that holds its heap");
                // SAFETY: a stage that reads the owner tables holds its live heap (`store_gate`).
                let [chains, owners, types] = unsafe { quench_staged_owner_table_bases(self.heap) };
                match table {
                    Table::SlotChains => chains,
                    Table::ChainOwners => owners,
                    _ => types,
                }
            }
        }
    }
    #[inline(always)]
    fn walk_prototypes(&mut self, records: u64, object: u64, target: u64, max_depth: u32) -> (bool, bool) {
        super::object::walk_prototypes_loop(self, records, object, target, max_depth)
    }
    /// The tuned forward search of `vm::wtf16::search` (memchr, the block filter, memmem).
    #[inline(always)]
    fn find_span(&mut self, text: UnitSpan<u64>, needle: UnitSpan<u64>, from: u64) -> (bool, u64) {
        // SAFETY: the hook reads the two live spans the caller holds for the call.
        unsafe { quench_staged_find_span(text, needle, from) }
    }
    /// The tuned reverse search: memrchr or memmem's reverse search for ASCII, `Units::rfind` otherwise.
    #[inline(always)]
    fn rfind_span(&mut self, text: UnitSpan<u64>, needle: UnitSpan<u64>, from: u64) -> (bool, u64) {
        // SAFETY: as `find_span`.
        unsafe { quench_staged_rfind_span(text, needle, from) }
    }
    /// Slice equality of the units from `start`.
    #[inline(always)]
    fn span_equal_at(&mut self, text: UnitSpan<u64>, needle: UnitSpan<u64>, start: u64) -> bool {
        // SAFETY: as `find_span`.
        unsafe { quench_staged_span_equal_at(text, needle, start) }
    }
    /// memcpy of the units.
    #[inline(always)]
    fn copy_span(&mut self, wide: bool, target: u64, source: u64, length: u64) {
        let bytes = length as usize * if wide { 2 } else { 1 };
        // SAFETY: a Concrete span names live, disjoint units the caller holds for the call.
        unsafe {
            let (from, to) = (source as *const u8, target as *mut u8);
            // A short run is cheaper copied in line than through a memcpy call: two overlapping
            // reads of the widest word that fits, from each end.
            if bytes >= 8 && bytes <= 16 {
                let (head, tail) = (from.cast::<u64>().read_unaligned(), from.add(bytes - 8).cast::<u64>().read_unaligned());
                to.cast::<u64>().write_unaligned(head);
                to.add(bytes - 8).cast::<u64>().write_unaligned(tail);
            } else if bytes >= 4 && bytes < 8 {
                let (head, tail) = (from.cast::<u32>().read_unaligned(), from.add(bytes - 4).cast::<u32>().read_unaligned());
                to.cast::<u32>().write_unaligned(head);
                to.add(bytes - 4).cast::<u32>().write_unaligned(tail);
            } else if bytes >= 2 && bytes < 4 {
                let (head, tail) = (from.cast::<u16>().read_unaligned(), from.add(bytes - 2).cast::<u16>().read_unaligned());
                to.cast::<u16>().write_unaligned(head);
                to.add(bytes - 2).cast::<u16>().write_unaligned(tail);
            } else if bytes == 1 {
                to.write(from.read());
            } else if bytes > 16 {
                std::ptr::copy_nonoverlapping(source as *const u8, target as *mut u8, bytes);
            }
        }
    }
    /// memset of a byte, or a slice fill of a wide unit.
    #[inline(always)]
    fn fill_span(&mut self, wide: bool, target: u64, unit: u64, length: u64) {
        // SAFETY: a Concrete span names live units the caller holds for the call.
        unsafe {
            if wide {
                std::slice::from_raw_parts_mut(target as *mut u16, length as usize).fill(unit as u16);
            } else if (8..=16).contains(&length) {
                // A short fill is two overlapping words of the byte, from each end, not a memset call.
                let (to, word) = (target as *mut u8, (unit & 0xff) * 0x0101_0101_0101_0101);
                to.cast::<u64>().write_unaligned(word);
                to.add(length as usize - 8).cast::<u64>().write_unaligned(word);
            } else {
                std::slice::from_raw_parts_mut(target as *mut u8, length as usize).fill(unit as u8);
            }
        }
    }
    /// The slice loop of the case map, which the compiler vectorizes.
    #[inline(always)]
    fn case_span(&mut self, target: u64, source: u64, length: u64, upper: bool) {
        // SAFETY: a Concrete span names live, disjoint bytes the caller holds for the call; the
        // target may be uninitialized, so it is written through its pointer only.
        let bytes = unsafe { std::slice::from_raw_parts(source as *const u8, length as usize) };
        let out = target as *mut u8;
        for (index, byte) in bytes.iter().enumerate() {
            let mapped = if upper { byte.to_ascii_uppercase() } else { byte.to_ascii_lowercase() };
            unsafe { out.add(index).write(mapped) };
        }
    }
    /// The slice scan of the case map.
    #[inline(always)]
    fn case_changes_span(&mut self, source: u64, length: u64, upper: bool) -> bool {
        // SAFETY: a Concrete span names live bytes the caller holds for the call.
        let bytes = unsafe { std::slice::from_raw_parts(source as *const u8, length as usize) };
        if upper { bytes.iter().any(u8::is_ascii_lowercase) } else { bytes.iter().any(u8::is_ascii_uppercase) }
    }
}
