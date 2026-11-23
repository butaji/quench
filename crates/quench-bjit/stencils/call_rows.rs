//! The call rows: a helper call and the direct call through a callee's entry cell, the reloads
//! after them, and the leave that passes a (status, word) through. Contracts, all of them:
//!
//! Registers and stack. A row is entered and left like every row (`rows.rs`): frame in r12,
//! context (`CallContext`) in r13, pinned ints r14, r15, rdi, rsi, pinned doubles xmm0..7, and
//! its successor is a `become` through a hole. A call row makes exactly one SysV call, through
//! an address it loads with `movabs` from a value hole into a register (`call reg`: the one
//! form `build.rs` allows beside `jmp`/`jcc`, in rows named `call_*` only). Across that call
//! r12 and r13 live in the C ABI's callee-saved registers and the stack is 16-byte aligned by
//! the compiler's own frame, which is popped before the `become`. **Every pinned register is
//! dead from the call on**: the planner spills the live ones to their frame slots before the
//! row (the `Spill` rows, as canonical tagged words) and reloads the ones live after it with
//! the `reload_*` rows, so the row passes zeros to its successors, and a successor that
//! leaves does not spill (the frame is already canonical).
//!
//! Helper row `call_helper`: holes V0 = the call record's pc (the fourth helper argument), V1 =
//! the helper's address (`body_call_helper`); vm, residual and lane context come from the
//! dispatch payload behind the context. Successors: COMPLETED continues at NEXT; MISSED
//! (nothing ran) leaves at the call's pc through EXIT0 (a leave that does not spill); LEFT and
//! TERMINATED leave through EXIT1 with the (status, word) in r14, r15 (`leave_status`); BRANCHED
//! (a step that took its conditional jump) continues at TAKEN, the reloads of the registers live
//! at the jump's target.
//!
//! Semantic row `call_semantic`: holes V0 = the record's pc, V1 = the opcode's semantic entry, V2 = the
//! record's operands, V3 = the jump target of the record (`u64::MAX` where it has none). The entry
//! (`native_js_operation_entry`) runs the record on the activation's frame and writes its next PC to
//! a word of this row's frame. Status 0 continues at TAKEN where the next PC is the target, else NEXT;
//! any other status is a stop, which leaves through EXIT0 with (status, word) in r14, r15.
//!
//! Direct row `call_direct_a<N>_<strict|sloppy|lexical>[_unchecked][_window]`: hole V0 = the address of the site's
//! `CallRecord`. A checked row compares the callee word (the `_unchecked` row, whose callee a Stable fact
//! holds, does not read the callee register) and (sloppy) binds the receiver (an arrow, lexical, binds
//! none: its body finds its `this` in its environment, as `body_call_helper` passes it), admits the call with the
//! shared `admit_native` row (one validity compare, one room check) plus the plain depth bound,
//! writes `N` arguments and `undefined` for the rest into the callee's window at the lane
//! stack's top, pushes the body's plain header (depth bumped, its body in the body table), calls the cell's entry with the body ABI (context,
//! this, captures, window) and handles the returned (status, word) as the u-jit's direct call
//! does. The arguments come from the places the record names (`argument_places`): a leading
//! argument may be a pinned register's value (read from the entry state, its spill elided by the
//! planner when it is dead after the call), else the window slot. The `_window` row is the same row
//! for a call whose arguments are all in the window: it reads no place, so it has no place branches
//! (the planner names it when no leading argument is pinned; argc 0 has no such row). Successors: refusal before any effect, or a callee that was not entered (the stub),
//! goes to NEXT, which the planner makes the helper row; a completion stores the word to the
//! call's register and goes to TAKEN (the reloads); any other status leaves through EXIT0 with
//! (status, word) in r14, r15, the callee's body frames kept for the lane to materialize.
//!
//! Exit row `exit_here`: leaves through EXIT0 with every register as it came, so the exit stub
//! writes the live pinned registers back: the lowering of an instruction whose operation only
//! the interpreter runs.

