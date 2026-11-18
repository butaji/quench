//! The routines around a Wasm function's rows (x86-64): the entry, which takes the u-jit's
//! `WasmEntry` ABI (rdi = context, rsi = memory base, rdx = memory length, rcx = arguments,
//! r8 = result) and returns the status in eax, and the leave routines its rows end in.
//!
//! The entry saves the callee-saved registers, reserves the function's frame on the native stack
//! (hole 0: its bytes, 8 more than a multiple of 16, so the call to the first row is aligned),
//! puts the result pointer, the memory base and its length in the frame's header words, and copies the arguments (hole 1: their
//! count) into the registers the parameters are (hole 3: the first one's frame offset, past
//! the instance bindings), and zeroes the registers after them (hole 2: their count), as the
//! interpreter's frame starts them. The rows run with the
//! frame in r12, the context in r13, the memory base in r15 and its length in rdi (the second
//! and third pins of the row convention); a leave returns to the entry with the status in eax.

use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
    static HOLE_V3: u8;
}

/// The status of a call past the context's stack limit (its first word), which the entry
/// returns before it touches the stack, as the u-jit's entry does.
const EXHAUSTED: u32 = match crate::WasmTrap::CallStackExhausted.compiled_status() {
    Some(status) => status,
    None => 0,
};

/// Enters the function: frame, arguments, then the first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_wasm() {
    core::arch::naked_asm!(
        "cmp rsp, [rdi]",
        "jae 2f",
        "mov eax, {exhausted}",
        "ret",
        "2:",
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
        "mov r15, rsi",
        "mov rbx, rdx",
        "mov [r12], r8",
        "mov [r12 + 8], rsi",
        "mov [r12 + 16], rdx",
        "movabs rdi, offset {first_parameter}",
        "add rdi, r12",
        "mov rsi, rcx",
        "movabs rcx, offset {count}",
        "rep movsq",
        "xor eax, eax",
        "movabs rcx, offset {rest}",
        "rep stosq",
        "mov rdi, rbx",
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
        first_parameter = sym HOLE_V3,
        exhausted = const EXHAUSTED,
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
