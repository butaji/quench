//! AArch64 forms of the routines around the rows (`entry.rs` has the x86-64 ones), with the same
//! names, entry ABIs and contracts.
//!
//! Row registers (`rust-preserve-none` on AArch64, LLVM's `CC_AArch64_Preserve_None`): integer
//! arguments go to x20, x21, x22, ... first, so frame x20, context x21, pinned ints x22..x25; the
//! pinned doubles are d0..d7. `preserve_none` keeps nothing but fp (x29), lr (x30) and x18, so the
//! entry routines save every AAPCS64 callee-saved register (x19..x28, x29, x30, d8..d15) themselves.
//! The register names below are that map's, in one place (`FRAME`, `CONTEXT`, `P0`, `P1`), and the
//! stencil tests check them against what LLVM assigned (`ROW_ABI_A64`).
//!
//! Leave routines end with the word in x0 and the status in x1, as the x86-64 ones end with
//! (rax, rdx); `entry_thunk`, `entry_body` and `entry_resume` swap them into the lane body ABI's
//! (w0 status, x1 word).
//!
//! Entry ABI (`entry_thunk`, `entry_body`, `entry_resume`, `lane_body_abi.rs`): (x0 = context,
//! x1 = receiver, x2 = captures, x3 = the register file) -> (w0 = status, x1 = word). The receiver
//! and captures words are the window's header already (`CONTEXT_THIS`, `CONTEXT_BODY`), written by
//! whoever enters the body, so the entries save none of the dispatch payload.
//!
//! Holes in assembly: ELF patches four `movz`/`movk` groups (`R_AARCH64_MOVW_UABS_G*`), Mach-O a
//! literal word the code loads and branches over (`words.rs` `hole_word!`, the same two forms).

use crate::layout::{CALL_CONTEXT_ADAPTER_PAYLOAD, JS_STEP_RETURNED_PC};
use crate::layout::call_record::NATIVE_JS_STOPPED;
use crate::staged::frame;
use crate::words::HOLE_V0;
use crate::Row;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
}

/// Bytes an exit stub pushes: one 16-byte slot, which keeps `sp` 16-byte aligned (`quench_bjit::stub`).
const STUB_SLOT: usize = 16;

/// The 64-bit value of hole `$h` into register `$r`, as one assembly string per target.
#[cfg(not(target_vendor = "apple"))]
macro_rules! hole_into {
    ($r:literal, $h:literal) => {
        concat!(
            "movz ", $r, ", #:abs_g3:{", $h, "}\n",
            "movk ", $r, ", #:abs_g2_nc:{", $h, "}\n",
            "movk ", $r, ", #:abs_g1_nc:{", $h, "}\n",
            "movk ", $r, ", #:abs_g0_nc:{", $h, "}"
        )
    };
}
#[cfg(target_vendor = "apple")]
macro_rules! hole_into {
    ($r:literal, $h:literal) => {
        concat!("ldr ", $r, ", 2f\n", "b 3f\n", ".p2align 3\n", "2:\n", ".quad {", $h, "}\n", "3:")
    };
}

/// Saves every AAPCS64 callee-saved register (x19..x30, d8..d15: 160 bytes, 16-byte aligned), runs
/// `$pre` with the body ABI's arguments still in x0..x3, puts the context in x21 and the register file
/// in x20 (the row map), calls the chain through `$call`, runs `$post`, restores, and returns the
/// chain's (x0 word, x1 status) as the body ABI's (w0 status, x1 word).
macro_rules! body_thunk {
    ([$($pre:expr),*], $call:expr, [$($post:expr),*] $(, $($binding:tt)*)?) => {
        core::arch::naked_asm!(
            "stp x29, x30, [sp, #-160]!",
            "mov x29, sp",
            "stp x19, x20, [sp, #16]",
            "stp x21, x22, [sp, #32]",
            "stp x23, x24, [sp, #48]",
            "stp x25, x26, [sp, #64]",
            "stp x27, x28, [sp, #80]",
            "stp d8, d9, [sp, #96]",
            "stp d10, d11, [sp, #112]",
            "stp d12, d13, [sp, #128]",
            "stp d14, d15, [sp, #144]",
            $($pre,)*
            "mov x21, x0",
            "mov x20, x3",
            $call,
            $($post,)*
            "mov x2, x0",
            "mov w0, w1",
            "mov x1, x2",
            "ldp d14, d15, [sp, #144]",
            "ldp d12, d13, [sp, #128]",
            "ldp d10, d11, [sp, #112]",
            "ldp d8, d9, [sp, #96]",
            "ldp x27, x28, [sp, #80]",
            "ldp x25, x26, [sp, #64]",
            "ldp x23, x24, [sp, #48]",
            "ldp x21, x22, [sp, #32]",
            "ldp x19, x20, [sp, #16]",
            "ldp x29, x30, [sp], #160",
            "ret"
            $(, $($binding)*)?
        )
    };
}