use crate::layout::call_record::{helper_outcome, CallRecord, DirectHole, HelperHole, LaneStatus, Outcome, CALLEE_EXIT, PLACED_ARGUMENTS, REGISTER_BYTES};
use crate::codes::{DOUBLE_PINS, LAST_INT_PIN, MEM, PIN, PINF};
use crate::heap_rows::{context_word, payload, set_context_word, word_at};
use crate::layout::{CELL_BODY_AT, CELL_COUNTERS_AT, CELL_NUMBERS_AT, CELL_PROVEN_AT, CELL_ROWS_AT, CELL_ENTRY_AT, CELL_USES_THIS_AT, CELL_RECOVERY_AT, JS_STEP_ACTIVATION_COUNT, JS_STEP_BODY_TABLE, JS_STEP_NATIVE_TABLE, JS_STEP_POP_OBSERVED_HEADER, JS_STEP_LANE_CONTEXT, JS_STEP_LANE_VALUES_END, JS_STEP_RESIDUAL, JS_STEP_VM, PLAIN_DEPTH_LIMIT, CONTEXT_BODY, CONTEXT_CAPTURE, CONTEXT_ENV, CONTEXT_THIS, FRAME_CONTEXT_WORDS, RAISED_RUN, RECOVERY_ACTIVATIONS_AT, RECOVERY_VALUES_AT};
use crate::places::Places;
use crate::stage::StencilStage;
use crate::staged::frame::{CONTEXT_PACKED, NATIVE_FLAG, PLAIN_DEPTH_AT, PLAIN_REGISTERS_AT};
use crate::staged::region::Region;
use crate::staged::{call::admit_native, protocol, value, word_offset, CallFrame, CallOperation, Passed, Stage, Step};
use crate::words::{Out, S, dead, hole64};
use crate::{Row, Value};
use core::mem::{offset_of, transmute};

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_TAKEN: u8;
    static HOLE_EXIT0: u8;
    static HOLE_EXIT1: u8;
}

/// The last double register (xmm7, d7) a value may name: the double pins follow `PINF`.
const LAST_DOUBLE_PIN: u8 = PINF + DOUBLE_PINS - 1;
/// The semantic row's holes: the record's pc, the opcode's semantic entry, its operand record and its jump target.
const HOLE_SEMANTIC_PC: u8 = 0;
const HOLE_SEMANTIC: u8 = 1;
const HOLE_OPERANDS: u8 = 2;
const HOLE_TARGET: u8 = 3;
/// Shift from a word count to bytes.
const WORD_SHIFT: u32 = REGISTER_BYTES.trailing_zeros();

/// `leave_status`: the leave a (status, word) in r14, r15 ends in, as every leave routine ends
/// (`entry.rs`): the word in rax and the status in rdx, which `entry_body` swaps into the lane
/// body ABI's (eax, rdx) and `entry_thunk` reads the word of.
#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_status() {
    core::arch::naked_asm!("mov rax, r15", "mov edx, r14d", "ret")
}

/// AArch64 `leave_status`: the first two pinned ints (x22 status, x23 word, `entry_a64.rs`) into the
/// leave routines' (x0 word, x1 status).
#[cfg(target_arch = "aarch64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_status() {
    core::arch::naked_asm!("mov x0, x23", "mov w1, w22", "ret")
}

/// A lane body's return: its status and result word in (eax, rdx), as `BodyReturn`.
#[repr(C)]
struct BodyReturn {
    status: u32,
    value: u64,
}

/// A lane body's entry (`BodyEntry`): (context, receiver, capture word, registers).
type BodyEntry = unsafe extern "C" fn(usize, u64, u64, usize) -> BodyReturn;
/// A b-jit body's row entry (`entry_rows`, `EntryCell::rows`): (window, context, receiver, captures)
/// -> (word, status), in the row ABI, which saves no callee-saved register.
type RowsEntry = unsafe extern "rust-preserve-none" fn(usize, usize, u64, u64) -> RowsReturn;
#[repr(C)]
struct RowsReturn {
    value: u64,
    status: u64,
}
/// A semantic helper (`body_call_helper`): (vm, residual, lane context, call pc) -> status.
type Helper = unsafe extern "C" fn(u64, u64, u64, u64) -> u64;

