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
//! the helper's address (`lane_body_call`); vm, residual and lane context come from the
//! dispatch payload behind the context. Successors: COMPLETED continues at NEXT; MISSED
//! (nothing ran) leaves at the call's pc through EXIT0 (a leave that does not spill); LEFT and
//! TERMINATED leave through EXIT1 with the (status, word) in r14, r15 (`leave_status`); BRANCHED
//! (a step that took its conditional jump) continues at TAKEN, the reloads of the registers live
//! at the jump's target.
//!
//! Direct row `call_direct_a<N>_<strict|sloppy|lexical>`: hole V0 = the address of the site's
//! `CallRecord`. It checks the callee word and (sloppy) the receiver (an arrow, lexical, binds
//! none: its body finds its `this` in its environment, as `lane_body_call` passes it), admits the call with the
//! shared `admit_native` row (one validity compare, one room check) plus the shadow frame's
//! room, writes `N` arguments and `undefined` for the rest into the callee's window at the lane
//! stack's top, pushes the shadow frame, calls the cell's entry with the body ABI (context,
//! this, captures, window) and handles the returned (status, word) as the u-jit's direct call
//! does. Successors: refusal before any effect, or a callee that was not entered (the stub),
//! goes to NEXT, which the planner makes the helper row; a completion stores the word to the
//! call's register and goes to TAKEN (the reloads); any other status leaves through EXIT0 with
//! (status, word) in r14, r15, the callee's shadow frames kept for the lane to materialize.
//!
//! Exit row `exit_here`: leaves through EXIT0 with every register as it came, so the exit stub
//! writes the live pinned registers back: the lowering of an instruction whose operation only
//! the interpreter runs.

use crate::layout::shared_words::KIND_NATIVE;
use crate::layout::call_record::{CallRecord, CALLEE_EXIT, LANE_BODY_NOT_ENTERED, LANE_BODY_RETURNED, LANE_BODY_THREW, LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_RAISED, NATIVE_JS_STOPPED, REGISTER_BYTES, TERMINATED_STATUS};
use crate::codes::{LAST_INT_PIN, PIN, PINF};
use crate::heap_rows::{payload, word_at};
use crate::layout::{ACTIVATION_KIND_OFFSET, CELL_BODY_AT, CELL_COUNTERS_AT, CELL_ENTRY_AT, CELL_RECOVERY_AT, JS_STEP_LANE_ACTIVATIONS_TOP, JS_STEP_LANE_CONTEXT, JS_STEP_LANE_FRAMES_END, JS_STEP_LANE_VALUES_END, JS_STEP_LANE_VALUES_TOP, JS_STEP_RESIDUAL, JS_STEP_RETURNED_PC, JS_STEP_VM, FRAME_CONTEXT_WORDS, RAISED_RUN, RECOVERY_ACTIVATIONS_AT, RECOVERY_VALUES_AT};
use crate::stage::StencilStage;
use crate::staged::{call::{admit_native, window_fill}, frame, value};
use crate::words::{S, hole64, is_int, slot};
use crate::{Row, Value};
use core::mem::{offset_of, transmute};

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_TAKEN: u8;
    static HOLE_EXIT0: u8;
    static HOLE_EXIT1: u8;
}

/// The values holes of the call rows: the helper row's call pc and helper address, the direct
/// row's record address.
const HOLE_CALL_PC: u8 = 0;
const HOLE_HELPER: u8 = 1;
const HOLE_RECORD: u8 = 0;
/// A direct row's call record index in the caller, which a raise names to the lane (`raising`).
const HOLE_DIRECT_PC: u8 = 1;
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
/// A semantic helper (`lane_body_call`): (vm, residual, lane context, call pc) -> status.
type Helper = unsafe extern "C" fn(u64, u64, u64, u64) -> u64;

/// The state a successor receives where nothing is live: the frame and context, pinned
/// registers zero.
#[inline(always)]
fn dead(s: &S) -> S {
    S { frame: s.frame, ctx: s.ctx, p: [junk(); 4], f: [junk_f64(); 8] }
}

/// A word no successor reads (every pinned register is dead past a call): an empty assembly
/// output, so the row writes nothing into the register, where zeroing it cost an instruction
/// per register (12 per call: 4 integer `xor`, 8 `vxorps`).
#[inline(always)]
fn junk() -> usize {
    let word: usize;
    // SAFETY: an empty template; the register's prior contents are the value.
    unsafe { core::arch::asm!("/* {0} */", out(reg) word, options(pure, nomem, nostack, preserves_flags)) };
    word
}