/// A region's entry (the lane body ABI): sets the row map's context and window and calls the
/// first row, as `entry_body` does; its leave routines return (word, status), which the thunk
/// swaps into the body ABI's (status, word).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_thunk() {
    body_thunk!([], "bl {first}", [], first = sym HOLE_NEXT)
}

/// Calls the address in x2 (the body ABI's `captures` argument, which a stencil region does not read):
/// a resume stub of the region, which loads the pinned registers live at its PC and branches to the
/// PC's first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_resume() {
    body_thunk!(["mov x16, x2"], "blr x16", [])
}

/// The body entry (the lane body ABI): calls the first row. The receiver and captures words it is
/// entered with are in the window's header already (`CONTEXT_THIS`, `CONTEXT_BODY`), written by the
/// caller, so nothing is saved or restored here.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_body() {
    body_thunk!([], "bl {first}", [], first = sym HOLE_NEXT)
}

/// The body's row entry (`EntryCell::rows`): a b-jit direct call row (`rust-preserve-none`: window x20,
/// context x21, receiver x22, captures x23), which keeps nothing live across the call, so only the frame
/// record (x29, x30) is saved. The window's header already holds the receiver and captures (the call row's
/// push). Returns the leave routine's (x0 word, x1 status) as they are.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_rows() {
    core::arch::naked_asm!(
        "stp x29, x30, [sp, #-16]!",
        "mov x29, sp",
        "bl {first}",
        "ldp x29, x30, [sp], #16",
        "ret",
        first = sym HOLE_NEXT,
    )
}

/// Returns hole 0 (a word the region leaves with: a PC or a lane exit), status `NATIVE_JS_STOPPED`.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_word() {
    core::arch::naked_asm!(hole_into!("x0", "word"), "mov w1, #{stopped}", "ret", word = sym HOLE_V0, stopped = const NATIVE_JS_STOPPED)
}

/// Returns the word an exit stub pushed, status `NATIVE_JS_STOPPED`.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_pop() {
    core::arch::naked_asm!("ldr x0, [sp], #{slot}", "mov w1, #{stopped}", "ret", slot = const STUB_SLOT, stopped = const NATIVE_JS_STOPPED)
}

/// Leaves a body with the word at hole 0 and the status at hole 1.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_stop() {
    core::arch::naked_asm!(hole_into!("x0", "word"), hole_into!("x1", "status"), "ret", word = sym HOLE_V0, status = sym HOLE_V1)
}

/// Leaves a body with the word an exit stub pushed and the status at hole 1.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_stop_pop() {
    core::arch::naked_asm!("ldr x0, [sp], #{slot}", hole_into!("x1", "status"), "ret", slot = const STUB_SLOT, status = sym HOLE_V1)
}

/// Completes the activation: writes the record at hole 2 to the dispatch payload's `returned_pc`, and
/// leaves with the register slot at byte offset hole 0 of the frame (x20) as the word and the status at hole 1.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_returned() {
    core::arch::naked_asm!(
        hole_into!("x9", "slot"),
        "ldr x0, [x20, x9]",
        "ldr x10, [x21, #{payload}]",
        hole_into!("x11", "record"),
        "str x11, [x10, #{returned}]",
        hole_into!("x1", "status"),
        "ret",
        slot = sym HOLE_V0,
        status = sym HOLE_V1,
        record = sym HOLE_V2,
        payload = const CALL_CONTEXT_ADAPTER_PAYLOAD,
        returned = const JS_STEP_RETURNED_PC,
    )
}
