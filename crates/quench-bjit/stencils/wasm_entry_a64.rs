//! The routines around a Wasm function's rows (AArch64): the AArch64 twin of `wasm_entry.rs`, with
//! the same symbols (`entry_wasm`, `leave_wasm_word`, `leave_wasm_void`, `leave_wasm_trap`), the same
//! `WasmEntry` ABI and the same frame layout. Naked functions: their bytes are exactly the assembly
//! below, copied and patched like any stencil (`entry_*` may `bl` the first row and `ret`, `leave_*`
//! may `ret`; `build/transfer.rs` allows nothing else to them).
//!
//! Entry ABI (AAPCS64, `extern "C"`): x0 = context, x1 = memory base, x2 = memory length, x3 = the
//! arguments, x4 = the result pointer; the status is returned in w0 (`WasmEntry`).
//!
//! The entry checks the context's first word (the stack limit) against `sp` before it touches the
//! stack, and returns `CallStackExhausted`'s status if the call would pass it, as `wasm_entry.rs`
//! does. It saves x19-x30 and d8-d15 (the rows are `preserve_none`, which has no callee-saved
//! registers, so every row may clobber them), reserves the frame (hole 0: its bytes, a multiple of 16
//! so `sp` stays 16-byte aligned; `wasm_stitch.rs` `frame_bytes` supplies it), and fills the frame:
//! word 0 the result pointer, word 1 the memory base, word 2 the memory length (`FRAME_HEADER_WORDS`
//! = 3), then the arguments (hole 1: their count) at the first parameter's frame offset (hole 3),
//! then zeroed words (hole 2: their count), as the interpreter's frame starts them.
//!
//! The rows run with the row convention of `stencils/rows.rs` on AArch64: frame in x0, context in
//! x1, pinned ints x2..x5 (p0 = x2 the status, p1 = x3 the memory base, p2 = x4 the memory length,
//! p3 = x5), pinned doubles d0..d7. The entry passes the frame as sp, the context, the memory base
//! and length; p0, p3 and the doubles are not set, as on x86-64. A leave returns to the entry's
//! `bl` with the status in w0.
//!
//! Hole loads are the four `movz`/`movk` groups of a 64-bit value (`words.rs` `hole_word!`, the
//! ELF form). Mach-O has no `MOVW` relocation, so this file is built for ELF targets only
//! (`rows.rs` cfg `not(target_vendor = "apple")`); a Mach-O form needs the literal-pool load
//! (`ldr` over a `.quad` of the hole, class `ABS64`) in place of each `movz`/`movk` group.

use crate::words::HOLE_V0;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
    static HOLE_V3: u8;
}

/// The status of a call past the context's stack limit, which the entry returns before it touches
/// the stack, as `wasm_entry.rs` does. `movz` takes 16 bits, so the status must fit them.
const EXHAUSTED: u32 = match crate::WasmTrap::CallStackExhausted.compiled_status() {
    Some(status) => status,
    None => 0,
};
const _: () = assert!(EXHAUSTED <= 0xffff);

/// Bytes the entry saves below its caller's `sp`: x29, x30 (the pre-indexed pair, at offset 0),
/// x19-x28 and d8-d15, 160 in all, a multiple of 16 so the frame above it stays aligned.
const SAVED: usize = 160;

