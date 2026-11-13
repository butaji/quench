//! The stage the b-jit rows run on: machine words computed in registers, conditions as `bool`,
//! a failed guard as `None` (the row's caller leaves through its exit hole), and a per-site
//! operand (`site`) as the address of the next value hole `HOLE_V<n>`, which the stitcher
//! patches into the instruction that uses it. An operation with no stencil form calls an
//! `UNSUPPORTED_*` symbol, which `build.rs` rejects, so a row cannot silently use one.

use crate::staged::{Stage, Table};
use crate::typed_ir::Type;
use crate::staged::effects::Region;

unsafe extern "C" {
    static HOLE_V0: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
    fn UNSUPPORTED_store_gate(a: u64, b: u64) -> u64;
    fn UNSUPPORTED_assumption(a: u64) -> u64;
    fn UNSUPPORTED_number_operator(a: u64, b: u64, c: u64) -> u64;
    fn UNSUPPORTED_number_truthy(a: u64) -> u64;
    fn UNSUPPORTED_shape_in(a: u64) -> u64;
    fn UNSUPPORTED_table(a: u64) -> u64;
    fn UNSUPPORTED_site(a: u64) -> u64;
}

/// The stencil stage; `sites` is the number of the next value hole a `site` call names.
#[derive(Clone, Copy)]
pub(crate) struct StencilStage {
    sites: u8,
}

impl StencilStage {
    /// A stage whose first `site` is hole `first`.
    #[inline(always)]
    pub(crate) fn from_hole(first: u8) -> Self {
        Self { sites: first }
    }

    /// The runtime's `Concrete::new`, which the shared rows' interpreter wrappers call; those
    /// wrappers are never used here.
    #[inline(always)]
    pub(crate) fn new(_property_values: *const crate::Value) -> Self {
        Self { sites: 0 }
    }
}

impl Stage for StencilStage {
    type Word = u64;
    type Proof = ();
    type Cond = bool;
    type Miss = ();
    type Imm = u64;

    #[inline(always)]
    fn site(&mut self, _v: u64) -> u64 {
        let n = self.sites;
        self.sites += 1;
        // SAFETY: only the addresses of the statics are taken.
        unsafe {
            match [&raw const HOLE_V0, &raw const HOLE_V1, &raw const HOLE_V2].get(n as usize) {
                Some(hole) => *hole as u64,
                None => UNSUPPORTED_site(n as u64),
            }
        }
    }
    #[inline(always)]
    fn load_site(&mut self, base: u64, offset: u64) -> u64 {
        // SAFETY: the offset is a layout offset the site proved.
        unsafe { ((base.wrapping_add(offset)) as *const u64).read() }
    }
    #[inline(always)]
    fn shape_in_site(&mut self, shape: u64, _shapes: &[u32]) -> bool {
        // SAFETY: never run; the symbol fails the build.
        unsafe { UNSUPPORTED_shape_in(shape) != 0 }
    }
    #[inline(always)]
    fn constant(&mut self, bits: u64) -> u64 { bits }
    #[inline(always)]
    fn load_u32(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: a staged operation loads only from a live record or table at a layout offset.
        u64::from(unsafe { (base.wrapping_add(offset as i64 as u64) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *const u64).read() }
    }
    #[inline(always)]
    fn load_table(&mut self, address: u64) -> u64 { self.load_u64(address, 0) }
    #[inline(always)]
    fn add(&mut self, a: u64, b: u64) -> u64 { a.wrapping_add(b) }
    #[inline(always)]
    fn or(&mut self, a: u64, b: u64) -> u64 { a | b }
    #[inline(always)]
    fn and(&mut self, a: u64, b: u64) -> u64 { a & b }
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a.wrapping_add(imm.into()) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a.wrapping_mul(imm.into()) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a & imm.into() }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a | imm.into() }
    #[inline(always)]
    fn shr_imm(&mut self, a: u64, shift: impl Into<u64>) -> u64 { a >> shift.into() }
    #[inline(always)]
    fn shl(&mut self, a: u64, b: u64) -> u64 { a << (b & 63) }
    #[inline(always)]
    fn sar32_imm(&mut self, a: u64, shift: u32) -> u64 { u64::from(((a as u32 as i32) >> shift) as u32) }
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { a == imm.into() }
    #[inline(always)]
    fn ne_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { a != imm.into() }
    #[inline(always)]
    fn ugt_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { a > imm.into() }
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
        unsafe { (base.wrapping_add(offset as i64 as u64) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn store_gate(&mut self, record_id: u64, value: u64) -> bool {
        // SAFETY: never run; the symbol fails the build.
        unsafe { UNSUPPORTED_store_gate(record_id, value) != 0 }
    }
    #[inline(always)]
    fn assumption(&mut self, watch: u64) -> bool {
        // SAFETY: never run; the symbol fails the build.
        unsafe { UNSUPPORTED_assumption(watch) != 0 }
    }
    #[inline(always)]
    fn number_operator(&mut self, operator: u64, passed: u64, constant: u64) -> (u64, bool) {
        // SAFETY: never run; the symbol fails the build.
        (unsafe { UNSUPPORTED_number_operator(operator, passed, constant) }, false)
    }
    #[inline(always)]
    fn not(&mut self, cond: bool) -> bool { !cond }
    #[inline(always)]
    fn any(&mut self, a: bool, b: bool) -> bool { a | b }
    #[inline(always)]
    fn pick(&mut self, cond: bool, then: bool, otherwise: bool) -> bool { if cond { then } else { otherwise } }
    #[inline(always)]
    fn number_truthy(&mut self, bits: u64) -> bool {
        // SAFETY: never run; the symbol fails the build.
        unsafe { UNSUPPORTED_number_truthy(bits) != 0 }
    }
    #[inline(always)]
    fn shape_in(&mut self, shape: u64, _shapes: &[u32]) -> bool {
        // SAFETY: never run; the symbol fails the build.
        unsafe { UNSUPPORTED_shape_in(shape) != 0 }
    }
    #[inline(always)]
    fn choose(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> u64, otherwise: impl FnOnce(&mut Self) -> u64) -> u64 {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _miss: ()) -> Option<()> { cond.then_some(()) }
    #[inline(always)]
    fn known(&self, _cond: bool) -> Option<bool> { None }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 {
        // SAFETY: never run; the symbol fails the build.
        unsafe { UNSUPPORTED_table(table as u64) }
    }
}

const _: fn(Region, Type) = |_, _| {};