/// The helper at hole V1 as a *branch*: Mach-O arm64 allows a branch into text but not an
/// absolute word there, so on Apple AArch64 the row calls the hole's symbol directly and the
/// image links the branch to the helper (`Reloc::Arm64Call`). Every other target loads the
/// address the hole holds and calls it.
#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
unsafe extern "C" {
    #[link_name = "HOLE_V1_BRANCH"]
    fn helper_branch(vm: u64, residual: u64, context: u64, call_pc: u64, first: u64, imm: u64) -> u64;
}
/// A semantic entry (`native_js_operation_entry`): (context, pc, operands, result word address) -> status.
type SemanticEntry = unsafe extern "C" fn(usize, u64, usize, usize) -> i64;

/// `dead` with a (status, word) for `leave_status` in the first two pinned ints.
#[inline(always)]
fn leaving(s: &S, status: u32, word: u64) -> S {
    S { p: [status as usize, word as usize, 0, 0], ..dead(s) }
}

/// The caller's own end by the exception a callee raised (`Vm::lane_raised`): the record that raised it is
/// in the payload for the lane to unwind from (`LANE_BODY_THREW`).
#[inline(always)]
unsafe fn raising(s: &S, payload: usize, pc: u64) -> S {
    protocol::record_raised_pc(&mut StencilStage::from_hole(0), payload as u64, pc);
    leaving(s, LaneStatus::Threw as u32, 0)
}

macro_rules! go {
    ($hole:ident, $s:expr) => {{
        let s: S = $s;
        let next: Row = transmute(&raw const $hole);
        become next(s.frame, s.ctx, s.p[0], s.p[1], s.p[2], s.p[3], s.f[0], s.f[1], s.f[2], s.f[3], s.f[4], s.f[5], s.f[6], s.f[7])
    }};
}

/// The register index the record names at `field`, read as the frame's accesses are (`Region::Frame`: a call
/// record lives on the lane stack).
#[inline(always)]
fn register_index(record: usize, field: usize) -> u16 {
    // The planner stored the index in the live record, so the load reads a register number.
    StencilStage::from_hole(0).load_u16(Region::Frame.mutable(), record as u64, field as i32) as u16
}

/// The word of the register the record names at `field`, in the caller's register file at `file`
/// (the call protocol's `CallFrame::call_registers`), read through the frame region.
#[inline(always)]
unsafe fn read_register(file: u64, record: usize, field: usize) -> u64 {
    // SAFETY: as `register_index`, a register of the live frame.
    StencilStage::from_hole(0).load_u64(Region::Frame.mutable(), file, word_offset(register_index(record, field)))
}

/// Writes `word` to the register the record names at `field` of the file at `file`.
#[inline(always)]
unsafe fn write_register(file: u64, record: usize, field: usize, word: u64) {
    // SAFETY: as `read_register`.
    StencilStage::from_hole(0).store_u64(Region::Frame.mutable(), file, word_offset(register_index(record, field)), word)
}