/// Enters the function: stack limit check, frame, arguments, then the first row.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_wasm() {
    core::arch::naked_asm!(
        // The stack limit: the context's first word. Unsigned `sp >= limit` proceeds.
        "ldr x9, [x0]",
        "cmp sp, x9",
        "b.hs 2f",
        "movz w0, #{exhausted}",
        "ret",
        "2:",
        // Save the link and frame registers and the callee-saved registers the rows clobber.
        "stp x29, x30, [sp, #-{saved}]!",
        "stp x19, x20, [sp, #16]",
        "stp x21, x22, [sp, #32]",
        "stp x23, x24, [sp, #48]",
        "stp x25, x26, [sp, #64]",
        "stp x27, x28, [sp, #80]",
        "stp d8, d9, [sp, #96]",
        "stp d10, d11, [sp, #112]",
        "stp d12, d13, [sp, #128]",
        "stp d14, d15, [sp, #144]",
        // Reserve the frame (hole 0), a multiple of 16.
        "movz x9, #:abs_g3:{frame}",
        "movk x9, #:abs_g2_nc:{frame}",
        "movk x9, #:abs_g1_nc:{frame}",
        "movk x9, #:abs_g0_nc:{frame}",
        "sub sp, sp, x9",
        // Header: result pointer, memory base, memory length.
        "str x4, [sp]",
        "stp x1, x2, [sp, #8]",
        // Arguments (hole 1 words) at the first parameter's offset (hole 3); x6 is the first word after them.
        "movz x7, #:abs_g3:{first_parameter}",
        "movk x7, #:abs_g2_nc:{first_parameter}",
        "movk x7, #:abs_g1_nc:{first_parameter}",
        "movk x7, #:abs_g0_nc:{first_parameter}",
        "add x6, sp, x7",
        "mov x5, x3",
        "movz x8, #:abs_g3:{count}",
        "movk x8, #:abs_g2_nc:{count}",
        "movk x8, #:abs_g1_nc:{count}",
        "movk x8, #:abs_g0_nc:{count}",
        "3:",
        "cbz x8, 4f",
        "ldr x9, [x5], #8",
        "str x9, [x6], #8",
        "sub x8, x8, #1",
        "b 3b",
        "4:",
        // The registers after the parameters (hole 2 words), zeroed.
        "movz x8, #:abs_g3:{rest}",
        "movk x8, #:abs_g2_nc:{rest}",
        "movk x8, #:abs_g1_nc:{rest}",
        "movk x8, #:abs_g0_nc:{rest}",
        "5:",
        "cbz x8, 6f",
        "str xzr, [x6], #8",
        "sub x8, x8, #1",
        "b 5b",
        "6:",
        // The first row's arguments: frame, context, p1 = memory base, p2 = memory length.
        "mov x9, x0",
        "mov x10, x1",
        "mov x11, x2",
        "mov x0, sp",
        "mov x1, x9",
        "mov x3, x10",
        "mov x4, x11",
        "bl {first}",
        // The leave returned with its status in w0; release the frame and restore the saved registers.
        "movz x9, #:abs_g3:{frame}",
        "movk x9, #:abs_g2_nc:{frame}",
        "movk x9, #:abs_g1_nc:{frame}",
        "movk x9, #:abs_g0_nc:{frame}",
        "add sp, sp, x9",
        "ldp d14, d15, [sp, #144]",
        "ldp d12, d13, [sp, #128]",
        "ldp d10, d11, [sp, #112]",
        "ldp d8, d9, [sp, #96]",
        "ldp x27, x28, [sp, #80]",
        "ldp x25, x26, [sp, #64]",
        "ldp x23, x24, [sp, #48]",
        "ldp x21, x22, [sp, #32]",
        "ldp x19, x20, [sp, #16]",
        "ldp x29, x30, [sp], #{saved}",
        "ret",
        frame = sym HOLE_V0,
        count = sym HOLE_V1,
        rest = sym HOLE_V2,
        first_parameter = sym HOLE_V3,
        exhausted = const EXHAUSTED,
        saved = const SAVED,
        first = sym HOLE_NEXT,
    )
}

/// Returns the frame word at byte offset hole 0 through the result pointer, status 0.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_wasm_word() {
    core::arch::naked_asm!(
        // x0 is the frame: a row passes its frame in x0 to the next row, and so does the leave.
        "movz x9, #:abs_g3:{slot}",
        "movk x9, #:abs_g2_nc:{slot}",
        "movk x9, #:abs_g1_nc:{slot}",
        "movk x9, #:abs_g0_nc:{slot}",
        "ldr x10, [x0, x9]",
        "ldr x9, [x0]",
        "str x10, [x9]",
        "mov w0, #0",
        "ret",
        slot = sym HOLE_V0,
    )
}

/// Returns no result, status 0.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_wasm_void() {
    core::arch::naked_asm!("mov w0, #0", "ret")
}

/// Returns the status a trapping row left in the first pin (p0 = x2).
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_wasm_trap() {
    core::arch::naked_asm!("mov w0, w2", "ret")
}
