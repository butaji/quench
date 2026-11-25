//! The routines around a regexp program's rows (x86-64): the entry, which takes the u-jit's
//! `NativeFn` ABI (rdi = the subject's bytes, rsi = its length, rdx = the start, rcx = the
//! registers, r8 = the backtracking stack, r9 = its end, then on the stack the last start and
//! the address the start is written to) and returns the match's end, `NATIVE_FAIL` or
//! `NATIVE_OVERFLOW` in rax, and the leave routines its rows end in.
//!
//! The entry saves the callee-saved registers and reserves the context words (hole 0: their
//! bytes, the marks included, 8 more than a multiple of 16 so the call to the first row is
//! aligned): the stack's end, the last start, the found address and the current start
//! (`regexp_layout::CONTEXT_*`). The rows run with the registers in r12, the context in r13,
//! the position in r14, the stack's top in r15, the bytes in rdi and the length in rsi (the
//! row convention's frame, context and four pins); a leave returns to the entry.

use crate::regexp_layout::{CONTEXT_FOUND, CONTEXT_LAST, CONTEXT_STACK_END, CONTEXT_START};
use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
}

/// The bytes the six saved registers and the return address take above the context words.
const SAVED: usize = 7 * 8;

/// Enters the rows: context words, then the first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_regexp() {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "movabs rax, offset {frame}",
        "sub rsp, rax",
        "mov r13, rsp",
        "mov [r13 + {stack_end}], r9",
        "mov r10, [r13 + rax + {last_at}]",
        "mov [r13 + {last}], r10",
        "mov r10, [r13 + rax + {found_at}]",
        "mov [r13 + {found}], r10",
        "mov [r13 + {start}], rdx",
        "mov r12, rcx",
        "mov r14, rdx",
        "mov r15, r8",
        "call {first}",
        "movabs rcx, offset {frame}",
        "add rsp, rcx",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
        frame = sym HOLE_V0,
        first = sym HOLE_NEXT,
        stack_end = const CONTEXT_STACK_END,
        last = const CONTEXT_LAST,
        found = const CONTEXT_FOUND,
        start = const CONTEXT_START,
        last_at = const SAVED,
        found_at = const SAVED + 8,
    )
}

/// Returns the position: the match's end.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_regexp_match() {
    core::arch::naked_asm!("mov rax, r14", "ret")
}

/// Returns `NATIVE_FAIL`: no start from here matches.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_regexp_fail() {
    core::arch::naked_asm!("mov rax, -1", "ret")
}

/// Returns `NATIVE_OVERFLOW`: the stack is full, and the interpreter runs the search again.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_regexp_overflow() {
    core::arch::naked_asm!("mov rax, -2", "ret")
}
