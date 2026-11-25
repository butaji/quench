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
use crate::layout::call_record::CallRecord;
use crate::layout::{
    JS_STEP_ARMED_ARRAY_PROOFS, JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS, JS_STEP_ARRAY_RECORD_BASE,
    JS_STEP_ASCII_UNIT_STRINGS, JS_STEP_STRING_RECORD_BASE,
};
use crate::stage::StencilStage;
use crate::staged::call_native;
use crate::staged::native_row::{self, NativeRow};
use crate::words::{S, hole64};
use crate::Row;
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

/// The register index the record names at `field`.
#[inline(always)]
unsafe fn record_register(record: usize, field: usize) -> u16 {
    // SAFETY: the record is the chain's, live for the code's life.
    unsafe { ((record + field) as *const u16).read() }
}

/// Where a row's words live in a stencil: the dispatch payload's records and proofs.
struct PayloadEnv<'a> {
    s: &'a S,
}

impl native_row::RowEnv<StencilStage> for PayloadEnv<'_> {
    // SAFETY (each): the dispatch payload's words name the thread's records and tables, live for the thread.
    #[inline(always)]
    fn string_records(&mut self, _stage: &mut StencilStage) -> u64 {
        unsafe { word_at(payload(self.s), JS_STEP_STRING_RECORD_BASE) }
    }
    #[inline(always)]
    fn ascii_unit_strings(&mut self, _stage: &mut StencilStage) -> u64 {
        unsafe { word_at(payload(self.s), JS_STEP_ASCII_UNIT_STRINGS) }
    }
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut StencilStage) -> u64 {
        unsafe { word_at(payload(self.s), JS_STEP_ARRAY_RECORD_BASE) }
    }
    #[inline(always)]
    fn index_free_chain(&mut self, _stage: &mut StencilStage) -> u64 {
        unsafe { word_at(word_at(payload(self.s), JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS) as usize, 0) }
    }
    #[inline(always)]
    fn armed_array_proofs(&mut self, _stage: &mut StencilStage) -> u64 {
        unsafe { word_at(word_at(payload(self.s), JS_STEP_ARMED_ARRAY_PROOFS) as usize, 0) }
    }
}

/// A row's body: the shared row (`native_row::step_native_row`) over the payload's words.
struct BuiltinBody<'a> {
    s: &'a S,
    builtin: NativeRow,
}

impl call_native::InPlaceBody<StencilStage> for BuiltinBody<'_> {
    #[inline(always)]
    fn in_place_value(&mut self, stage: &mut StencilStage, receiver: u64, arguments: u64, miss: ()) -> Option<u64> {
        native_row::step_native_row(stage, &mut PayloadEnv { s: self.s }, self.builtin, receiver, arguments, miss)
    }
}

/// The builtin `builtin`'s call at the record: the shared in-place call step
/// (`call_native::step_in_place_call_of`, the interpreter's `lane_call_in_place`) over the frame,
/// admitted against the identity the record holds, its value stored to the result register.
#[inline(always)]
unsafe fn builtin_call(s: &S, record: usize, builtin: NativeRow) -> Option<u64> {
    // SAFETY: the record is the chain's; its registers are the live frame's, canonical after the
    // spills before the row; the call passes the arguments the row reads (`BuiltinRow::arguments`).
    unsafe {
        let call = call_native::InPlaceCall {
            callee: record_register(record, offset_of!(CallRecord, callee_register)),
            receiver: record_register(record, offset_of!(CallRecord, this_register)),
            arguments: record_register(record, offset_of!(CallRecord, args_register)),
            result: record_register(record, offset_of!(CallRecord, result_register)),
        };
        let identity = word_at(record, offset_of!(CallRecord, callee));
        let mut stage = StencilStage::from_hole(0);
        call_native::step_in_place_call_of(&mut stage, &mut BuiltinBody { s, builtin }, s.frame as u64, identity, call, ())
    }
}

/// One builtin row: its call's step stored its value and the row continues at TAKEN, or the row
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
                    Some(_) => go!(HOLE_TAKEN, dead($s)),
                    None => {
                        core::hint::cold_path();
                        go!(HOLE_NEXT, dead($s))
                    }
                }
            }
        }
    };
}

/// One stencil per row of the table (`native_row_table::with_native_rows`).
macro_rules! builtin_stencils {
    ($( $(#[$doc:meta])* $row:ident = $symbol:literal, $stencil:ident, $arguments:literal; )*) => {
        $( builtin_row!($stencil, |s, record| builtin_call(s, record, NativeRow::$row)); )*
    };
}
crate::staged::native_row_table::with_native_rows!(builtin_stencils);
