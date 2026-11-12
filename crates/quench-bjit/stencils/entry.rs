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
//!
//! A function body (`entry_body`, the lane body ABI of `lane_body_abi.rs`) is entered as
//! (rdi = context, rsi = receiver, rdx = captures, rcx = the register file) and returns the
//! status in eax and its result word in rdx. Its leave routines put the word in rax and the
//! status in rdx, which the thunk swaps on the way out: `leave_stop` (hole 0 the word, hole 1
//! the status), `leave_stop_pop` (the word an exit stub pushed) and `leave_returned` (the
//! activation completes with the register slot at hole 0; hole 1 the status, hole 2 the
//! record that completes it, which the lane reads from the dispatch payload). A body's captures
//! word is unpacked into the payload's capture words for its run (the slots, their count and whether
//! they admit every direct store), as a loop region's entry leaves them, and cleared when it returns;
//! its receiver (rsi) is published as the payload's receiver for the run, and `DELETED` on return.

use crate::layout::{
    CALL_CONTEXT_ADAPTER_PAYLOAD, CAPTURE_ADDRESS_BITS, CAPTURE_ADDRESS_PAD, CAPTURE_STORES_BIT, JS_STEP_CAPTURE_LEN, JS_STEP_CAPTURE_STORES, JS_STEP_CAPTURE_VALUES,
    JS_STEP_RECEIVER, JS_STEP_RETURNED_PC,
};
use crate::layout::words::DELETED;
use crate::staged::frame;
use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
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

/// The body entry: saves the callee-saved registers, takes the context in r13 and the register
/// file in r12, publishes the captures word (the dense slots' address below `CAPTURE_ADDRESS_BITS`,
/// whose low bit says they admit every direct store, and their count above) as the payload's
/// capture words and the receiver (rsi) as the payload's receiver, calls the first row, clears
/// them (the receiver to `DELETED`), and
/// returns (status, word) as the chain left them in (rdx, rax). The pad keeps the stack 16-byte
/// aligned at the call, as the six pushes and the return address leave it 8 off.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_body() {
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
        "mov rax, [rdi + {payload}]",
        "mov [rax + {receiver}], rsi",
        "mov r8, rdx",
        "shl r8, {pad}",
        "shr r8, {pad}",
        "mov r9, r8",
        "and r9, {stores_bit}",
        "mov [rax + {stores}], r9",
        "xor r8, r9",
        "mov [rax + {values}], r8",
        "shr rdx, {bits}",
        "mov [rax + {len}], rdx",
        "call {first}",
        "mov rcx, [r13 + {payload}]",
        "mov qword ptr [rcx + {values}], 0",
        "mov qword ptr [rcx + {len}], 0",
        "mov qword ptr [rcx + {stores}], 0",
        "movabs r8, {deleted}",
        "mov [rcx + {receiver}], r8",
        "add rsp, 8",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "xchg rax, rdx",
        "ret",
        first = sym HOLE_NEXT,
        payload = const CALL_CONTEXT_ADAPTER_PAYLOAD,
        values = const JS_STEP_CAPTURE_VALUES,
        len = const JS_STEP_CAPTURE_LEN,
        stores = const JS_STEP_CAPTURE_STORES,
        stores_bit = const CAPTURE_STORES_BIT,
        receiver = const JS_STEP_RECEIVER,
        deleted = const DELETED,
        bits = const CAPTURE_ADDRESS_BITS,
        pad = const CAPTURE_ADDRESS_PAD,
    )
}

/// Leaves a body with the word at hole 0 and the status at hole 1.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_stop() {
    core::arch::naked_asm!("movabs rax, offset {word}", "movabs rdx, offset {status}", "ret", word = sym HOLE_V0, status = sym HOLE_V1)
}

/// Leaves a body with the word an exit stub pushed and the status at hole 1.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_stop_pop() {
    core::arch::naked_asm!("pop rax", "movabs rdx, offset {status}", "ret", status = sym HOLE_V1)
}

/// Completes the activation: writes the record at hole 2 to the dispatch payload's
/// `returned_pc`, and leaves with the register slot at hole 0 as the word and the status at hole 1.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_returned() {
    core::arch::naked_asm!(
        "mov rax, [r12 + {slot}]",
        "mov rcx, [r13 + {payload}]",
        "movabs rdx, offset {record}",
        "mov [rcx + {returned}], rdx",
        "movabs rdx, offset {status}",
        "ret",
        slot = sym HOLE_V0,
        status = sym HOLE_V1,
        record = sym HOLE_V2,
        payload = const CALL_CONTEXT_ADAPTER_PAYLOAD,
        returned = const JS_STEP_RETURNED_PC,
    )
}
