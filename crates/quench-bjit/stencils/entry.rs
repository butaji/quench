//! The routines around a region's rows that are not rows themselves: the entry thunk, which
//! saves the callee-saved registers and calls the first row, and the two leave routines, which
//! return a word to it. Naked functions: their bytes are exactly the assembly below, so the
//! runtime copies them like any stencil (`entry_*` may `call`, `leave_*` may `ret`; `build.rs`
//! allows nothing else to).
//!
//! Entry ABI, the lane body ABI: (rdi = context, rcx = the window of registers) -> (eax =
//! status, rdx = word). The thunk moves the context to r13 and the window into r12 (the row
//! convention), calls the first row (`HOLE_NEXT`) and returns the word the chain returned in
//! rdx beside status 0.

use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
}

/// Saves rbp, rbx and r12-r15, calls the first row, returns its word beside status 0. The
/// eighth word keeps the stack 16-byte aligned at the call.
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
        "sub rsp, 8",
        "mov r13, rdi",
        "mov r12, rcx",
        "call {first}",
        "add rsp, 8",
        "mov rdx, rax",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "xor eax, eax",
        "ret",
        first = sym HOLE_NEXT,
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