/// The word a call-protocol step produced; `None` where it left at its miss edge.
#[inline(always)]
fn word_of(step: Step<u64, ()>) -> Option<u64> {
    match step {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

/// A packed step helper (`lane_body_step_packed`): (vm, residual, lane context, call pc, first word, immediate) -> status.
type HelperPacked = unsafe extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64;

/// One helper row body, expanded for each entry (`PACKED`): the semantic helper at hole V1 runs the step at the
/// call pc (hole V0), which for `PACKED` also takes the instruction's words (holes V2, V3) so it runs without
/// decoding the record. The status the helper answers picks the successor. `call_helper` and `call_helper_packed`
/// are the two expansions, so the row's contract is written once.
macro_rules! helper_row {
    ($(#[$doc:meta])* $name:ident, $packed:expr) => {
        $(#[$doc])*
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            const PACKED: bool = $packed;
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                let payload = payload(&s);
                let (vm, residual, context) = (word_at(payload, JS_STEP_VM), word_at(payload, JS_STEP_RESIDUAL), word_at(payload, JS_STEP_LANE_CONTEXT));
                #[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
                let status = if PACKED {
                    helper_branch(vm, residual, context, hole64::<{ HelperHole::CallPc as u8 }>(), hole64::<{ HelperHole::PackedFirst as u8 }>(), hole64::<{ HelperHole::PackedImm as u8 }>())
                } else {
                    // The four-argument helper reads only its four: the packed words are unused.
                    helper_branch(vm, residual, context, hole64::<{ HelperHole::CallPc as u8 }>(), 0, 0)
                };
                #[cfg(not(all(target_arch = "aarch64", target_vendor = "apple")))]
                let status = if PACKED {
                    let helper: HelperPacked = transmute(hole64::<{ HelperHole::Helper as u8 }>() as usize);
                    helper(vm, residual, context, hole64::<{ HelperHole::CallPc as u8 }>(), hole64::<{ HelperHole::PackedFirst as u8 }>(), hole64::<{ HelperHole::PackedImm as u8 }>())
                } else {
                    let helper: Helper = transmute(hole64::<{ HelperHole::Helper as u8 }>() as usize);
                    helper(vm, residual, context, hole64::<{ HelperHole::CallPc as u8 }>())
                };
                match helper_outcome(status) {
                    Outcome::Next => go!(HOLE_NEXT, dead(&s)),
                    Outcome::Taken => go!(HOLE_TAKEN, dead(&s)),
                    Outcome::Missed => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, dead(&s))
                    }
                    Outcome::Left => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT1, leaving(&s, LaneStatus::NativeJsStopped as u32, CALLEE_EXIT))
                    }
                    Outcome::Raised => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT1, raising(&s, payload, hole64::<{ HelperHole::CallPc as u8 }>()))
                    }
                    Outcome::Terminated => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT1, leaving(&s, LaneStatus::Terminated as u32, 0))
                    }
                }
            }
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn call_semantic(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
    unsafe {
        let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
        let entry: SemanticEntry = transmute(hole64::<HOLE_SEMANTIC>() as usize);
        let mut word: u64 = 0;
        let status = entry(s.ctx, hole64::<HOLE_SEMANTIC_PC>(), hole64::<HOLE_OPERANDS>() as usize, &raw mut word as usize);
        if status != 0 {
            core::hint::cold_path();
            go!(HOLE_EXIT0, leaving(&s, status as u32, word));
        }
        if word == hole64::<HOLE_TARGET>() {
            go!(HOLE_TAKEN, dead(&s))
        } else {
            go!(HOLE_NEXT, dead(&s))
        }
    }
}

helper_row!(
    /// Helper row `call_helper`: holes V0 = the call record's pc (the fourth helper argument), V1 =
    /// the helper's address (`body_call_helper`); vm, residual and lane context come from the dispatch payload
    /// behind the context. Successors: COMPLETED continues at NEXT; MISSED (nothing ran) leaves at the call's
    /// pc through EXIT0 (a leave that does not spill); LEFT and TERMINATED leave through EXIT1 with the
    /// (status, word) in r14, r15 (`leave_status`); BRANCHED (a step that took its conditional jump) continues
    /// at TAKEN, the reloads of the registers live at the jump's target.
    call_helper, false);

helper_row!(
    /// Helper row `call_helper_packed`: `call_helper` with the instruction its planner decoded passed as holes
    /// V2 and V3, so the helper runs it without decoding the record again. Its successors and exits are `call_helper`'s.
    call_helper_packed, true);

/// How a direct call ended.
enum Entered {
    /// Refused before any effect, or the callee's cell held the stub: the helper takes the call.
    Refused,
    /// The callee completed; its word is stored in the call's register.
    Returned,
    /// The callee ended by an exception: the caller raises it at the call.
    Raised,
    /// The callee left: its (status, word), which the caller leaves with.
    Left(u32, u64),
}

