//! The stage the b-jit rows run on: machine words computed in registers, conditions as `bool`,
//! a failed guard as `None` (the row's caller leaves through its exit hole), and a per-site
//! operand (`site`) as the address of the next value hole `HOLE_V<n>`, which the stitcher
//! patches into the instruction that uses it. An operation with no stencil form calls an
//! `UNSUPPORTED_*` symbol, which `build.rs` rejects, so a row cannot silently use one.

use crate::layout::{
    words::{HEAP_TAG, TAG_MASK},
    JS_STEP_FUNCTION_MARK_BLOCKS, JS_STEP_FUNCTION_MARK_BLOCK_COUNT, JS_STEP_OBJECT_MARK_BLOCKS, JS_STEP_OBJECT_MARK_BLOCK_COUNT, JS_STEP_PROPERTY_BUFFER, JS_STEP_SLOTS_TRACKED,
    LAYOUT_TABLE_BASE, NATIVE_OBJECT_LAYOUT as OBJECT, PROPERTY_BUFFER_BASE,
};
use crate::staged::{Passed, StagedRecordSpace, Stage, Table};
use crate::words::{hole, HOLE_FOURTH, HOLE_THIRD, LAST_HOLE};
use crate::typed_ir::Type;
use crate::staged::region::{Access, Region};

unsafe extern "C" {
    fn UNSUPPORTED_assumption(a: u64) -> u64;
    fn UNSUPPORTED_number_operator(a: u64, b: u64, c: u64) -> u64;
    fn UNSUPPORTED_shape_in(a: u64) -> u64;
    fn UNSUPPORTED_site(a: u64) -> u64;
}

/// The stencil stage; `sites` is the number of the next value hole a `site` call names,
/// `values` the property arena's value base a row that reads object storage was given (else the
/// arena is found through `payload`), and `payload` the activation's dispatch payload
/// (`JsStepDispatch`), which the process tables (`table`) and the barrier words (`store_gate`)
/// are read through; zero for a row that addresses none.
#[derive(Clone, Copy)]
pub(crate) struct StencilStage {
    sites: u8,
    values: u64,
    payload: u64,
}

impl StencilStage {
    /// A stage whose first `site` is hole `first`.
    #[inline(always)]
    pub(crate) fn from_hole(first: u8) -> Self {
        Self { sites: first, values: 0, payload: 0 }
    }

    /// As `from_hole`, for a row whose operations address a process table through `payload`.
    #[inline(always)]
    pub(crate) fn with_payload(first: u8, payload: u64) -> Self {
        Self { sites: first, values: 0, payload }
    }

    /// As `from_hole`, with the property arena's value base (`Table::PropertyValues`).
    #[inline(always)]
    pub(crate) fn with_values(first: u8, values: u64) -> Self {
        Self { sites: first, values, payload: 0 }
    }

    /// As `with_values`, for a row that stores through the dispatch payload at `payload`.
    #[inline(always)]
    pub(crate) fn storing(first: u8, values: u64, payload: usize) -> Self {
        Self { sites: first, values, payload: payload as u64 }
    }

    /// The `T` at `base + offset`, which need not be aligned (a typed array's bytes).
    #[inline(always)]
    fn read<T>(&self, base: u64, offset: i32) -> T {
        // SAFETY: a staged operation loads only from a live record, backing or buffer at a layout offset.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *const T).read_unaligned() }
    }

    /// Writes `value` at `base + offset`, which need not be aligned.
    #[inline(always)]
    fn write<T>(&self, base: u64, offset: i32, value: T) {
        // SAFETY: as `read`; a store writes only a live backing or buffer.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *mut T).write_unaligned(value) }
    }