/// `junk` for a pinned double.
#[inline(always)]
fn junk_f64() -> f64 {
    let word: f64;
    #[cfg(target_arch = "x86_64")]
    // SAFETY: as `junk`.
    unsafe { core::arch::asm!("/* {0} */", out(xmm_reg) word, options(pure, nomem, nostack, preserves_flags)) };
    #[cfg(target_arch = "aarch64")]
    // SAFETY: as `junk`.
    unsafe { core::arch::asm!("/* {0} */", out(vreg) word, options(pure, nomem, nostack, preserves_flags)) };
    word
}

/// `dead` with a (status, word) for `leave_status` in the first two pinned ints.
#[inline(always)]
fn leaving(s: &S, status: u32, word: u64) -> S {
    S { p: [status as usize, word as usize, 0, 0], ..dead(s) }
}

/// The caller's own end by the exception a callee raised (`Vm::lane_raised`): the record that raised it is
/// in the payload for the lane to unwind from (`LANE_BODY_THREW`).
#[inline(always)]
unsafe fn raising(s: &S, payload: usize, pc: u64) -> S {
    // SAFETY: the payload is the live activation's.
    unsafe { put(payload, JS_STEP_RETURNED_PC, pc) };
    leaving(s, LANE_BODY_THREW, 0)
}

macro_rules! go {
    ($hole:ident, $s:expr) => {{
        let s: S = $s;
        let next: Row = transmute(&raw const $hole);
        become next(s.frame, s.ctx, s.p[0], s.p[1], s.p[2], s.p[3], s.f[0], s.f[1], s.f[2], s.f[3], s.f[4], s.f[5], s.f[6], s.f[7])
    }};
}

#[inline(always)]
unsafe fn put(base: usize, offset: usize, word: u64) {
    // SAFETY: a layout offset of a live record, as every staged store.
    unsafe { ((base + offset) as *mut u64).write(word) }
}

/// The frame slot's address of the register the record names at `field`.
#[inline(always)]
unsafe fn register_slot(s: &S, record: usize, field: usize) -> usize {
    // SAFETY: a register index the planner stored in a live record.
    let register = unsafe { ((record + field) as *const u16).read() };
    s.frame + usize::from(register) * REGISTER_BYTES
}

/// The word of the register the record names at `field`.
#[inline(always)]
unsafe fn read_register(s: &S, record: usize, field: usize) -> u64 {
    // SAFETY: as `register_slot`, a register of the live frame.
    unsafe { (register_slot(s, record, field) as *const u64).read() }
}

#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn call_helper(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
    unsafe {
        let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
        let payload = payload(&s);
        let helper: Helper = transmute(hole64::<HOLE_HELPER>() as usize);
        let status = helper(word_at(payload, JS_STEP_VM), word_at(payload, JS_STEP_RESIDUAL), word_at(payload, JS_STEP_LANE_CONTEXT), hole64::<HOLE_CALL_PC>());
        match status {
            LANE_CALL_COMPLETED => go!(HOLE_NEXT, dead(&s)),
            LANE_CALL_BRANCHED => go!(HOLE_TAKEN, dead(&s)),
            LANE_CALL_MISSED => {
                core::hint::cold_path();
                go!(HOLE_EXIT0, dead(&s))
            }
            LANE_CALL_LEFT => {
                core::hint::cold_path();
                go!(HOLE_EXIT1, leaving(&s, NATIVE_JS_STOPPED, CALLEE_EXIT))
            }
            LANE_CALL_RAISED => {
                core::hint::cold_path();
                go!(HOLE_EXIT1, raising(&s, payload, hole64::<HOLE_CALL_PC>()))
            }
            _ => {
                core::hint::cold_path();
                go!(HOLE_EXIT1, leaving(&s, TERMINATED_STATUS, 0))
            }
        }
    }
}

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

/// How a direct row binds the receiver (`bound_this`).
const STRICT: u8 = 0;
const SLOPPY: u8 = 1;
const LEXICAL: u8 = 2;

/// The receiver the callee binds: strict takes it as it is, sloppy binds an object to itself and
/// `undefined` or `null` to the record's global object, any other receiver refused; an arrow
/// (lexical) binds none, the deleted marker.
#[inline(always)]
fn bound_this<const MODE: u8>(record: usize, this: u64) -> Option<u64> {
    if MODE == STRICT {
        return Some(this);
    }
    if MODE == LEXICAL {
        return Some(Value::DELETED.to_bits());
    }
    let object = value::is_direct_object(&mut StencilStage::from_hole(0), this);
    let nullish = this == Value::UNDEFINED.to_bits() || this == Value::NULL.to_bits();
    match (object, nullish) {
        (true, _) => Some(this),
        (false, true) => Some(word_at(record, offset_of!(CallRecord, global_this))),
        (false, false) => None,
    }
}