/// The tagged words of the `ARGC` arguments a direct call passes, each from the place the planner named
/// for it (`CallRecord::argument_places`): a pinned int's or double's canonical word, else the caller's
/// window slot at `window`. Only the leading `PLACED_ARGUMENTS` may be pinned; the rest are read from the
/// window. A row without places (`PLACED` false, the `_window` row) reads none, so its places fold away.
#[inline(always)]
unsafe fn argument_words<const ARGC: usize, const PLACED: bool>(s: &S, file: u64, window: u16, record: usize) -> [u64; ARGC] {
    // SAFETY: the record's places are the planner's, for the live frame; the window's `ARGC` registers
    // lie in the caller's frame.
    unsafe {
        let mut stage = StencilStage::from_hole(0);
        let places = (record + offset_of!(CallRecord, argument_places)) as *const u8;
        let mut words = [0u64; ARGC];
        for (k, word) in words.iter_mut().enumerate() {
            let place = if PLACED && k < PLACED_ARGUMENTS { places.add(k).read() } else { MEM };
            *word = match place {
                PIN..=LAST_INT_PIN => value::int_word(&mut stage, s.p[usize::from(place - PIN)] as u32 as u64),
                PINF..=LAST_DOUBLE_PIN => stage.box_number(s.f[usize::from(place - PINF)].to_bits()),
                _ => stage.load_u64(Region::Frame.mutable(), file, word_offset(window + k as u16)),
            };
        }
        words
    }
}

/// What admission found: the callee's cell, the lane stack's values' top where its window goes, the
/// window's size, and the plain depth and innermost window a pop restores.
struct Admitted {
    cell: usize,
    top: usize,
    room: usize,
    /// The registers past the arguments the push starts `undefined` (`window_fill`): the rest are
    /// written before any read, as the interpreter's own call leaves them.
    fill: usize,
    /// The plain depth and innermost window before the push.
    depth: u64,
    registers: u64,
    /// The body table entry for the frame's body, at the total frame count (records + plain depth), and
    /// the native table entry for its descriptor at the same index (`JsStepHead::native_table`).
    table: usize,
    native: usize,
    /// The dispatch payload, whose VM and spilled-header helper a pop reads.
    payload: usize,
    frames: u64,
    /// The addresses of the lane's state words, read once at admission: every move writes through them.
    tops: protocol::LaneTops<u64>,
}

/// Pops the frame the call pushed: the plain depth, innermost window and body top it found, and the values' top.
/// Where Rust observed the frame, the activation pops where Rust keeps it (`LaneStack::pop_observed_header`), as the
/// u-jit's `pop_native` pops it: a nested entry to emitted code under the callee (`Vm::stack_words_in_lane`) made its
/// native header a record (`LaneStack::spill_plain`), leaving no plain header, or a helper linked it
/// (`native_frames::link_header`), which cleared its NATIVE flag and gave it a side record that pops with it.
#[inline(always)]
fn pop(a: &Admitted) {
    let mut stage = StencilStage::from_hole(0);
    if word_at(a.tops.plain_at as usize, PLAIN_DEPTH_AT) == 0 || context_word(a.top, CONTEXT_PACKED) & NATIVE_FLAG == 0 {
        core::hint::cold_path();
        // SAFETY: the payload's helper is `pop_observed_header_in_lane`, which takes the VM and residual words.
        unsafe {
            let helper: unsafe extern "C" fn(u64, u64) -> u64 = transmute(word_at(a.payload, JS_STEP_POP_OBSERVED_HEADER) as usize);
            helper(word_at(a.payload, JS_STEP_VM), word_at(a.payload, JS_STEP_RESIDUAL));
        }
    } else {
        protocol::set_plain(&mut stage, a.tops, a.depth, a.registers);
    }
    protocol::set_body_top(&mut stage, a.tops, a.frames);
    protocol::set_values(&mut stage, a.tops, a.top as u64);
}

