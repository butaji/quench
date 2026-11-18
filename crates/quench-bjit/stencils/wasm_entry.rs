//! The routines around a Wasm function's rows (x86-64): the entry, which takes the u-jit's
//! `WasmEntry` ABI (rdi = context, rsi = memory base, rdx = memory length, rcx = arguments,
//! r8 = result) and returns the status in eax, and the leave routines its rows end in.
//!
//! The entry saves the callee-saved registers, reserves the function's frame on the native stack
//! (hole 0: its bytes, 8 more than a multiple of 16, so the call to the first row is aligned),
//! puts the result pointer in the frame's first word and copies the arguments (hole 1: their
//! count) into the words after it, the registers the parameters are, and zeroes the other
//! registers (hole 2: their count), as the interpreter's frame starts them. The rows run with the
//! frame in r12 and the context in r13 (the row convention); a leave returns to the entry with
//! the status in eax.

use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
}

/// Enters the function: frame, arguments, then the first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_wasm() {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "movabs rax, offset {frame}",
        "sub rsp, rax",
        "mov r12, rsp",
        "mov r13, rdi",
        "mov [r12], r8",
        "lea rdi, [r12 + 8]",
        "mov rsi, rcx",
        "movabs rcx, offset {count}",
        "rep movsq",
        "xor eax, eax",
        "movabs rcx, offset {rest}",
        "rep stosq",
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
        count = sym HOLE_V1,
        rest = sym HOLE_V2,
        first = sym HOLE_NEXT,
    )
}

/// Returns the frame word at byte offset hole 0 through the result pointer, status 0.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_wasm_word() {
    core::arch::naked_asm!(
        "movabs rcx, offset {slot}",
        "mov rax, [r12 + rcx]",
        "mov rcx, [r12]",
        "mov [rcx], rax",
        "xor eax, eax",
        "ret",
        slot = sym HOLE_V0,
    )
}

/// Returns no result, status 0.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_wasm_void() {
    core::arch::naked_asm!("xor eax, eax", "ret")
}

/// Returns the status a trapping row left in the first pin (r14).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_wasm_trap() {
    core::arch::naked_asm!("mov eax, r14d", "ret")
}
