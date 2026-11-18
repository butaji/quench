//! The Wasm rows over raw frame words: one stencil per (operator, width, operand shape), compiled
//! by LLVM from the staged Wasm rules the interpreter (`ConcreteWasm`) and the u-jit
//! (`Emitter`) run, so a row traps exactly where they do. A Wasm frame holds each register as
//! its machine word (a 32-bit value zero-extended), never a tagged `Value`: the hole of an
//! operand or destination is its byte offset in the frame, the hole of an immediate its word.
//!
//! A row continues (`Out::Next`), branches (`Out::Taken`), or leaves through exit 0 with its
//! status in the first pinned register: a trap's (`WasmTrap::compiled_status`, as the u-jit
//! leaves), which the exit stub returns. Wasm rows pin nothing else.

use crate::staged::machine::MachineStage;
use crate::staged::wasm::{Width, integer_binary};
use crate::staged::wasm_concrete::ConcreteWasm;
use crate::wasm::WasmTrap;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::words::{HOLE_A, HOLE_B, Out, S, hole64, slot};

pub(crate) use crate::wasm_codes::*;

/// The row of codes `K`, `OP`, `W`, `SHAPE` (`wasm_library.rs`).
#[inline(always)]
pub(crate) unsafe fn run<const K: u8, const OP: u32, const W: u8, const SHAPE: u8>(s: S) -> Out {
    unsafe {
        match K {
            BINARY => binary::<OP, W, SHAPE>(s),
            UNARY => unary::<OP, W>(s),
            _ => jump::<OP, W, SHAPE>(s),
        }
    }
}

// A tag outside the family names no row; the dispatch above never reaches it, but its arms are
// still instantiated, so the lookups do not panic.
const fn binary_operator(tag: u32) -> Binary {
    match Binary::from_tag(tag) {
        Some(operator) => operator,
        None => Binary::Add,
    }
}

const fn unary_operator(tag: u32) -> Unary {
    match Unary::from_tag(tag) {
        Some(operator) => operator,
        None => Unary::EqualZero,
    }
}

const fn width_of(width: u8) -> Width {
    if width == W32 { Width::I32 } else { Width::I64 }
}

#[inline(always)]
unsafe fn read<const H: u8>(s: &S) -> u64 {
    unsafe { *(slot::<H>(s) as *const u64) }
}

#[inline(always)]
unsafe fn write<const H: u8>(s: &S, word: u64) {
    unsafe { *(slot::<H>(s) as *mut u64) = word }
}

/// The second operand: a register's word, or the immediate the row's hole holds.
#[inline(always)]
unsafe fn right<const SHAPE: u8>(s: &S) -> u64 {
    if SHAPE == IMMEDIATE { hole64::<HOLE_B>() } else { unsafe { read::<HOLE_B>(s) } }
}

/// Leaves with `trap`'s status.
#[inline(always)]
fn trap(mut s: S, trap: WasmTrap) -> Out {
    s.p[0] = match trap.compiled_status() {
        Some(status) => status as usize,
        None => 0,
    };
    Out::Exit(s)
}

/// `D = A op B` at width `W`: the staged integer rule, its traps included.
#[inline(always)]
pub(crate) unsafe fn binary<const OP: u32, const W: u8, const SHAPE: u8>(s: S) -> Out {
    let operator = const { binary_operator(OP) };
    unsafe {
        let (left, right) = (read::<HOLE_A>(&s), right::<SHAPE>(&s));
        match integer_binary(&mut ConcreteWasm, operator, const { width_of(W) }, left, right) {
            Ok(word) => {
                write::<0>(&s, word);
                Out::Next(s)
            }
            Err(error) => trap(s, error),
        }
    }
}

/// `D = op A` at width `W`.
#[inline(always)]
pub(crate) unsafe fn unary<const OP: u32, const W: u8>(s: S) -> Out {
    let operator = const { unary_operator(OP) };
    unsafe {
        let word = ConcreteWasm.operate_unary(operator, const { width_of(W) }, read::<HOLE_A>(&s));
        write::<0>(&s, word);
    }
    Out::Next(s)
}

/// Branches where the comparison `A op B` at width `W` holds.
#[inline(always)]
pub(crate) unsafe fn jump<const OP: u32, const W: u8, const SHAPE: u8>(s: S) -> Out {
    let operator = const { binary_operator(OP) };
    unsafe {
        let (left, right) = (read::<HOLE_A>(&s), right::<SHAPE>(&s));
        if ConcreteWasm.operate(operator, const { width_of(W) }, left, right) != 0 { Out::Taken(s) } else { Out::Next(s) }
    }
}