/// The cell's validity and room (`admit_native`, less the callee's recovery space) and the plain depth bound.
#[inline(always)]
unsafe fn admit(stage: &mut StencilStage, record: usize, payload: usize) -> Option<Admitted> {
    // SAFETY: the record and cell are live for the code's life, the payload for the activation's.
    unsafe {
        let cell = word_at(record, offset_of!(CallRecord, cell)) as usize;
        let tops = protocol::lane_tops(stage, payload as u64);
        let (top, end) = (word_at(tops.values_at as usize, 0), word_at(payload, JS_STEP_LANE_VALUES_END));
        let recovery = word_at(cell, CELL_RECOVERY_AT) as usize;
        let reserved = word_at(recovery, RECOVERY_VALUES_AT) + FRAME_CONTEXT_WORDS as u64;
        let (room, fill) = admit_native(stage, cell as u64, word_at(record, offset_of!(CallRecord, expected)), top, end, reserved, ())?;
        let depth = word_at(tops.plain_at as usize, PLAIN_DEPTH_AT);
        let frames = word_at(recovery, RECOVERY_ACTIVATIONS_AT) + 1;
        // The payload names the limit's cell (`LaneStack::plain_depth_limit`), which Rust zeroes when it changes the frames
        // under a bound run or the run's window is a leaf's (`refuse_direct_calls_in_leaf`): the helper row calls then.
        // Comparing against the cell's address instead admitted every call, so a body entered as a leaf pushed its callee
        // over the leaf's pending activation (crypto's and deltablue's `--b-jit` failures).
        if depth + frames > word_at(word_at(payload, PLAIN_DEPTH_LIMIT) as usize, 0) {
            return None;
        }
        let registers = word_at(tops.plain_at as usize, PLAIN_REGISTERS_AT);
        let records = word_at(word_at(payload, JS_STEP_ACTIVATION_COUNT) as usize, 0);
        let index = ((records + depth) as usize) * REGISTER_BYTES;
        let table = word_at(payload, JS_STEP_BODY_TABLE) as usize + index;
        let native = word_at(payload, JS_STEP_NATIVE_TABLE) as usize + index;
        Some(Admitted { cell, top: top as usize, room: room as usize, fill: fill as usize, depth, registers, table, native, payload, tops, frames: records + depth })
    }
}

/// The callee's window, its plain header, its body in the body table, and the lane's new tops.
#[inline(always)]
unsafe fn push<const ARGC: usize, const PLACED: bool>(s: &S, file: u64, record: usize, window: u16, a: &Admitted, this: u64, captures: u64) {
    // SAFETY: as `admit`, which proved the room.
    unsafe {
        // The window's words past the arguments that are written: the cell's fill (at least the arguments).
        let written = if a.fill < ARGC { ARGC } else { a.fill };
        // The arguments are the call's words (a pinned place's value, else the caller's window), which the
        // protocol's fill writes as its prefix, then `undefined` to `written` registers.
        let passed = argument_words::<ARGC, PLACED>(s, file, window, record);
        protocol::fill_call(&mut StencilStage::from_hole(0), a.top as u64, &passed, Passed::List { file, registers: &[] }, written as u16);
        // The callee's receiver (only where its activation reads `this`: the cell's word says so, read
        // now, so a republish that changes it is seen by the next call) and captures word, in the gap its window leaves below register 0.
        if word_at(a.cell, CELL_USES_THIS_AT) != 0 {
            set_context_word(a.top, CONTEXT_THIS, this);
        }
        set_context_word(a.top, CONTEXT_BODY, captures);
        let field = |at: usize| word_at(record, at);
        // The callee's environment and captures pointer, which the interpreter reads from a linked frame.
        set_context_word(a.top, CONTEXT_ENV, field(offset_of!(CallRecord, env)));
        set_context_word(a.top, CONTEXT_CAPTURE, field(offset_of!(CallRecord, capture)));
        let header = protocol::BodyHeader { call: field(offset_of!(CallRecord, call)), caller_registers: file, caller_base: field(offset_of!(CallRecord, caller_base)),
            packed: field(offset_of!(CallRecord, packed)), callee: field(offset_of!(CallRecord, callee)) };
        let mut stage = StencilStage::from_hole(0);
        protocol::push_body_header(&mut stage, a.top as u64, header);
        // The running body, at the frame's depth in the body table; the depth and window it bumps.
        let body = word_at(a.cell, CELL_BODY_AT);
        stage.store_u64(Region::Frame.mutable(), a.table as u64, 0, body);
        // Its descriptor at the same index, which links the native header where Rust observes it
        // (`native_frames::link_header`), as the u-jit's `push_native` writes it.
        stage.store_u64(Region::Frame.mutable(), a.native as u64, 0, field(offset_of!(CallRecord, descriptor)));
        protocol::set_plain(&mut stage, a.tops, a.depth + 1, a.top as u64);
        protocol::set_body_top(&mut stage, a.tops, a.frames + 1);
        protocol::set_values(&mut stage, a.tops, (a.top + ((a.room + FRAME_CONTEXT_WORDS) << WORD_SHIFT)) as u64);
    }
}

