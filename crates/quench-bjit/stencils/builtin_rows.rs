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
use crate::layout::call_record::CallRecord;
use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
use crate::layout::{
    JS_STEP_ARMED_ARRAY_PROOFS, JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS, JS_STEP_ARRAY_RECORD_BASE,
    JS_STEP_ASCII_UNIT_STRINGS, JS_STEP_STRING_RECORD_BASE,
};
use crate::stage::StencilStage;
use crate::staged::region::Region;
use crate::staged::{builtins, call_native, elements, object, strings};
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

/// The builtin a row runs: its in-place body (`call_native::InPlaceBody`) over the operands the
/// shared step reads, with the dispatch payload's words its natives need.
#[derive(Clone, Copy)]
enum Builtin {
    CharCodeAt,
    CharAt,
    IsInteger,
    Imul,
    Push,
    Pop,
    IsArray,
}

/// A row's body: which builtin, over the row's state (for the dispatch payload's words).
struct BuiltinBody<'a> {
    s: &'a S,
    builtin: Builtin,
}

impl call_native::InPlaceBody<StencilStage> for BuiltinBody<'_> {
    /// The builtin's value for `receiver` and the argument window at `arguments`: its operands
    /// prepared from the dispatch payload's records (a flat string, a dense array's backing), then
    /// the interpreter's own native step. `charCodeAt` and `charAt` read a flat string's unit
    /// (`strings::step_string_native_of`); `Number.isInteger` and `Math.imul` are
    /// `call_native::step_number_native_of`; `push` and `pop` are `builtins::step_array_native_of`.
    /// A `push` holds the index-free prototype chain proof (`JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS`)
    /// and stores only a value no write barrier logs; a `pop` misses while array storage proofs are
    /// armed (`JS_STEP_ARMED_ARRAY_PROOFS`), which a row cannot retire.
    #[inline(always)]
    fn in_place_value(&mut self, stage: &mut StencilStage, receiver: u64, arguments: u64, miss: ()) -> Option<u64> {
        let argument = |stage: &mut StencilStage, index: u16| stage.load_u64(Region::Frame.mutable(), arguments, crate::staged::word_offset(index));
        // SAFETY: the dispatch payload's words name the thread's records and tables, live for the
        // thread; the backing's header is live with its record.
        unsafe {
            let payload = payload(self.s);
            match self.builtin {
                Builtin::CharCodeAt => {
                    let string = strings::flat_string(stage, word_at(payload, JS_STEP_STRING_RECORD_BASE), receiver, miss)?;
                    let index = argument(stage, 0);
                    strings::step_string_native_of(stage, strings::StringNative::CharCodeAt { string, index }, miss)
                }
                Builtin::CharAt => {
                    let string = strings::flat_string(stage, word_at(payload, JS_STEP_STRING_RECORD_BASE), receiver, miss)?;
                    let (index, table) = (argument(stage, 0), word_at(payload, JS_STEP_ASCII_UNIT_STRINGS));
                    strings::step_string_native_of(stage, strings::StringNative::CharAt { string, index, table }, miss)
                }
                Builtin::IsInteger => {
                    let value = argument(stage, 0);
                    call_native::step_number_native_of(stage, call_native::NumberNative::IsInteger, &[value], miss)
                }
                Builtin::Imul => {
                    let args = [argument(stage, 0), argument(stage, 1)];
                    call_native::step_number_native_of(stage, call_native::NumberNative::Math(call_native::MathCall::Imul), &args, miss)
                }
                Builtin::Push => {
                    let backing = elements::array_backing(stage, word_at(payload, JS_STEP_ARRAY_RECORD_BASE), receiver, miss)?;
                    let object = stage.add_imm(backing.record, ARRAY.object_offset as u64);
                    let chain = word_at(word_at(payload, JS_STEP_ARRAY_CHAIN_INDEX_FREE_ADDRESS) as usize, 0);
                    if chain == 0 || object::proto_link(stage, object) != chain {
                        return None;
                    }
                    // The room is the capacity word under its mask, read as `elements::append_in_room` reads it.
                    let word = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_capacity_offset as i32);
                    let room = stage.and_imm(word, u64::from(ARRAY.capacity_mask));
                    let value = argument(stage, 0);
                    builtins::unreferenced(stage, value, miss)?;
                    builtins::step_array_native_of(stage, builtins::ArrayNative::Push { room, value }, backing, miss)
                }
                Builtin::Pop => {
                    if word_at(word_at(payload, JS_STEP_ARMED_ARRAY_PROOFS) as usize, 0) != 0 {
                        return None;
                    }
                    let backing = elements::array_backing(stage, word_at(payload, JS_STEP_ARRAY_RECORD_BASE), receiver, miss)?;
                    builtins::step_array_native_of(stage, builtins::ArrayNative::Pop, backing, miss)
                }
                Builtin::IsArray => builtins::is_plain_array(stage, word_at(payload, JS_STEP_ARRAY_RECORD_BASE), argument(stage, 0), miss),
            }
        }
    }
}

/// The builtin `builtin`'s call at the record: the shared in-place call step
/// (`call_native::step_in_place_call_of`, the interpreter's `lane_call_in_place`) over the frame,
/// admitted against the identity the record holds, its value stored to the result register.
#[inline(always)]
unsafe fn builtin_call(s: &S, record: usize, builtin: Builtin) -> Option<u64> {
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

builtin_row!(builtin_char_code_at, |s, record| builtin_call(s, record, Builtin::CharCodeAt));
builtin_row!(builtin_char_at, |s, record| builtin_call(s, record, Builtin::CharAt));
builtin_row!(builtin_is_integer, |s, record| builtin_call(s, record, Builtin::IsInteger));
builtin_row!(builtin_imul, |s, record| builtin_call(s, record, Builtin::Imul));
builtin_row!(builtin_push, |s, record| builtin_call(s, record, Builtin::Push));
builtin_row!(builtin_pop, |s, record| builtin_call(s, record, Builtin::Pop));
builtin_row!(builtin_is_array, |s, record| builtin_call(s, record, Builtin::IsArray));
