//! The builtin rows: a call of a native the b-jit knows by identity, run on its staged row (the
//! rows the interpreter and the u-jit run: `staged::builtins`, `staged::strings`, `staged::math`).
//! One row per builtin, `builtin_<symbol>` (`quench_bjit::call::BuiltinRow`).
//!
//! Contract. A row is entered and left like every row (`rows.rs`): frame in r12, context in r13.
//! Value hole 0 is the call's `CallRecord` address. The record's `callee` is the identity word the
//! callee register (`callee_register`) must hold; `this_register` is the receiver and
//! `args_register` the first argument of the window, the next arguments following it;
//! `result_register` receives the result. Spills made the registers canonical before the row, so
//! the row reads the frame slots.
//!
//! Successors: TAKEN after the result is stored to the result register; NEXT, when the row misses
//! (the guard, or an operand the staged row leaves at), with nothing written: the planner lays the
//! helper row there, which makes the call. No row leaves through an exit or makes a call.

use crate::heap_rows::{payload, word_at};
use crate::staged::Stage;
use crate::layout::call_record::{CallRecord, REGISTER_BYTES};
use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
use crate::layout::{
    JS_STEP_ARMED_ARRAY_PROOFS, JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS, JS_STEP_ARRAY_RECORD_BASE,
    JS_STEP_ASCII_UNIT_STRINGS, JS_STEP_STRING_RECORD_BASE,
};
use crate::stage::StencilStage;
use crate::staged::region::Region;
use crate::staged::{Concrete, builtins, call_native, elements, object, strings};
use crate::words::{S, hole64};
use crate::{Row, Value};
use core::mem::{offset_of, transmute};

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_TAKEN: u8;
}

/// The value hole of the call record's address.
const HOLE_RECORD: u8 = 0;

macro_rules! go {
    ($hole:ident, $s:expr) => {{
        let s: S = $s;
        let next: Row = transmute(&raw const $hole);
        become next(s.frame, s.ctx, s.p[0], s.p[1], s.p[2], s.p[3], s.f[0], s.f[1], s.f[2], s.f[3], s.f[4], s.f[5], s.f[6], s.f[7])
    }};
}

/// The state a successor receives where nothing is live: the frame and context, pinned registers zero.
#[inline(always)]
fn dead(s: &S) -> S {
    S { frame: s.frame, ctx: s.ctx, p: Default::default(), f: Default::default() }
}

/// The word of the frame register `register`.
#[inline(always)]
unsafe fn frame_word(s: &S, register: usize) -> u64 {
    // SAFETY: a register of the live frame, canonical after the spills before the row.
    unsafe { ((s.frame + register * REGISTER_BYTES) as *const u64).read() }
}

/// The register index the record names at `field`.
#[inline(always)]
unsafe fn record_register(record: usize, field: usize) -> usize {
    // SAFETY: the record is the chain's, live for the code's life.
    unsafe { usize::from(((record + field) as *const u16).read()) }
}

/// Whether the callee register holds the identity the record names: the guard a u-jit call makes.
#[inline(always)]
unsafe fn is_callee(s: &S, record: usize) -> bool {
    // SAFETY: as `record_register`.
    unsafe { frame_word(s, record_register(record, offset_of!(CallRecord, callee_register))) == word_at(record, offset_of!(CallRecord, callee)) }
}

/// The receiver's word.
#[inline(always)]
unsafe fn receiver(s: &S, record: usize) -> u64 {
    // SAFETY: as `record_register`.
    unsafe { frame_word(s, record_register(record, offset_of!(CallRecord, this_register))) }
}

/// The word of argument `index` of the call's window.
#[inline(always)]
unsafe fn argument(s: &S, record: usize, index: usize) -> u64 {
    // SAFETY: as `record_register`; the call passes the arguments the row reads (`BuiltinRow::arguments`).
    unsafe { frame_word(s, record_register(record, offset_of!(CallRecord, args_register)) + index) }
}

/// Writes the result word to the call's result register.
#[inline(always)]
unsafe fn store_result(s: &S, record: usize, word: u64) {
    // SAFETY: as `record_register`, a register of the live frame.
    unsafe { ((s.frame + record_register(record, offset_of!(CallRecord, result_register)) * REGISTER_BYTES) as *mut u64).write(word) }
}

/// `charCodeAt` of the string receiver at the index argument (`strings::string_char_code_at`).
#[inline(always)]
unsafe fn char_code_at(s: &S, record: usize) -> Option<u64> {
    // SAFETY: as `is_callee`, `payload`'s and the receiver's reads.
    unsafe {
        if !is_callee(s, record) {
            return None;
        }
        let records = word_at(payload(s), JS_STEP_STRING_RECORD_BASE);
        let mut stage = StencilStage::from_hole(0);
        let string = strings::flat_string(&mut stage, records, receiver(s, record), ())?;
        strings::step_string_native_of(&mut stage, strings::StringNative::CharCodeAt { string, index: argument(s, record, 0) }, ())
    }
}

