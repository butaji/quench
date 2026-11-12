//! The routines around a region's rows that are not rows themselves: the entry thunk, which
//! saves the callee-saved registers and calls the first row, and the two leave routines, which
//! return a word to it. Naked functions: their bytes are exactly the assembly below, so the
//! runtime copies them like any stencil (`entry_*` may `call`, `leave_*` may `ret`; `build.rs`
//! allows nothing else to).
//!
//! Entry ABI: (rdi = context, rcx = the frame, r8 = result pointer). The thunk moves the
//! context to r13 and loads the frame's window of registers (`staged::frame::WINDOW`) into r12
//! (the row convention), calls the first row
//! (`HOLE_NEXT`), stores the word the chain returned through the result pointer and returns
//! status `NATIVE_JS_STOPPED`, the status of every run that leaves with a word. `entry_resume` is the same thunk calling the address in rdx, where a resume stub
//! enters the chain mid-region.

use crate::layout::NATIVE_JS_STOPPED;
use crate::staged::frame;
use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
}

/// The thunk around a call of the row chain: saves rbp, rbx, r12-r15 and the result pointer
/// r8, calls the chain through `$call`, stores its word. Written once for both entries.
macro_rules! thunk {
    ($call:literal $(, $($binding:tt)*)?) => {
        core::arch::naked_asm!(
            "push rbp",
            "push rbx",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "push r8",
            "mov r13, rdi",
            "mov r12, [rcx + {window}]",
            $call,
            "pop r8",
            "mov [r8], rax",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbx",
            "pop rbp",
            "mov eax, {stopped}",
            "ret",
            window = const frame::WINDOW,
            stopped = const NATIVE_JS_STOPPED
            $(, $($binding)*)?
        )
    };
}

/// Calls the first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_thunk() {
    thunk!("call {first}", first = sym HOLE_NEXT)
}

/// Calls the address in rdx (the shared ABI's `length` argument): a resume stub of the region,
/// which loads the pinned registers live at its PC and jumps to the PC's first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_resume() {
    thunk!("call rdx")
}

/// Returns hole 0 (a word the region leaves with: a PC or a lane exit).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_word() {
    core::arch::naked_asm!("movabs rax, offset {word}", "ret", word = sym HOLE_V0)
}

/// Returns the word an exit stub pushed.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_pop() {
    core::arch::naked_asm!("pop rax", "ret")
}
