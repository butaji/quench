//! The interpreter's stage: every staged operation run on machine words of live memory.

use super::{Stage, Table};
use super::region::Access;

/// The interpreter's stage: machine words, read from live memory. It knows every condition,
/// so a guard never fails and each `choose` runs one arm.
#[derive(Clone, Copy)]
pub struct Concrete {
    tables: [u64; 1],
    /// The heap whose store barrier `store_gate` reads; null where no store runs.
    heap: *const crate::heap::Heap,
}

impl Concrete {
    #[inline(always)]
    pub fn new(property_values: *const crate::Value) -> Self {
        Self { tables: [property_values as u64], heap: std::ptr::null() }
    }

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

    /// `storing` over the running VM's heap.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub fn storing_in(vm: &crate::vm::Vm) -> Self { Self::storing(&raw const vm.heap) }

    /// The stage of stores into `heap`'s objects.
    #[inline(always)]
    pub(crate) fn storing(heap: *const crate::heap::Heap) -> Self {
        // SAFETY: the caller's heap is live.
        Self { tables: [unsafe { (*heap).arena_values_base() } as u64], heap }
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
    #[inline(always)]
    fn shr(&mut self, a: u64, b: u64) -> u64 { a.wrapping_shr(b as u32) }
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { let imm = imm.into(); a.wrapping_add(imm) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { let imm = imm.into(); a.wrapping_mul(imm) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { let imm = imm.into(); a & imm }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { let imm = imm.into(); a | imm }
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { let imm = imm.into(); a == imm }
    #[inline(always)]
    fn ne_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { let imm = imm.into(); a != imm }
    #[inline(always)]
    fn ugt_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { let imm = imm.into(); a > imm }
    #[inline(always)]
    fn eq(&mut self, a: u64, b: u64) -> bool { a == b }
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
    fn box_number(&mut self, number: u64) -> u64 { crate::Value::number_inline(f64::from_bits(number)).to_bits() }
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
                crate::vm::fill_registers(top as *mut crate::Value, usize::from(registers), crate::Value::from_bits(undefined));
                crate::vm::copy_arguments(top as *mut crate::Value, arguments as *const crate::Value, count.min(registers));
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
    fn store_gate(&mut self, space: super::StagedRecordSpace, record_id: u64, value: u64) -> bool {
        // SAFETY: a stage that stores holds its live heap.
        !self.heap.is_null() && unsafe { (*self.heap).store_unlogged(space, record_id as u32, crate::Value::from_bits(value)) }
    }
    #[inline(always)]
    fn held_store_gate(&mut self, space: super::StagedRecordSpace, record_id: u64, value: u64) -> bool {
        // SAFETY: a stage that stores holds its live heap.
        !self.heap.is_null() && unsafe { (*self.heap).held_store_unlogged(space, record_id as u32, crate::Value::from_bits(value)) }
    }
    #[inline(always)]
    fn immediate_store_open(&mut self) -> bool {
        if self.heap.is_null() {
            // A stage that holds no heap (the interpreter's rows) reads whether any heap counts.
            return !crate::heap::rc::COUNTS_SEEN.load(core::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: a stage that stores holds its live heap.
        unsafe { (*self.heap).immediate_store_unlogged() }
    }
    #[inline(always)]
    fn number_operator(&mut self, operator: u64, passed: u64, constant: u64) -> (u64, bool) {
        let operated = crate::Value::from_bits(passed).as_number().zip(crate::Value::from_bits(constant).as_number())
            .and_then(|(passed, constant)| crate::vm::operations::static_number_binary(operator as u32, passed, constant))
            .and_then(|constant| match constant {
                crate::bytecode::Constant::Number(number) => Some(crate::Value::number(number).to_bits()),
                _ => None,
            });
        (operated.unwrap_or(0), operated.is_some())
    }
    #[inline(always)]
    fn assumption(&mut self, watch: u64) -> bool {
        // SAFETY: the operand is the address of a live watch (`staged::holder::concrete_holder_read`).
        unsafe { (*(watch as *const crate::code_dependencies::PublishedCode<()>)).is_published() }
    }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 {
        match table {
            Table::PropertyValues => self.tables[0],
            // A static's address: no field to load.
            Table::Layouts => crate::heap::layouts::table_base() as u64,
        }
    }
    #[inline(always)]
    fn walk_prototypes(&mut self, records: u64, object: u64, target: u64, max_depth: u32) -> (bool, bool) {
        super::object::walk_prototypes_loop(self, records, object, target, max_depth)
    }
}