/// The callee's window at `top`: `ARGC` arguments from the caller's register `args`, then
/// `undefined`, `room` registers in all (the cell's fill, at least the arguments). Volatile stores keep LLVM from turning the loop into a
/// `memcpy` or `memset` call, which a stencil may not make.
#[inline(always)]
unsafe fn fill_window<const ARGC: usize>(args: usize, top: usize, room: usize) {
    // SAFETY: `ARGC` argument registers lie in the caller's window; `room` words lie at the lane
    // stack's top, which the admission proved free.
    unsafe {
        let passed: [u64; ARGC] = (args as *const [u64; ARGC]).read();
        for k in 0..room {
            let word = if k < ARGC { *passed.get_unchecked(k) } else { Value::UNDEFINED.to_bits() };
            ((top + (k << WORD_SHIFT)) as *mut u64).write_volatile(word);
        }
    }
}

/// What admission found: the callee's cell, the lane stack's values' top where its window goes, the
/// window's size and the activations' top where its native frame goes.
struct Admitted {
    cell: usize,
    top: usize,
    room: usize,
    /// The registers past the arguments the push starts `undefined` (`window_fill`): the rest are
    /// written before any read, as the interpreter's own call leaves them.
    fill: usize,
    shadow: usize,
}

/// The word at the address the payload's word `at` holds: a lane stack top, owned by the VM.
#[inline(always)]
unsafe fn stack_top(payload: usize, at: usize) -> usize {
    // SAFETY: the payload names the live lane stack's top words.
    unsafe { word_at(word_at(payload, at) as usize, 0) as usize }
}

/// Moves the lane stack's two tops: the values' and the activations'.
#[inline(always)]
unsafe fn set_tops(payload: usize, values: usize, activations: usize) {
    // SAFETY: as `stack_top`.
    unsafe {
        put(word_at(payload, JS_STEP_LANE_VALUES_TOP) as usize, 0, values as u64);
        put(word_at(payload, JS_STEP_LANE_ACTIVATIONS_TOP) as usize, 0, activations as u64);
    }
}

/// The cell's validity and both stacks' room: `admit_native` over the values' free words less the callee's
/// recovery space, and the frames its recovery needs.
#[inline(always)]
unsafe fn admit(record: usize, payload: usize) -> Option<Admitted> {
    // SAFETY: the record and cell are live for the code's life, the payload for the activation's.
    unsafe {
        let cell = word_at(record, offset_of!(CallRecord, cell)) as usize;
        let (top, end) = (stack_top(payload, JS_STEP_LANE_VALUES_TOP), word_at(payload, JS_STEP_LANE_VALUES_END) as usize);
        let recovery = word_at(cell, CELL_RECOVERY_AT) as usize;
        let free = (((end - top) >> WORD_SHIFT) as u64).saturating_sub(word_at(recovery, RECOVERY_VALUES_AT) + FRAME_CONTEXT_WORDS as u64);
        let room = admit_native(&mut StencilStage::from_hole(0), cell as u64, word_at(record, offset_of!(CallRecord, expected)), free, ())? as usize;
        let shadow = stack_top(payload, JS_STEP_LANE_ACTIVATIONS_TOP);
        let frames = word_at(recovery, RECOVERY_ACTIVATIONS_AT) as usize + 1;
        let fits = shadow + frames * word_at(record, offset_of!(CallRecord, frame_bytes)) as usize <= word_at(payload, JS_STEP_LANE_FRAMES_END) as usize;
        let fill = window_fill(&mut StencilStage::from_hole(0), cell as u64, room as u64) as usize;
        fits.then_some(Admitted { cell, top, room, fill, shadow })
    }
}

/// The callee's window, its native frame (`ActivationKind::Native`), and the lane stack's new tops.
#[inline(always)]
unsafe fn push<const ARGC: usize>(s: &S, record: usize, payload: usize, a: &Admitted, this: u64) {
    // SAFETY: as `admit`, which proved the room.
    unsafe {
        let written = if a.fill < ARGC { ARGC } else { a.fill };
        fill_window::<ARGC>(register_slot(s, record, offset_of!(CallRecord, args_register)), a.top, written);
        put(a.shadow, frame::CALLEE, word_at(record, offset_of!(CallRecord, descriptor)));
        put(a.shadow, frame::THIS, this);
        put(a.shadow, frame::BODY, word_at(a.cell, CELL_BODY_AT));
        put(a.shadow, frame::WINDOW, a.top as u64);
        put(a.shadow, ACTIVATION_KIND_OFFSET, u64::from(KIND_NATIVE));
        set_tops(payload, a.top + ((a.room + FRAME_CONTEXT_WORDS) << WORD_SHIFT), a.shadow + word_at(record, offset_of!(CallRecord, frame_bytes)) as usize);
    }
}

