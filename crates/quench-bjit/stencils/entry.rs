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
//! status 0.

use crate::staged::frame;
use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
}

/// Saves rbp, rbx, r12-r15 and the result pointer r8, calls the first row, stores its word.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_thunk() {
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
        "call {first}",
        "pop r8",
        "mov [r8], rax",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "xor eax, eax",
        "ret",
        first = sym HOLE_NEXT,
        window = const frame::WINDOW,
    )
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
