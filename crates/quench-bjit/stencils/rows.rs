//! Stencil rows compiled by rustc (`build.rs`), one `extern "rust-preserve-none"` function per
//! row: the frame arrives in r12, the context in r13, the pinned ints in r14, r15, rdi, rsi and
//! the pinned doubles in xmm0..7, every successor is a guaranteed tail call (`become`) through
//! a hole, and no row saves or restores a register. Holes are the addresses of extern statics
//! `HOLE_V<n>` (value hole n, patched into a 32-bit displacement or immediate), and the
//! successors are `HOLE_NEXT`, `HOLE_TAKEN` and `HOLE_EXIT<k>`; `build.rs` turns the object's
//! relocations into the stencil tables and rejects a row that contains `ret` or `call`
//! (only the `leave_*` rows return).
#![no_std]
#![feature(explicit_tail_calls, rust_preserve_none_cc)]
#![allow(incomplete_features, non_upper_case_globals)]

/// The row ABI: (frame, context, p0..p3, f0..f7).
type Row = unsafe extern "rust-preserve-none" fn(usize, usize, usize, usize, usize, usize, f64, f64, f64, f64, f64, f64, f64, f64) -> usize;

unsafe extern "C" {
    static HOLE_V0: u8;
    static HOLE_V1: u8;
    static HOLE_V3: u8;
    static HOLE_NEXT: u8;
}

macro_rules! hole {
    ($symbol:ident) => {
        &$symbol as *const u8 as usize
    };
}

macro_rules! next {
    ($frame:ident, $ctx:ident, $p0:ident, $p1:ident, $p2:ident, $p3:ident, $f0:ident, $f1:ident, $f2:ident, $f3:ident, $f4:ident, $f5:ident, $f6:ident, $f7:ident) => {{
        let next: Row = core::mem::transmute(&HOLE_NEXT as *const u8);
        become next($frame, $ctx, $p0, $p1, $p2, $p3, $f0, $f1, $f2, $f3, $f4, $f5, $f6, $f7)
    }};
}

/// `frame[V0] = entry[V1]`, where `entry` is the loop entry record at the start of the context's
/// adapter payload (`JsStepDispatch::loop_entry`, its first field) and V3 is the byte offset of
/// `CallContext::adapter_payload`.
#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn field_read(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
    unsafe {
        let payload = *((ctx + hole!(HOLE_V3)) as *const usize);
        let entry = *(payload as *const usize);
        *((frame + hole!(HOLE_V0)) as *mut usize) = *((entry + hole!(HOLE_V1)) as *const usize);
        next!(frame, ctx, p0, p1, p2, p3, f0, f1, f2, f3, f4, f5, f6, f7)
    }
}

/// `return frame[0]`: the chain's last row.
#[unsafe(no_mangle)]
pub unsafe extern "rust-preserve-none" fn leave_frame0(frame: usize, _ctx: usize, _p0: usize, _p1: usize, _p2: usize, _p3: usize, _f0: f64, _f1: f64, _f2: f64, _f3: f64, _f4: f64, _f5: f64, _f6: f64, _f7: f64) -> usize {
    unsafe { *(frame as *const usize) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
