//! The Wasm rows over raw frame words: one stencil per (operator, width, operand shape), compiled
//! by LLVM from the staged Wasm rules the interpreter (`ConcreteWasm`) and the u-jit
//! (`Emitter`) run, so a row traps exactly where they do. A Wasm frame holds each register as
//! its machine word (a 32-bit value zero-extended), never a tagged `Value`: the hole of an
//! operand or destination is its byte offset in the frame, the hole of an immediate its word.
//!
//! A row continues (`Out::Next`), branches (`Out::Taken`), or leaves through exit 0 with its
//! status in the first pinned register: a trap's (`WasmTrap::compiled_status`, as the u-jit
//! leaves), which the exit stub returns. Wasm rows pin nothing else.
//!
//! The rows call the staged rules: the operator decoders are `staged::wasm*`, the frame accessors
//! `wasm_frame.rs`, the stage primitives `wasm_stage.rs`. The one row with no staged rule is
//! `call_wasm`: a call enters compiled code, which no staged rule describes.

use crate::staged::machine::MachineStage;
use crate::staged::wasm::{binary_operator, integer_binary, unary_operator};
use crate::staged::wasm_concrete::ConcreteWasm;
use crate::staged::wasm_float::{float_binary, float_binary_operator, float_unary_operator};
use crate::staged::wasm_memory::{load, store};
use crate::wasm::WasmTrap;
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole, hole64};

pub(crate) use crate::wasm_codes::*;

#[path = "wasm_frame.rs"]
pub(crate) mod wasm_frame;
use wasm_frame::{access, float_of, read, reload_memory, right, trap, width_of, write};
#[path = "wasm_stage.rs"]
mod wasm_stage;
pub(crate) use wasm_stage::StencilMemory;
use wasm_stage::call_entry;
#[path = "wasm_call_stage.rs"]
mod wasm_call_stage;
#[path = "wasm_rows_memory.rs"]
mod wasm_rows_memory;
use wasm_rows_memory::{memory_copy, memory_fill, memory_size};
#[path = "wasm_rows_global.rs"]
mod wasm_rows_global;
use wasm_rows_global::{global_get, global_set};
#[path = "wasm_rows_indirect.rs"]
mod indirect;
#[path = "wasm_rows_simd.rs"]
mod simd;
#[path = "wasm_rows_simd_more.rs"]
mod simd_more;
#[path = "wasm_rows_convert.rs"]
mod wasm_rows_convert;
use wasm_rows_convert::{COPYSIGN_TAG, convert_row, copysign_row};
#[path = "wasm_rows_instance.rs"]
mod wasm_rows_instance;
use wasm_rows_instance::call_memory_grow;
#[path = "wasm_rows_call_vm.rs"]
mod wasm_rows_call_vm;
use wasm_rows_call_vm::call_vm;
#[path = "wasm_rows_callout.rs"]
mod wasm_rows_callout;
use wasm_rows_callout::{callout, gc_allocate};
#[path = "wasm_rows_roots.rs"]
mod wasm_rows_roots;
use wasm_rows_roots::{root_pop, root_push};