/// `charAt` of the string receiver at the index argument: the one-unit string from the table of those
/// strings (`strings::string_char_at`); a miss leaves the call to the helper row.
#[inline(always)]
unsafe fn char_at(s: &S, record: usize) -> Option<u64> {
    // SAFETY: as `char_code_at`, with the dispatch payload's table word.
    unsafe {
        if !is_callee(s, record) {
            return None;
        }
        let records = word_at(payload(s), JS_STEP_STRING_RECORD_BASE);
        let table = word_at(payload(s), JS_STEP_ASCII_UNIT_STRINGS);
        let mut stage = StencilStage::from_hole(0);
        let string = strings::flat_string(&mut stage, records, receiver(s, record), ())?;
        strings::step_string_native_of(&mut stage, strings::StringNative::CharAt { string, index: argument(s, record, 0), table }, ())
    }
}

/// `Number.isInteger` of the argument: the interpreter's own step (`call_native::step_number_native_of`).
/// A non-Number misses.
#[inline(always)]
unsafe fn is_integer(s: &S, record: usize) -> Option<u64> {
    // SAFETY: as `is_callee`.
    unsafe {
        if !is_callee(s, record) {
            return None;
        }
        call_native::step_number_native_of(&mut Concrete::from_hole(0), call_native::NumberNative::IsInteger, &[argument(s, record, 0)], ())
    }
}

/// `Math.imul` of the two arguments: the interpreter's own step (`call_native::step_number_native_of`).
/// An operand that is no Number misses.
#[inline(always)]
unsafe fn imul(s: &S, record: usize) -> Option<u64> {
    // SAFETY: as `is_callee`.
    unsafe {
        if !is_callee(s, record) {
            return None;
        }
        let args = [argument(s, record, 0), argument(s, record, 1)];
        call_native::step_number_native_of(&mut Concrete::from_hole(0), call_native::NumberNative::Math(call_native::MathCall::Imul), &args, ())
    }
}

/// Whether a lane run's array storage proofs are armed (`heap::array_proofs`). A row cannot retire
/// them, so a row that shrinks an array's backing misses while any is armed.
#[inline(always)]
unsafe fn proofs_armed(s: &S) -> bool {
    // SAFETY: the payload's word names the thread's armed-proofs word, live for the thread; a
    // nonzero word is the armed proofs' address.
    unsafe { word_at(word_at(payload(s), JS_STEP_ARMED_ARRAY_PROOFS) as usize, 0) != 0 }
}

/// `push(value)` of the dense array receiver (`builtins::array_push_one`): its room is the backing's
/// capacity word, and its prototype link must be the one the held index-free chain proof covers,
/// which the link word names (`JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS`) and which proves no prototype
/// holds an index. The value must hold no heap reference, so its store is unlogged. The new length.
#[inline(always)]
unsafe fn array_push(s: &S, record: usize) -> Option<u64> {
    // SAFETY: as `is_callee`, `payload`'s and the receiver's reads; the header is the backing's,
    // live with its record; the payload's address names the thread's link word, live for the thread.
    unsafe {
        if !is_callee(s, record) {
            return None;
        }
        let records = word_at(payload(s), JS_STEP_ARRAY_RECORD_BASE);
        let mut stage = StencilStage::from_hole(0);
        let backing = elements::array_backing(&mut stage, records, receiver(s, record), ())?;
        let object = stage.add_imm(backing.record, ARRAY.object_offset as u64);
        let chain = word_at(word_at(payload(s), JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS) as usize, 0);
        if chain == 0 || object::proto_link(&mut stage, object) != chain {
            return None;
        }
        // The room is the capacity word under its mask, read as `elements::append_in_room` reads it.
        let word = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_capacity_offset as i32);
        let room = stage.and_imm(word, u64::from(ARRAY.capacity_mask));
        let value = argument(s, record, 0);
        builtins::unreferenced(&mut stage, value, ())?;
        builtins::step_array_native_of(&mut stage, builtins::ArrayNative::Push { room, value }, backing, ())
    }
}

/// `pop()` of the dense array receiver (`builtins::array_pop`). A shrink retires the armed array
/// proofs, which a row cannot, so it misses while any are armed. The removed element.
#[inline(always)]
unsafe fn array_pop(s: &S, record: usize) -> Option<u64> {
    // SAFETY: as `is_callee`, `payload`'s and the receiver's reads.
    unsafe {
        if !is_callee(s, record) || proofs_armed(s) {
            return None;
        }
        let records = word_at(payload(s), JS_STEP_ARRAY_RECORD_BASE);
        let mut stage = StencilStage::from_hole(0);
        let backing = elements::array_backing(&mut stage, records, receiver(s, record), ())?;
        builtins::step_array_native_of(&mut stage, builtins::ArrayNative::Pop, backing, ())
    }
}

/// One builtin row: its staged body's word is stored and the row continues at TAKEN, or the row
/// continues at NEXT with nothing written.
macro_rules! builtin_row {
    ($name:ident, |$s:ident, $record:ident| $body:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                let $record = hole64::<HOLE_RECORD>() as usize;
                let $s = &s;
                match $body {
                    Some(word) => {
                        store_result($s, $record, word);
                        go!(HOLE_TAKEN, dead($s))
                    }
                    None => {
                        core::hint::cold_path();
                        go!(HOLE_NEXT, dead($s))
                    }
                }
            }
        }
    };
}

builtin_row!(builtin_char_code_at, |s, record| char_code_at(s, record));
builtin_row!(builtin_char_at, |s, record| char_at(s, record));
builtin_row!(builtin_is_integer, |s, record| is_integer(s, record));
builtin_row!(builtin_imul, |s, record| imul(s, record));
builtin_row!(builtin_push, |s, record| array_push(s, record));
builtin_row!(builtin_pop, |s, record| array_pop(s, record));
