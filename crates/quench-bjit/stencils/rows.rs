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
//! A shard of generated rows also names its family (`--cfg stencil_family = "js"` or `"wasm"`), and only that
//! family's modules are compiled, so its sources (and its cache key in `build.rs`) are its family's alone.
#![no_std]
#![feature(explicit_tail_calls, rust_preserve_none_cc, core_float_math, stmt_expr_attributes)]
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

#[cfg(any(fixed_rows, stencil_family = "call"))]
mod call_rows;
#[cfg(any(fixed_rows, stencil_family = "call"))]
mod direct_frame;
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
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod heap_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod ic_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod environment_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod key_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod name_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod site_rows;
pub mod layout;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod numeric;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_call;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_guard;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_property;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_environment;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_heap;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_view;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod places_names;
#[path = "../../quench-stack/src/projection.rs"]
pub(crate) mod projection;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod stage;
mod staged;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
#[macro_use]
mod staged_rows;
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
mod supernode_rows;
mod wasm;
mod wasm_kinds;
mod wasm_layout;
#[cfg(all(fixed_rows, target_arch = "x86_64"))]
mod wasm_entry;
#[cfg(all(fixed_rows, target_arch = "aarch64", not(target_vendor = "apple")))]
#[path = "wasm_entry_a64.rs"]
mod wasm_entry;
#[cfg(not(any(stencil_family = "js", stencil_family = "regexp")))]
mod wasm_rows;
// The regexp rows (`regexp_rows.rs`): their own family, with their entry and leave routines.
#[cfg(stencil_family = "regexp")]
pub mod regexp_layout;
#[cfg(stencil_family = "regexp")]
mod regexp_stage;
#[cfg(stencil_family = "regexp")]
mod regexp_rows;
#[cfg(all(stencil_family = "regexp", target_arch = "x86_64"))]
mod regexp_entry;
mod words;
pub(crate) use facade::{Value, ValueType, bytecode, float_layout, heap, operations, typed_ir, units, value, value_vec, vm};
pub(crate) use wasm::{WasmTrap, WasmType};

use words::{Out, S};

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

/// A stencil row's entry: the row ABI's arguments as the state `$s`, then `$body`, which goes on
/// to a successor. The one spelling of a row's signature every family's row macro expands to.
macro_rules! row_entry {
    ($name:ident, $s:ident, $body:block) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "rust-preserve-none" fn $name(frame: usize, ctx: usize, p0: usize, p1: usize, p2: usize, p3: usize, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64) -> usize {
            unsafe {
                let $s = S { frame, ctx, p: [p0, p1, p2, p3], f: [f0, f1, f2, f3, f4, f5, f6, f7] };
                $body
            }
        }
    };
}

macro_rules! number_row {
    ($name:ident, $k:expr, $d:expr, $a:expr, $b:expr, $spec:expr) => {
        row_entry!($name, s, {
        match numeric::run::<{ $k }, { $d }, { $a }, { $b }, { $spec }>(s) {
            Out::Next(s) => go!(HOLE_NEXT, s),
            Out::Taken(s) => go!(HOLE_TAKEN, s),
            Out::Exit(s) => {
                core::hint::cold_path();
                go!(HOLE_EXIT0, s)
            }
        }
        });
    };
}

macro_rules! heap_row {
    ($name:ident, $k:expr, $entry:expr, $d:expr, $a:expr, $b:expr) => {
        row_entry!($name, s, {
        match heap_rows::run::<{ $k }, { $entry }, { $d }, { $a }, { $b }>(s) {
            Out::Next(s) => go!(HOLE_NEXT, s),
            Out::Taken(s) => go!(HOLE_TAKEN, s),
            Out::Exit(s) => {
                core::hint::cold_path();
                go!(HOLE_EXIT0, s)
            }
        }
        });
    };
}

macro_rules! supernode_row {
    ($name:ident, $first:ty, $second:ty) => {
        row_entry!($name, s, {
        match supernode_rows::run::<$first, $second>(s) {
            Out::Next(s) => go!(HOLE_NEXT, s),
            Out::Taken(s) => go!(HOLE_TAKEN, s),
            Out::Exit(s) => {
                core::hint::cold_path();
                go!(HOLE_EXIT0, s)
            }
        }
        });
    };
}

macro_rules! wasm_row {
    ($name:ident, $k:expr, $op:expr, $w:expr, $shape:expr) => {
        row_entry!($name, s, {
        match wasm_rows::run::<{ $k }, { $op }, { $w }, { $shape }>(s) {
            Out::Next(s) => go!(HOLE_NEXT, s),
            Out::Taken(s) => go!(HOLE_TAKEN, s),
            Out::Exit(s) => {
                core::hint::cold_path();
                go!(HOLE_EXIT0, s)
            }
        }
        });
    };
}

/// A regexp row: its way on is the next or taken row, the backtrack row (exit 0), or a row by its
/// address (`regexp_stage::Way::Jump`).
macro_rules! regexp_row {
    ($name:ident, $k:expr, $a:expr, $b:expr) => {
        row_entry!($name, s, {
        match regexp_rows::row::<{ $k }, { $a }, { $b }>(s) {
            regexp_stage::Way::Next(s) => go!(HOLE_NEXT, s),
            regexp_stage::Way::Taken(s) => go!(HOLE_TAKEN, s),
            regexp_stage::Way::Fail(s) => go!(HOLE_EXIT0, s),
            regexp_stage::Way::Jump(s, address) => {
                let next: Row = core::mem::transmute(address);
                become next(s.frame, s.ctx, s.p[0], s.p[1], s.p[2], s.p[3], s.f[0], s.f[1], s.f[2], s.f[3], s.f[4], s.f[5], s.f[6], s.f[7])
            }
        }
        });
    };
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

include!(env!("QUENCH_STENCIL_INSTANCES"));
