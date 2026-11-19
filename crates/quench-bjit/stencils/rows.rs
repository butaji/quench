//! The b-jit stencil rows, compiled by `build.rs` with the toolchain's rustc into one object.
//! Every row is an `extern "rust-preserve-none"` function, and every successor is a guaranteed
//! tail call (`become`) through a hole (`HOLE_NEXT`, `HOLE_TAKEN`, `HOLE_EXIT0`); no row saves
//! or restores a register. The row ABI below is the register map, assigned by LLVM and never
//! named in a row: x86-64 frame r12, context r13, pinned ints r14, r15, rdi, rsi; AArch64 frame
//! x20, context x21, pinned ints x22..x25 (`CC_AArch64_Preserve_None` assigns x20..x28 first); pinned doubles xmm0..7 / d0..d7. `HOLE_V<n>` are
//! the value holes the stitcher patches (`words.rs` `hole_word!` says how each target spells one).
//! `build.rs` compiles this file once per shard, each appending
//! its own slice of the generated instances (one row per kind, places and speculation); the
//! target's fixed rows (`entry*.rs`) belong to the shard built with `--cfg fixed_rows`.
#![no_std]
#![feature(explicit_tail_calls, rust_preserve_none_cc, core_float_math)]
#![allow(incomplete_features, non_upper_case_globals, unused_imports)]
extern crate core as std;

/// The row's staged work's word, or the row leaves through exit 0 with nothing written.
macro_rules! need {
    ($s:expr, $e:expr) => {
        match $e {
            Some(v) => v,
            None => return Out::Exit($s),
        }
    };
}

#[cfg(fixed_rows)]
mod call_rows;
#[macro_use]
#[path = "row_catalog.rs"]
mod row_catalog;
use codes::TYPED_KINDS;
pub mod codes;
pub mod site_bits;
pub mod row_names;
#[cfg(all(fixed_rows, target_arch = "x86_64"))]
mod entry;
#[cfg(all(fixed_rows, target_arch = "aarch64"))]
#[path = "entry_a64.rs"]
mod entry;
mod facade;
mod heap_rows;
mod ic_rows;
mod environment_rows;
mod key_rows;
mod name_rows;
mod site_rows;
pub mod layout;
mod numeric;
mod places;
mod places_call;
mod places_guard;
mod places_property;
mod places_environment;
mod places_heap;
mod places_view;
mod places_names;
mod stage;
mod staged;
#[macro_use]
mod staged_rows;
mod supernode_rows;
mod wasm;
mod wasm_kinds;
mod wasm_layout;
#[cfg(all(fixed_rows, target_arch = "x86_64"))]
mod wasm_entry;
#[cfg(all(fixed_rows, target_arch = "aarch64", not(target_vendor = "apple")))]
#[path = "wasm_entry_a64.rs"]
mod wasm_entry;
mod wasm_rows;
mod words;
pub(crate) use facade::{Value, ValueType, bytecode, heap, operations, typed_ir, units, value, value_vec, vm};
pub(crate) use wasm::{WasmTrap, WasmType};

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
    ($name:ident, $first:ty, $second:ty) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match supernode_rows::run::<$first, $second>(s) {
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

macro_rules! wasm_row {
    ($name:ident, $k:expr, $op:expr, $w:expr, $shape:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                match wasm_rows::run::<{ $k }, { $op }, { $w }, { $shape }>(s) {
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