/// The row entry a direct call enters: the callee's proven-arguments entry where every Number guard it skips (the cell's
/// `numbers`) is one the call's proven arguments (`CallRecord::proven_args`) cover, else its row entry; 0 where it has none.
#[inline(always)]
fn row_entry(cell: usize, record: usize) -> usize {
    let proven = word_at(cell, CELL_PROVEN_AT) as usize;
    let covered = word_at(cell, CELL_NUMBERS_AT) & !word_at(record, offset_of!(CallRecord, proven_args)) == 0;
    if covered && proven != 0 { proven } else { word_at(cell, CELL_ROWS_AT) as usize }
}

/// Where each status of the callee goes: a completion pops its window and frame and stores its word
/// to the call's register; a callee that was not entered pops them and is refused; one that ended by an
/// exception pops them, is counted in its cell and raises in the caller; any other status keeps the frames
/// (the lane links them) and is the caller's leave.
#[inline(always)]
unsafe fn finish(file: u64, record: usize, a: &Admitted, returned: BodyReturn) -> Entered {
    // SAFETY: as `push`.
    unsafe {
        // The protocol's completion: a returned callee's value, stored to the call's register.
        let completion = CallOperation::Complete { status: u64::from(returned.status), value: returned.value, receiver: None, returned: LaneStatus::Returned as u32 };
        if let Some(value) = word_of(protocol::step(&mut StencilStage::from_hole(0), (), completion)) {
            pop(a);
            write_register(file, record, offset_of!(CallRecord, result_register), value);
            return Entered::Returned;
        }
        match LaneStatus::of(returned.status) {
            Some(LaneStatus::NotEntered) => {
                pop(a);
                Entered::Refused
            }
            Some(LaneStatus::Threw) => {
                pop(a);
                let counters = word_at(a.cell, CELL_COUNTERS_AT);
                StencilStage::from_hole(0).store_u64(Region::EntryCell.mutable(), a.cell as u64, CELL_COUNTERS_AT as i32, counters.wrapping_add(RAISED_RUN));
                Entered::Raised
            }
            _ => Entered::Left(returned.status, returned.value),
        }
    }
}

/// How a direct row binds the receiver: as it is, sloppy (`protocol::bind_this`), or none (an arrow).
const STRICT: u8 = 0;
const SLOPPY: u8 = 1;
const LEXICAL: u8 = 2;

