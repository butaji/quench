//! AArch64 forms of the routines around a region's rows (`entry.rs` has the x86-64 ones): the
//! entry thunk and the two leave routines, with the same names, entry ABI and contract.
//!
//! `entry_thunk` and `leave_word` are ordinary Rust functions: the compiler saves what the C ABI
//! requires around the call into the row chain (`preserve_none` clobbers every register) and
//! places the first row's arguments and the returned word itself, so no register of the row
//! convention is named here. Only `leave_pop` is assembly: it pops the PC an exit stub pushed
//! (`quench_bjit::stub`) into x0, the register AAPCS64 and `preserve_none` return a word in.
//!
//! Entry ABI: (x0 = context, x3 = the frame record, x4 = result pointer), status `NATIVE_JS_STOPPED` in w0.

use crate::layout::NATIVE_JS_STOPPED;
use crate::staged::frame;
use crate::words::hole64;
use crate::Row;

unsafe extern "C" {
    static HOLE_NEXT: u8;
}

/// Bytes an exit stub pushes: one 16-byte slot, which keeps `sp` 16-byte aligned.
const STUB_SLOT: usize = 16;

/// Loads the frame's window of registers, runs the row chain, stores the word it returned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn entry_thunk(ctx: usize, _unused0: usize, _unused1: usize, frame: usize, result: *mut usize) -> u32 {
    // SAFETY: the stitched code places the first row at `HOLE_NEXT`; `frame` is a frame record.
    unsafe {
        let first: Row = core::mem::transmute(&raw const HOLE_NEXT);
        let window = ((frame + frame::WINDOW) as *const usize).read();
        result.write(first(window, ctx, 0, 0, 0, 0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
    }
    NATIVE_JS_STOPPED
}

/// Returns hole 0 (a word the region leaves with: a PC or a lane exit).
#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn leave_word(_frame: usize, _ctx: usize, _p0: usize, _p1: usize, _p2: usize, _p3: usize, _f0: f64, _f1: f64, _f2: f64, _f3: f64, _f4: f64, _f5: f64, _f6: f64, _f7: f64) -> usize {
    hole64::<0>() as usize
}

/// Returns the word an exit stub pushed.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn leave_pop() {
    core::arch::naked_asm!("ldr x0, [sp], #{slot}", "ret", slot = const STUB_SLOT)
}
