//! The b-jit stencil rows, compiled by `build.rs` with the toolchain's rustc into one object.
//! Every row is an `extern "rust-preserve-none"` function, and every successor is a guaranteed
//! tail call (`become`) through a hole (`HOLE_NEXT`, `HOLE_TAKEN`, `HOLE_EXIT0`); no row saves
//! or restores a register. The row ABI below is the register map, assigned by LLVM and never
//! named in a row: x86-64 frame r12, context r13, pinned ints r14, r15, rdi, rsi; AArch64 frame
//! x0, context x1, pinned ints x2..x5 (`preserve_none` has no callee-saved registers, so x19-x28
//! are free scratch, not a home for the pins); pinned doubles xmm0..7 / d0..d7. `HOLE_V<n>` are
//! the value holes the stitcher patches (`words.rs` `hole_word!` says how each target spells one).
//! `build.rs` compiles this file once per shard, each appending
//! its own slice of the generated instances (one row per kind, places and speculation); the
//! target's fixed rows (`entry*.rs`) belong to the shard built with `--cfg fixed_rows`.
#![no_std]
#![feature(explicit_tail_calls, rust_preserve_none_cc)]
#![allow(incomplete_features, non_upper_case_globals, dead_code, unused_imports)]
extern crate core as std;

#[cfg(all(fixed_rows, target_arch = "x86_64"))]
mod call_rows;
mod codes;
#[cfg(all(fixed_rows, target_arch = "x86_64"))]
mod entry;
#[cfg(all(fixed_rows, target_arch = "aarch64"))]
#[path = "entry_a64.rs"]
mod entry;
mod facade;
mod heap_rows;
mod ic_rows;
mod layout;
mod numeric;
mod stage;
mod staged;
mod supernode_rows;
mod words;
pub(crate) use facade::{Value, heap, typed_ir, value, value_vec, vm};

use numeric::{Out, S};

/// The row ABI: (frame, context, p0..p3, f0..f7).
pub(crate) type Row = unsafe extern "rust-preserve-none" fn(usize, usize, usize, usize, usize, usize, f64, f64, f64, f64, f64, f64, f64, f64) -> usize;

unsafe extern "C" {
    static HOLE_NEXT: u8;
    static HOLE_TAKEN: u8;
    static HOLE_EXIT0: u8;
}

macro_rules! go {
    ($hole:ident, $s:ident) => {{
        let next: Row = core::mem::transmute(&raw const $hole);
        become next($s.frame, $s.ctx, $s.p[0], $s.p[1], $s.p[2], $s.p[3], $s.f[0], $s.f[1], $s.f[2], $s.f[3], $s.f[4], $s.f[5], $s.f[6], $s.f[7])
    }};
}

macro_rules! number_row {
    ($name:ident, $k:expr, $d:expr, $a:expr, $b:expr, $spec:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match numeric::run::<{ $k }, { $d }, { $a }, { $b }, { $spec }>(s) {
                    Out::Next(s) => go!(HOLE_NEXT, s),
                    Out::Taken(s) => go!(HOLE_TAKEN, s),
                    Out::Exit(s) => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, s)
                    }
                }
            }
        }
    };
}

macro_rules! heap_row {
    ($name:ident, $k:expr, $entry:expr, $d:expr, $a:expr, $b:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match heap_rows::run::<{ $k }, { $entry }, { $d }, { $a }, { $b }>(s) {
                    Out::Next(s) => go!(HOLE_NEXT, s),
                    Out::Taken(s) => go!(HOLE_TAKEN, s),
                    Out::Exit(s) => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, s)
                    }
                }
            }
        }
    };
}

macro_rules! supernode_row {
    ($name:ident, $first:expr, $k0:expr, $e0:expr, $d0:expr, $a0:expr, $b0:expr, $s0:expr, $k1:expr, $d1:expr, $a1:expr, $b1:expr, $s1:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match supernode_rows::run::<{ $first }, { $k0 }, { $e0 }, { $d0 }, { $a0 }, { $b0 }, { $s0 }, { $k1 }, { $d1 }, { $a1 }, { $b1 }, { $s1 }>(s) {
                    Out::Next(s) => go!(HOLE_NEXT, s),
                    Out::Taken(s) => go!(HOLE_TAKEN, s),
                    Out::Exit(s) => {
                        core::hint::cold_path();
                        go!(HOLE_EXIT0, s)
                    }
                }
            }
        }
    };
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

include!(env!("QUENCH_STENCIL_INSTANCES"));
