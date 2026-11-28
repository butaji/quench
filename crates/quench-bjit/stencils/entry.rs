//! The routines around a region's rows that are not rows themselves: the entry thunk, which
//! saves the callee-saved registers and calls the first row, and the two leave routines, which
//! return a word to it. Naked functions: their bytes are exactly the assembly below, so the
//! runtime copies them like any stencil (`entry_*` may `call`, `leave_*` may `ret`; `build.rs`
//! allows nothing else to).
//!
//! Entry ABI: the lane body ABI (rdi = context, rcx = the register file). The thunk moves the
//! context to r13 and the register file to r12 (the row convention), calls the first row
//! (`HOLE_NEXT`) and returns (status, word) in (eax, rdx): `NATIVE_JS_STOPPED` from a region's own leaves
//! (`leave_word`, `leave_pop`), the status of the call that ended the run from `leave_status` (`call_rows.rs`).
//!
//! A function body (`entry_body`, the lane body ABI of `lane_body_abi.rs`) is entered as
//! (rdi = context, rsi = receiver, rdx = captures, rcx = the register file) and returns the
//! status in eax and its result word in rdx. Its receiver and captures words are the header of its
//! own window (`CONTEXT_THIS`, `CONTEXT_BODY`), written by whoever enters it (the call row's push, or
//! the lane's own entry; the entry writes the captures word it is passed too), so the entry saves and restores nothing: a callee's words never replace its
//! caller's. Its leave routines put the word in rax and the status in rdx, which the thunk swaps on
//! the way out: `leave_stop` (hole 0 the word, hole 1 the status), `leave_stop_pop` (the word an exit
//! stub pushed) and `leave_returned` (the activation completes with the register slot at hole 0; hole 1
//! the status, hole 2 the record that completes it, which the lane reads from the dispatch payload).

use crate::layout::{CALL_CONTEXT_ADAPTER_PAYLOAD, CONTEXT_BODY, JS_STEP_RETURNED_PC};
use crate::layout::call_record::NATIVE_JS_STOPPED;
use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
}

/// The thunk around a call of the row chain: saves rbp, rbx, r12-r15, takes the context in r13 and the register file
/// in r12 (the row convention), runs `$pre`, calls the chain through `$call`, runs `$post` and returns (status, word):
/// the chain leaves them in (rdx, rax). The status goes to eax and the word to both rcx and rdx, so one return
/// serves both conventions a body is entered under: the body convention (`BODY_CALL_CONV`, Cranelift `tail`, whose
/// second return register is rcx) and SysV (rdx). The pad (`sub rsp, 8`) keeps the stack
/// 16-byte aligned at the call, as the six pushes and the return address leave it 8 off. Written once for every entry.
macro_rules! thunk {
    ([$($pre:literal),*], $call:literal, [$($post:literal),*] $(, $($binding:tt)*)?) => {
        core::arch::naked_asm!(
            "push rbp",
            "push rbx",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "mov r13, rdi",
            "mov r12, rcx",
            $($pre,)*
            $call,
            $($post,)*
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbx",
            "pop rbp",
            "mov rcx, rax",
            "mov rax, rdx",
            "mov rdx, rcx",
            "ret"
            $(, $($binding)*)?
        )
    };
}

/// Calls the first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_thunk() {
    thunk!(["sub rsp, 8"], "call {first}", ["add rsp, 8"], first = sym HOLE_NEXT)
}

/// Calls the address in rdx (the body ABI's `captures` argument, which a stencil region does not
/// read): a resume stub of the region, which loads the pinned registers live at its PC and jumps
/// to the PC's first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_resume() {
    thunk!(["sub rsp, 8"], "call rdx", ["add rsp, 8"])
}

/// Returns hole 0 (a word the region leaves with: a PC or a lane exit), status `NATIVE_JS_STOPPED`.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_word() {
    core::arch::naked_asm!("movabs rax, offset {word}", "mov edx, {stopped}", "ret", word = sym HOLE_V0, stopped = const NATIVE_JS_STOPPED)
}

/// Returns the word an exit stub pushed, status `NATIVE_JS_STOPPED`.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_pop() {
    core::arch::naked_asm!("pop rax", "mov edx, {stopped}", "ret", stopped = const NATIVE_JS_STOPPED)
}

/// The body entry (the lane body ABI): calls the first row. The receiver is in the window's header
/// already (`CONTEXT_THIS`, an arrow's lexical one included). The captures word the entry is passed
/// (rdx) is written to the header (`CONTEXT_BODY`) here, so a caller of the Cranelift body ABI, which
/// passes it but writes no header, enters a stencil body that took its function over as well.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_body() {
    thunk!(["sub rsp, 8", "mov [r12 - {body}], rdx"], "call {first}", ["add rsp, 8"], first = sym HOLE_NEXT, body = const CONTEXT_BODY * 8)
}

/// The body's row entry (`EntryCell::rows`): a b-jit direct call row (`rust-preserve-none`: window r12,
/// context r13, receiver r14, captures r15), which keeps nothing live across the call, so no callee-saved
/// register is saved. The window's header already holds the receiver and captures (the call row's push).
/// Calls the first row and returns the leave routine's (rax word, rdx status) as they are.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_rows() {
    core::arch::naked_asm!("sub rsp, 8", "call {first}", "add rsp, 8", "ret", first = sym HOLE_NEXT)
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
