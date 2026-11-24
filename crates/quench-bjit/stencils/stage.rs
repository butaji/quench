//! The stage the b-jit rows run on: machine words computed in registers, conditions as `bool`,
//! a failed guard as `None` (the row's caller leaves through its exit hole), and a per-site
//! operand (`site`) as the address of the next value hole `HOLE_V<n>`, which the stitcher
//! patches into the instruction that uses it. An operation with no stencil form calls an
//! `UNSUPPORTED_*` symbol, which `build.rs` rejects, so a row cannot silently use one.

use crate::layout::{
    words::{HEAP_TAG, TAG_MASK},
    JS_STEP_FUNCTION_MARK_BLOCKS, JS_STEP_FUNCTION_MARK_BLOCK_COUNT, JS_STEP_OBJECT_MARK_BLOCKS, JS_STEP_OBJECT_MARK_BLOCK_COUNT, JS_STEP_PROPERTY_BUFFER, JS_STEP_STORE_WATCH,
    JS_STEP_STORES_SPECIALIZED,
    LAYOUT_TABLE_BASE, PROPERTY_BUFFER_BASE,
};
use crate::staged::barrier::{unmarked, MarkList};
use crate::staged::{Passed, StagedRecordSpace, Stage, Table};
use crate::words::{hole, HOLE_FOURTH, HOLE_THIRD, LAST_HOLE};
use crate::typed_ir::Type;
use crate::staged::region::{Access, Region};

// Statics, not functions: an operation with no stencil form reads one, so its relocation fails the
// build as a call's would, but the path holds no call, which would keep LLVM from unrolling a loop
// over static data (an IC's ops) whose unrolled form never reaches it.
unsafe extern "C" {
    static UNSUPPORTED_assumption: u8;
    static UNSUPPORTED_number_operator: u8;
    static UNSUPPORTED_shape_in: u8;
    static UNSUPPORTED_site: u8;
}

/// The word an unsupported operation answers: a volatile read of its `UNSUPPORTED_*` static, which
/// no optimization folds away, so the relocation stays and fails the build.
macro_rules! unsupported {
    ($symbol:ident) => {
        // SAFETY: never run; the relocation against the static fails the build.
        u64::from(unsafe { core::ptr::read_volatile(&raw const $symbol) })
    };
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

impl StencilStage {
    /// The dispatch word that says the run stores specialized (`JS_STEP_STORES_SPECIALIZED`), read from `payload`;
    /// `slot_store::store_admitted` tests it.
    #[inline(always)]
    pub(crate) fn stores_specialized(&mut self, payload: u64) -> u64 {
        self.load_u64(Region::Dispatch.mutable(), payload, JS_STEP_STORES_SPECIALIZED as i32)
    }
}

/// The per-site operand of the row's stage: the address of its next value hole (`Stage::site`), which the
/// stitcher patches into the instruction that uses it.
#[inline(always)]
pub(crate) fn stage_site(stage: &mut StencilStage) -> u64 {
    stage.site(0)
}

crate::projection!(StencilStage => StencilStage);

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
                _ => unsupported!(UNSUPPORTED_site),
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
        let _ = shape;
        unsupported!(UNSUPPORTED_shape_in) != 0
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
    crate::staged::word_ops::word_stage_ops!();
    #[inline(always)]
    fn shr(&mut self, a: u64, b: u64) -> u64 { a.wrapping_shr(b as u32) }
    #[inline(always)]
    #[inline(always)]
    fn add_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a.wrapping_add(imm.into()) }
    #[inline(always)]
    fn mul_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a.wrapping_mul(imm.into()) }
    #[inline(always)]
    fn and_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a & imm.into() }
    #[inline(always)]
    fn or_imm(&mut self, a: u64, imm: impl Into<u64>) -> u64 { a | imm.into() }
    #[inline(always)]
    #[inline(always)]
    fn eq_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { a == imm.into() }
    #[inline(always)]
    fn ne_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { a != imm.into() }
    #[inline(always)]
    fn ugt_imm(&mut self, a: u64, imm: impl Into<u64>) -> bool { a > imm.into() }
    #[inline(always)]
    fn eq(&mut self, a: u64, b: u64) -> bool { a == b }
    /// A plain Rust loop: a stencil holds no call, and the body runs on the state until it says the loop ends.
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
    fn uextend32(&mut self, a: u64) -> u64 {
        // SAFETY: the operation's contract is a 32-bit word (a width-checked index or length), which
        // the guard before it proved; LLVM drops the extension and any range check on the result.
        unsafe { core::hint::assert_unchecked(a <= u64::from(u32::MAX)) };
        a
    }
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
    fn box_number(&mut self, number: crate::staged::Float64<u64>) -> u64 {
        // `Value::number` (`words::canonical`): its truncation is the target's own instruction, where
        // a saturating `as` cast would load its bounds from a constant pool a stencil may not use.
        crate::words::canonical(f64::from_bits(number.word()))
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
        if word(JS_STEP_STORE_WATCH) != 0 {
            return false;
        }
        // A value that names no heap record joins no barrier log (`StagedRecordSpace::store_logs`).
        if value & TAG_MASK != HEAP_TAG {
            return true;
        }
        let list = match space {
            StagedRecordSpace::Object => MarkList { blocks: JS_STEP_OBJECT_MARK_BLOCKS, count: JS_STEP_OBJECT_MARK_BLOCK_COUNT },
            StagedRecordSpace::Function => MarkList { blocks: JS_STEP_FUNCTION_MARK_BLOCKS, count: JS_STEP_FUNCTION_MARK_BLOCK_COUNT },
        };
        let payload = self.payload;
        unmarked(self, payload, list, record_id, ()).is_some()
    }
    #[inline(always)]
    fn immediate_store_open(&mut self) -> bool {
        // A row given no payload has no gate word to read; its immediate stores stay open, as they
        // were before counts were read (each counted store row is given the payload).
        if self.payload == 0 {
            return true;
        }
        // SAFETY: a payload word at a layout offset; a row that stores was given the live payload.
        let watch = unsafe { ((self.payload as usize + JS_STEP_STORE_WATCH) as *const u64).read() };
        watch & crate::layout::STORE_WATCH_COUNTS == 0
    }
    #[inline(always)]
    fn assumption(&mut self, watch: u64) -> bool {
        let _ = watch;
        unsupported!(UNSUPPORTED_assumption) != 0
    }
    #[inline(always)]
    fn number_operator(&mut self, operator: u64, passed: u64, constant: u64) -> (u64, bool) {
        let _ = (operator, passed, constant);
        (unsupported!(UNSUPPORTED_number_operator), false)
    }
    #[inline(always)]
    fn shape_in(&mut self, shape: u64, _shapes: &[u32]) -> bool {
        let _ = shape;
        unsupported!(UNSUPPORTED_shape_in) != 0
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
