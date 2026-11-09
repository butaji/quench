//! The interpreter's stage: every staged operation run on machine words of live memory.

use super::{Stage, Table};

/// The interpreter's stage: machine words, read from live memory. It knows every condition,
/// so a guard never fails and each `choose` runs one arm.
#[derive(Clone, Copy)]
pub(crate) struct Concrete {
    tables: [u64; Table::COUNT],
}

impl Concrete {
    #[inline(always)]
    pub(crate) fn new(property_values: *const crate::Value) -> Self {
        Self { tables: [property_values as u64] }
    }
}

impl Stage for Concrete {
    type Word = u64;
    type Proof = ();
    type Cond = bool;
    type Miss = ();

    #[inline(always)]
    fn constant(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn load_u32(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: a staged operation loads only from a live record or table at a layout offset.
        u64::from(unsafe { ((base as *const u8).offset(offset as isize) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`; records and arena values are 8-byte aligned.
        unsafe { ((base as *const u8).offset(offset as isize) as *const u64).read() }
    }
    #[inline(always)]
    fn load_table(&mut self, address: u64) -> u64 { self.load_u64(address, 0) }
    #[inline(always)]
    fn add(&mut self, a: u64, b: u64) -> u64 { a.wrapping_add(b) }
    #[inline(always)]
    fn or(&mut self, a: u64, b: u64) -> u64 { a | b }
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_add(imm) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: u64) -> u64 { a.wrapping_mul(imm) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: u64) -> u64 { a & imm }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: u64) -> u64 { a | imm }
    #[inline(always)]
    fn shr_imm(&mut self, a: u64, shift: u32) -> u64 { a >> shift }
    #[inline(always)]
    fn sar32_imm(&mut self, a: u64, shift: u32) -> u64 { u64::from(((a as u32 as i32) >> shift) as u32) }
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: u64) -> bool { a == imm }
    #[inline(always)]
    fn ne_imm(&mut self, a: u64, imm: u64) -> bool { a != imm }
    #[inline(always)]
    fn ugt_imm(&mut self, a: u64, imm: u64) -> bool { a > imm }
    #[inline(always)]
    fn ult(&mut self, a: u64, b: u64) -> bool { a < b }
    #[inline(always)]
    fn uextend32(&mut self, a: u64) -> u64 { a }
    #[inline(always)]
    fn sub(&mut self, a: u64, b: u64) -> u64 { a.wrapping_sub(b) }
    #[inline(always)]
    fn select(&mut self, cond: bool, then: u64, otherwise: u64) -> u64 { if cond { then } else { otherwise } }
    #[inline(always)]
    fn store_u32(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: a staged operation stores only into a live record or backing at a layout offset.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`; values and backings are 8-byte aligned.
        unsafe { ((base as *mut u8).offset(offset as isize) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn not(&mut self, cond: bool) -> bool { !cond }
    #[inline(always)]
    fn any(&mut self, a: bool, b: bool) -> bool { a | b }
    #[inline(always)]
    fn pick(&mut self, cond: bool, then: bool, otherwise: bool) -> bool { if cond { then } else { otherwise } }
    #[inline(always)]
    fn number_truthy(&mut self, bits: u64) -> bool { crate::vm::number_is_truthy(f64::from_bits(bits)) }
    #[inline(always)]
    fn shape_in(&mut self, shape: u64, shapes: &[u32]) -> bool {
        u32::try_from(shape).is_ok_and(|shape| shapes.binary_search(&shape).is_ok())
    }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _miss: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn known(&self, cond: bool) -> Option<bool> { Some(cond) }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 { self.tables[table as usize] }
}