/// Where each status of the callee goes: a completion pops its window and frame and stores its word
/// to the call's register; a callee that was not entered pops them and is refused; one that ended by an
/// exception pops them, is counted in its cell and raises in the caller; any other status keeps the frames
/// (the lane links them) and is the caller's leave.
#[inline(always)]
unsafe fn finish(s: &S, record: usize, payload: usize, a: &Admitted, returned: BodyReturn) -> Entered {
    // SAFETY: as `push`.
    unsafe {
        match returned.status {
            LANE_BODY_RETURNED => {
                set_tops(payload, a.top, a.shadow);
                put(register_slot(s, record, offset_of!(CallRecord, result_register)), 0, returned.value);
                Entered::Returned
            }
            LANE_BODY_NOT_ENTERED => {
                set_tops(payload, a.top, a.shadow);
                Entered::Refused
            }
            LANE_BODY_THREW => {
                set_tops(payload, a.top, a.shadow);
                let counters = word_at(a.cell, CELL_COUNTERS_AT);
                put(a.cell, CELL_COUNTERS_AT, counters.wrapping_add(RAISED_RUN));
                Entered::Raised
            }
            status => Entered::Left(status, returned.value),
        }
    }
}

/// The direct call: the callee and receiver, admission, the push, the call, the pop.
#[inline(always)]
unsafe fn enter<const ARGC: usize, const MODE: u8>(s: &S) -> Entered {
    // SAFETY: as `admit`.
    unsafe {
        let record = hole64::<HOLE_RECORD>() as usize;
        if read_register(s, record, offset_of!(CallRecord, callee_register)) != word_at(record, offset_of!(CallRecord, callee)) {
            return Entered::Refused;
        }
        let this = read_register(s, record, offset_of!(CallRecord, this_register));
        let Some(this) = bound_this::<MODE>(record, this) else { return Entered::Refused };
        let payload = payload(s);
        let Some(admitted) = admit(record, payload) else { return Entered::Refused };
        push::<ARGC>(s, record, payload, &admitted, this);
        let entry: BodyEntry = transmute(word_at(admitted.cell, CELL_ENTRY_AT) as usize);
        let returned = entry(s.ctx, this, word_at(record, offset_of!(CallRecord, captures)), admitted.top);
        finish(s, record, payload, &admitted, returned)
    }
}

macro_rules! direct_row {
    ($name:ident, $argc:expr, $mode:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match enter::<{ $argc }, { $mode }>(&s) {
                    Entered::Returned => go!(HOLE_TAKEN, dead(&s)),
                    Entered::Refused => {
                        core::hint::cold_path();
                        go!(HOLE_NEXT, dead(&s))
                    }
                    Entered::Raised => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, raising(&s, payload(&s), hole64::<HOLE_DIRECT_PC>()))
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

direct_row!(call_direct_a0_strict, 0, STRICT);
direct_row!(call_direct_a1_strict, 1, STRICT);
direct_row!(call_direct_a2_strict, 2, STRICT);
direct_row!(call_direct_a3_strict, 3, STRICT);
direct_row!(call_direct_a0_sloppy, 0, SLOPPY);
direct_row!(call_direct_a1_sloppy, 1, SLOPPY);
direct_row!(call_direct_a2_sloppy, 2, SLOPPY);
direct_row!(call_direct_a3_sloppy, 3, SLOPPY);
direct_row!(call_direct_a0_lexical, 0, LEXICAL);
direct_row!(call_direct_a1_lexical, 1, LEXICAL);
direct_row!(call_direct_a2_lexical, 2, LEXICAL);
direct_row!(call_direct_a3_lexical, 3, LEXICAL);

/// The unconditional leave: every register is passed to the exit as it came.
#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn exit_here(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
    unsafe { go!(HOLE_EXIT0, S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] }) }
}

/// A pinned register back from the tagged word a `Spill` row wrote to the frame slot at hole 0:
/// the proof (an int word, a Number) was made by the spill, so it is not made again.
#[inline(always)]
unsafe fn reload<const P: u8>(s: S) -> S {
    let mut s = s;
    // SAFETY: the slot holds the word the preceding spill wrote.
    let word = unsafe { (slot::<0>(&s) as *const u64).read() };
    match P {
        PIN..=LAST_INT_PIN => s.p[(P - PIN) as usize] = word as u32 as usize,
        _ => s.f[(P - PINF) as usize] = if is_int(word) { f64::from(word as u32 as i32) } else { f64::from_bits(word) },
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

reload_row!(reload_4, PIN);
reload_row!(reload_5, PIN + 1);
reload_row!(reload_6, PIN + 2);
reload_row!(reload_7, PIN + 3);
reload_row!(reload_8, PINF);
reload_row!(reload_9, PINF + 1);
reload_row!(reload_10, PINF + 2);
reload_row!(reload_11, PINF + 3);
reload_row!(reload_12, PINF + 4);
reload_row!(reload_13, PINF + 5);
reload_row!(reload_14, PINF + 6);
reload_row!(reload_15, PINF + 7);