    /// The runtime's `Concrete::new`, which the shared rows' interpreter wrappers call; those
    /// wrappers are never used here.
    #[inline(always)]
    pub(crate) fn new(_property_values: *const crate::Value) -> Self {
        Self { sites: 0, values: 0, payload: 0 }
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
            match n {
                0 => hole::<0>() as u64,
                1 => hole::<1>() as u64,
                HOLE_THIRD => hole::<HOLE_THIRD>() as u64,
                HOLE_FOURTH => hole::<HOLE_FOURTH>() as u64,
                LAST_HOLE => hole::<LAST_HOLE>() as u64,
                _ => UNSUPPORTED_site(n as u64),
            }
        }
    }
    #[inline(always)]
    fn load_site(&mut self, _at: Access, base: u64, offset: u64) -> u64 {
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
    fn load_u32(&mut self, _at: Access, base: u64, offset: i32) -> u64 {
        // SAFETY: a staged operation loads only from a live record or table at a layout offset.
        u64::from(unsafe { (base.wrapping_add(offset as i64 as u64) as *const u32).read() })
    }
    #[inline(always)]
    fn load_u64(&mut self, _at: Access, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *const u64).read() }
    }
    #[inline(always)]
    fn load_u8(&mut self, _at: Access, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`.
        u64::from(unsafe { (base.wrapping_add(offset as i64 as u64) as *const u8).read() })
    }
    #[inline(always)]
    fn load_u16(&mut self, _at: Access, base: u64, offset: i32) -> u64 {
        // SAFETY: as `load_u32`.
        u64::from(unsafe { (base.wrapping_add(offset as i64 as u64) as *const u16).read() })
    }
    #[inline(always)]
    fn load_table(&mut self, at: Access, address: u64) -> u64 { self.load_u64(at, address, 0) }
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
    fn uextend32(&mut self, a: u64) -> u64 {
        // SAFETY: the operation's contract is a 32-bit word (a width-checked index or length), which
        // the guard before it proved; LLVM drops the extension and any range check on the result.
        unsafe { core::hint::assert_unchecked(a <= u64::from(u32::MAX)) };
        a
    }
    #[inline(always)]
    fn sub(&mut self, a: u64, b: u64) -> u64 { a.wrapping_sub(b) }
    #[inline(always)]
    fn select(&mut self, cond: bool, then: u64, otherwise: u64) -> u64 { if cond { then } else { otherwise } }
    #[inline(always)]
    fn store_u32(&mut self, _at: Access, base: u64, offset: i32, value: u64) {
        // SAFETY: a staged operation stores only into a live record or backing at a layout offset.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *mut u32).write(value as u32) }
    }
    #[inline(always)]
    fn store_u64(&mut self, _at: Access, base: u64, offset: i32, value: u64) {
        // SAFETY: as `store_u32`.
        unsafe { (base.wrapping_add(offset as i64 as u64) as *mut u64).write(value) }
    }
    #[inline(always)]
    fn load_i8(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<i8>(base, offset) as i32 as u32) }
    #[inline(always)]
    fn load_i16(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<i16>(base, offset) as i32 as u32) }
    #[inline(always)]
    fn load_i32(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<u32>(base, offset)) }
    #[inline(always)]
    fn load_f32(&mut self, _at: Access, base: u64, offset: i32) -> u64 { u64::from(self.read::<u32>(base, offset)) }
    #[inline(always)]
    fn load_f64(&mut self, _at: Access, base: u64, offset: i32) -> u64 { self.read::<u64>(base, offset) }
    #[inline(always)]
    fn box_number(&mut self, number: u64) -> u64 {
        // `Value::number`: an integral double in int32 range (not `-0`) is an int32, NaN the
        // canonical NaN, any other double its own bits.
        let double = f64::from_bits(number);
        let integer = double as i32;
        if f64::from(integer) == double && (integer != 0 || number >> 63 == 0) {
            crate::layout::words::INT_TAG | u64::from(integer as u32)
        } else if double.is_nan() {
            crate::layout::words::CANONICAL_NAN
        } else {
            number
        }
    }
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
    fn clamp_u8(&mut self, a: u64) -> u64 { crate::units::clamp_u8(a) }
    #[inline(always)]
    fn store_gate(&mut self, space: StagedRecordSpace, record_id: u64, value: u64) -> bool {
        let word = |offset: usize| {
            // SAFETY: a payload word at a layout offset; a row that stores was given the live payload.
            unsafe { ((self.payload as usize + offset) as *const u64).read() }
        };
        if word(JS_STEP_SLOTS_TRACKED) != 0 {
            return false;
        }
        // A value that names no heap record joins no barrier log (`StagedRecordSpace::store_logs`).
        if value & TAG_MASK != HEAP_TAG {
            return true;
        }
        let (blocks_at, count_at) = match space {
            StagedRecordSpace::Object => (JS_STEP_OBJECT_MARK_BLOCKS, JS_STEP_OBJECT_MARK_BLOCK_COUNT),
            StagedRecordSpace::Function => (JS_STEP_FUNCTION_MARK_BLOCKS, JS_STEP_FUNCTION_MARK_BLOCK_COUNT),
        };
        let block = record_id >> OBJECT.mark_records_per_block.trailing_zeros();
        if block >= word(count_at) {
            return false;
        }
        let within = record_id & (OBJECT.mark_records_per_block as u64 - 1);
        let words = within / OBJECT.mark_word_bits as u64 * core::mem::size_of::<u64>() as u64;
        let marks = word(blocks_at) + block * OBJECT.mark_block_size as u64 + words + OBJECT.mark_offset as u64;
        // SAFETY: the record's block lies below the block count, and its marks are a word of that block.
        let marked = unsafe { (marks as *const u64).read() } >> (within % OBJECT.mark_word_bits as u64) & 1;
        marked == 0
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
        crate::vm::operations::number_is_truthy(f64::from_bits(bits))
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
    /// The branch `cond` takes, its leaving kept as its `None`: the default's `Cell` flag
    /// leaves a closure the row cannot inline (a call, which a stencil may not make).
    #[inline(always)]
    fn choose_leaving(&mut self, cond: bool, then: impl FnOnce(&mut Self) -> Option<u64>,
        otherwise: impl FnOnce(&mut Self) -> Option<u64>) -> Option<u64> {
        if cond { then(self) } else { otherwise(self) }
    }
    #[inline(always)]
    fn guard(&mut self, cond: bool, _miss: ()) -> Option<()> {
        if cond {
            return Some(());
        }
        // A failed guard leaves through an exit hole: the hot path stays straight.
        core::hint::cold_path();
        None
    }
    #[inline(always)]
    fn known(&self, _cond: bool) -> Option<bool> { None }
    #[inline(always)]
    fn table(&mut self, table: Table) -> u64 {
        match table {
            // The arena's header is the allocator's own, whose base native code reads each time.
            Table::PropertyValues if self.values != 0 => self.values,
            Table::PropertyValues => {
                let header = self.load_u64(Region::Dispatch.mutable(), self.payload, JS_STEP_PROPERTY_BUFFER as i32);
                self.load_u64(Region::HeapMeta.mutable(), header, PROPERTY_BUFFER_BASE as i32)
            }
            // A static of the runtime at a fixed address: no field to load.
            Table::Layouts => LAYOUT_TABLE_BASE,
        }
    }
    #[inline(always)]
    fn walk_prototypes(&mut self, records: u64, object: u64, target: u64, max_depth: u32) -> (bool, bool) {
        crate::staged::object::walk_prototypes_loop(self, records, object, target, max_depth)
    }
    /// `fill_by_rows` with volatile stores: the window's prefix, the arguments it holds room for,
    /// then `undefined`. The stores are volatile so LLVM cannot make the loop a `memcpy` or `memset`
    /// call, which a stencil may not make. The window's `registers` words lie at the lane stack's
    /// top, which admission proved free.
    #[inline(always)]
    fn fill_window(&mut self, top: u64, undefined: u64, prefix: &[u64], passed: Passed<'_, u64>, registers: u16) {
        let store = |index: usize, value: u64| {
            // SAFETY: as above; `index` is below `registers`.
            unsafe { (top.wrapping_add((index * 8) as u64) as *mut u64).write_volatile(value) }
        };
        let mut next = 0_usize;
        for bits in prefix.iter().take(usize::from(registers)) {
            store(next, *bits);
            next += 1;
        }
        for index in 0..passed.count().min(usize::from(registers) - next) {
            let value = match passed {
                Passed::Window { arguments, .. } => self.load_u64(Region::Frame.mutable(), arguments, (index * 8) as i32),
                Passed::List { file, registers: indices } => self.load_u64(Region::Frame.mutable(), file, i32::from(indices[index]) * 8),
            };
            store(next, value);
            next += 1;
        }
        for register in next..usize::from(registers) {
            store(register, undefined);
        }
    }
}

const _: fn(Access, Type) = |_, _| {};
