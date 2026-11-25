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

use crate::layout::call_record::{helper_outcome, DirectHole, HelperHole, LaneStatus, Outcome, CALLEE_EXIT};
use crate::codes::{LAST_INT_PIN, PIN, PINF};
use crate::heap_rows::{payload, word_at};
use crate::layout::{JS_STEP_LANE_CONTEXT, JS_STEP_RESIDUAL, JS_STEP_VM};
use crate::direct_frame::{DirectFrame, Ended, LEXICAL, SLOPPY, STRICT};
use crate::stage::StencilStage;
use crate::staged::region::Region;
use crate::staged::{protocol, value, Stage, Step};
use crate::words::{Out, S, dead, hole64};
use crate::{Row, Value};
use core::mem::transmute;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_TAKEN: u8;
    static HOLE_EXIT0: u8;
    static HOLE_EXIT1: u8;
}

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

/// A semantic helper (`body_call_helper`): (vm, residual, lane context, call pc) -> status.
type Helper = unsafe extern "C" fn(u64, u64, u64, u64) -> u64;

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

/// The direct call: the staged call (`invoke::invoke`, its `enter_direct`) over the row's frame
/// (`direct_frame::DirectFrame`), which holds the record's plan, the argument places, admission, the
/// entry and the settle; how the call ended is the frame's after the step.
#[inline(always)]
unsafe fn enter<const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool>(s: &S) -> Ended {
    let record = hole64::<{ DirectHole::Record as u8 }>() as usize;
    let mut stage = StencilStage::from_hole(0);
    let mut places = crate::places::Places::new::<0, 0, 0>(*s);
    let mut frame = DirectFrame::<ARGC, MODE, CHECKED, PLACED>::new(&mut places, record);
    let site = frame.site();
    let returned = matches!(crate::staged::invoke::invoke(&mut stage, &mut frame, (), site), Step::Value(_));
    frame.ended(returned)
}

macro_rules! direct_row {
    ($name:ident, $argc:expr, $mode:expr, $checked:expr, $placed:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match enter::<{ $argc }, { $mode }, { $checked }, { $placed }>(&s) {
                    Ended::Returned => go!(HOLE_TAKEN, dead(&s)),
                    Ended::Refused => {
                        core::hint::cold_path();
                        go!(HOLE_NEXT, dead(&s))
                    }
                    Ended::Raised => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, raising(&s, payload(&s), hole64::<{ DirectHole::CallPc as u8 }>()))
                    }
                    Ended::Left(status, word) => {
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