/// The direct call: the callee and receiver, admission, the push, the call, the pop. `CHECKED` compares the
/// callee register with the record's callee; the unchecked row's callee is a Stable fact's (its change retires the body).
/// `PLACED` reads the leading arguments' places; the `_window` row does not.
#[inline(always)]
unsafe fn enter<const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool>(s: &S) -> Entered {
    // SAFETY: as `admit`.
    unsafe {
        let record = hole64::<{ DirectHole::Record as u8 }>() as usize;
        // The window register the arguments start at, read once for the call.
        let window = register_index(record, offset_of!(CallRecord, args_register));
        let mut stage = StencilStage::from_hole(0);
        // The caller's register file: the call protocol's frame hook (`Places::call_registers`).
        let mut frame = Places::new::<0, 0, 0>(S { frame: s.frame, ctx: s.ctx, p: s.p, f: s.f });
        let Some(file) = CallFrame::call_registers(&mut frame, &mut stage) else { return Entered::Refused };
        // Identity (checked rows only), then the receiver the callee binds (strict as it is, sloppy by `protocol::bind_this`).
        if CHECKED {
            let callee = read_register(file, record, offset_of!(CallRecord, callee_register));
            let expected = word_at(record, offset_of!(CallRecord, callee));
            if word_of(protocol::step(&mut stage, (), CallOperation::Callee { held: callee, expected })).is_none() {
                return Entered::Refused;
            }
        }
        // An arrow (lexical) binds no receiver: the deleted marker, its body reads `this` from its scope.
        let this = if MODE == LEXICAL {
            Value::DELETED.to_bits()
        } else {
            let receiver = read_register(file, record, offset_of!(CallRecord, this_register));
            let sloppy = (MODE == SLOPPY).then(|| word_at(record, offset_of!(CallRecord, global_this)));
            let Some(this) = word_of(protocol::step(&mut stage, (), CallOperation::BindThis { receiver, sloppy })) else {
                return Entered::Refused;
            };
            this
        };
        let payload = payload(s);
        let Some(admitted) = admit(&mut stage, record, payload) else { return Entered::Refused };
        let captures = word_at(record, offset_of!(CallRecord, captures));
        push::<ARGC, PLACED>(s, file, record, window, &admitted, this, captures);
        // A body with a row entry runs its rows (the row ABI); else its lane body entry.
        let rows = row_entry(admitted.cell, record);
        let returned = if rows != 0 {
            let entry: RowsEntry = transmute(rows);
            let RowsReturn { value, status } = entry(admitted.top, s.ctx, this, captures);
            BodyReturn { status: status as u32, value }
        } else {
            let entry: BodyEntry = transmute(word_at(admitted.cell, CELL_ENTRY_AT) as usize);
            entry(s.ctx, this, captures, admitted.top)
        };
        finish(file, record, &admitted, returned)
    }
}

macro_rules! direct_row {
    ($name:ident, $argc:expr, $mode:expr, $checked:expr, $placed:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match enter::<{ $argc }, { $mode }, { $checked }, { $placed }>(&s) {
                    Entered::Returned => go!(HOLE_TAKEN, dead(&s)),
                    Entered::Refused => {
                        core::hint::cold_path();
                        go!(HOLE_NEXT, dead(&s))
                    }
                    Entered::Raised => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, raising(&s, payload(&s), hole64::<{ DirectHole::CallPc as u8 }>()))
                    }
                    Entered::Left(status, word) => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, leaving(&s, status, word))
                    }
                }
            }
        }
    };
}

/// The unconditional leave: every register is passed to the exit as it came.
#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn exit_here(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
    let s = match crate::words::exit_here(S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] }) {
        Out::Exit(s) | Out::Next(s) | Out::Taken(s) => s,
    };
    unsafe { go!(HOLE_EXIT0, s) }
}

/// A pinned register back from the tagged word a `Spill` row wrote to the frame slot at hole 0:
/// the proof (an int word, a Number) was made by the spill, so it is not made again. Backend glue:
/// the planner's spill slots, which no protocol row names.
#[inline(always)]
unsafe fn reload<const P: u8>(s: S) -> S {
    let mut s = s;
    let mut stage = StencilStage::from_hole(0);
    let at = stage.site(0); // the spill's frame slot (first value hole), holding the word the spill wrote
    let word = stage.load_site(Region::Frame.mutable(), s.frame as u64, at);
    let int = value::is_int(&mut stage, word);
    match P {
        PIN..=LAST_INT_PIN => s.p[(P - PIN) as usize] = word as u32 as usize,
        _ => s.f[(P - PINF) as usize] = if int { f64::from(word as u32 as i32) } else { f64::from_bits(word) },
    }
    s
}

macro_rules! reload_row {
    ($name:ident, $place:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = reload::<{ $place }>(S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] });
                go!(HOLE_NEXT, s)
            }
        }
    };
}

// The direct-call and reload row instances, one per argc, mode, identity check and window variant, and one
// reload per pinned place (build/call_rows.rs).
include!(env!("QUENCH_STENCIL_CALL_ROWS"));