/// The row of codes `K`, `OP`, `W`, `SHAPE` (`wasm_library.rs`).
#[inline(always)]
pub(crate) unsafe fn run<const K: u8, const OP: u32, const W: u8, const SHAPE: u8>(s: S) -> Out {
    unsafe {
        match K {
            BINARY => binary::<OP, W, SHAPE>(s),
            UNARY => unary::<OP, W>(s),
            JUMP_IF => jump::<OP, W, SHAPE>(s),
            MOVE => {
                write::<0>(&s, read::<HOLE_A>(&s));
                Out::Next(s)
            }
            LOAD => memory_load::<OP>(s),
            CALL => call_wasm(s),
            CALL_VM => call_vm(s),
            CALL_INDIRECT => indirect::call_indirect(s),
            GLOBAL_GET => global_get::<OP>(s),
            GLOBAL_SET => global_set::<OP>(s),
            FLOAT_BINARY if OP == COPYSIGN_TAG => copysign_row::<W>(s),
            FLOAT_BINARY => {
                let operator = const { float_binary_operator(OP) };
                let word = float_binary(&mut ConcreteWasm, operator, const { float_of(W) }, read::<HOLE_A>(&s), read::<HOLE_B>(&s));
                write::<0>(&s, word);
                Out::Next(s)
            }
            FLOAT_UNARY => {
                let operator = const { float_unary_operator(OP) };
                let word = ConcreteWasm.operate_float_unary(operator, const { float_of(W) }, read::<HOLE_A>(&s));
                write::<0>(&s, word);
                Out::Next(s)
            }
            CONVERT => convert_row::<OP>(s),
            SELECT => {
                // The condition is the i32 word in hole FOURTH: `a = (condition ? b : c)`.
                let condition = u64::from(read::<HOLE_FOURTH>(&s) as u32);
                write::<0>(&s, ConcreteWasm.select(condition, read::<HOLE_A>(&s), read::<HOLE_B>(&s)));
                Out::Next(s)
            }
            TRAP => trap(s, WasmTrap::Unreachable),
            STORE => memory_store::<OP>(s),
            CONSTANT => {
                write::<0>(&s, hole64::<HOLE_B>());
                Out::Next(s)
            }
            MEMORY_SIZE => memory_size(s),
            MEMORY_FILL => memory_fill(s),
            MEMORY_COPY => memory_copy(s),
            MEMORY_GROW => call_memory_grow(s),
            CALLOUT => callout::<OP>(s),
            GC_ALLOCATE => gc_allocate(s),
            ROOT_PUSH => root_push(s),
            ROOT_POP => root_pop(s),
            SIMD_BINARY => simd::binary::<OP>(s),
            SIMD_UNARY => simd::unary::<OP>(s),
            SIMD_SHUFFLE => simd::shuffle(s),
            SIMD_CONST => simd::constant(s),
            SIMD_LOAD => simd::load(s),
            SIMD_STORE => simd::store(s),
            SIMD_SPLAT => simd::splat::<OP>(s),
            SIMD_EXTRACT => simd::extract::<OP>(s),
            SIMD_REPLACE => simd::replace::<OP>(s),
            SIMD_MOVE => simd::moves(s),
            SIMD_MORE_BINARY => simd_more::binary::<OP>(s),
            SIMD_MORE_COMPARE => simd_more::compare::<OP>(s),
            SIMD_MORE_FLOAT => simd_more::float::<OP>(s),
            SIMD_MORE_SHIFT => simd_more::shift::<OP>(s),
            SIMD_MORE_TEST => simd_more::test::<OP>(s),
            SIMD_MORE_UNARY => simd_more::unary::<OP>(s),
            SIMD_MORE_WIDEN => simd_more::widen::<OP>(s),
            SIMD_MORE_WIDE => simd_more::wide::<OP>(s),
            SIMD_MORE_CONVERT => simd_more::convert::<OP>(s),
            SIMD_MORE_LOAD => simd_more::load::<OP>(s),
            _ => Out::Taken(s),
        }
    }
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
        // A comparison traps on nothing (`integer_binary` checks only divisions), so its flag is the word.
        let flag = integer_binary(&mut ConcreteWasm, operator, const { width_of(W) }, left, right).unwrap_or(0);
        if flag != 0 { Out::Taken(s) } else { Out::Next(s) }
    }
}

/// `D = memory[A + B]` of the access `OP` names: the staged rule's check and access.
#[inline(always)]
unsafe fn memory_load<const OP: u32>(s: S) -> Out {
    let access = const { access(OP) };
    let mut memory = StencilMemory::of(&s);
    unsafe {
        match load(&mut memory, access, read::<HOLE_A>(&s), hole::<HOLE_B>() as u32, false) {
            Ok(word) => {
                write::<0>(&s, word);
                Out::Next(s)
            }
            Err(error) => trap(s, error),
        }
    }
}

/// `memory[A + B] = D` of the access `OP` names.
#[inline(always)]
unsafe fn memory_store<const OP: u32>(s: S) -> Out {
    let access = const { access(OP) };
    let mut memory = StencilMemory::of(&s);
    unsafe {
        match store(&mut memory, access, read::<HOLE_A>(&s), hole::<HOLE_B>() as u32, read::<0>(&s), false) {
            Ok(()) => Out::Next(s),
            Err(error) => trap(s, error),
        }
    }
}

/// Calls the entry in the cell at hole B's address (`call_entry`): the arguments are the registers
/// from hole A on, the result goes to hole 0's register, and a trap's status leaves. The call
/// clobbers the pins; afterwards the memory's length is the context's (`reload_memory`, hole
/// `HOLE_FOURTH`).
#[inline(always)]
unsafe fn call_wasm(mut s: S) -> Out {
    unsafe {
        let status = call_entry(&s);
        reload_memory(&mut s, hole64::<HOLE_FOURTH>() as usize);
        if status != 0 {
            s.p[0] = status as usize;
            return Out::Exit(s);
        }
        Out::Next(s)
    }
}
