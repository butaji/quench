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
use crate::staged::wasm_concrete::{ConcreteWasm, scalar_stage};
use crate::staged::wasm_memory::{Access, WasmMemory, WasmMemoryWrite, load, store};
use crate::wasm::WasmType;
use crate::staged::wasm_float::{Float, convert, float_binary};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::WasmTrap;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, Out, S, hole, hole64, slot};

pub(crate) use crate::wasm_codes::*;

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
use wasm_rows_instance::memory_grow;
#[path = "wasm_rows_call_vm.rs"]
mod wasm_rows_call_vm;
use wasm_rows_call_vm::call_vm;
#[path = "wasm_rows_callout.rs"]
mod wasm_rows_callout;
use wasm_rows_callout::callout;

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
            CALL => call(s),
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
                let taken = read::<HOLE_FOURTH>(&s) as u32 != 0;
                write::<0>(&s, if taken { read::<HOLE_A>(&s) } else { read::<HOLE_B>(&s) });
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
            MEMORY_GROW => memory_grow(s),
            CALLOUT => callout::<OP>(s),
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

// A tag outside the family names no row; the dispatch above never reaches it, but its arms are
// still instantiated, so the lookups do not panic.
const fn binary_operator(tag: u32) -> Binary {
    match Binary::from_tag(tag) {
        Some(operator) => operator,
        None => Binary::Add,
    }
}

const fn float_binary_operator(tag: u32) -> FloatBinary {
    match FloatBinary::from_tag(tag) {
        Some(operator) => operator,
        None => FloatBinary::Add,
    }
}

const fn float_unary_operator(tag: u32) -> FloatUnary {
    match FloatUnary::from_tag(tag) {
        Some(operator) => operator,
        None => FloatUnary::Absolute,
    }
}

const fn conversion(tag: u32) -> ScalarConversionOperator {
    match ScalarConversionOperator::from_tag(tag) {
        Some(operator) => operator,
        None => match ScalarConversionOperator::from_tag(0) {
            Some(operator) => operator,
            None => unreachable!(),
        },
    }
}

const fn float_of(width: u8) -> Float {
    if width == W32 { Float::F32 } else { Float::F64 }
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

/// The access the operator code `index` names (`ACCESSES`).
const fn access(index: u32) -> Access {
    let (ty, bytes, signed) = ACCESSES[if (index as usize) < ACCESSES.len() { index as usize } else { 0 }];
    let ty = match ty {
        T_I32 => WasmType::I32,
        T_I64 => WasmType::I64,
        T_F32 => WasmType::F32,
        _ => WasmType::F64,
    };
    Access { ty, bytes, signed }
}

/// The linear memory a Wasm function runs over: its base in the second pin, its length in
/// bytes in the third, as the entry loads them (`wasm_entry.rs`). Unguarded: the staged rule
/// checks every access against the length, as the interpreter's does.
struct StencilMemory {
    base: *mut u8,
    length: u64,
}

impl StencilMemory {
    #[inline(always)]
    fn of(s: &S) -> Self {
        Self { base: s.p[1] as *mut u8, length: s.p[2] as u64 }
    }

    #[inline(always)]
    fn at(&self, index: u64, offset: u32) -> *mut u8 {
        self.base.wrapping_add((index + u64::from(offset)) as usize)
    }
}

scalar_stage!(StencilMemory);

impl WasmMemory for StencilMemory {
    fn guarded(&self) -> bool {
        false
    }

    #[inline(always)]
    fn memory_length(&mut self) -> u64 {
        self.length
    }

    /// A 32-bit address word is already its zero-extended index.
    #[inline(always)]
    fn memory_index(&mut self, address: u64) -> u64 {
        address
    }

    #[inline(always)]
    fn memory_load(&mut self, access: Access, index: u64, offset: u32) -> u64 {
        let at = self.at(index, offset);
        // SAFETY: the rule checked the access's bytes inside the memory.
        let raw = unsafe {
            match access.bytes {
                1 => u64::from(*at),
                2 => u64::from((at as *const u16).read_unaligned()),
                4 => u64::from((at as *const u32).read_unaligned()),
                _ => (at as *const u64).read_unaligned(),
            }
        };
        access.loaded(raw)
    }
}

impl WasmMemoryWrite for StencilMemory {
    #[inline(always)]
    fn memory_store(&mut self, access: Access, index: u64, offset: u32, value: u64) {
        let at = self.at(index, offset);
        // SAFETY: the rule checked the access's bytes inside the memory.
        unsafe {
            match access.bytes {
                1 => *at = value as u8,
                2 => (at as *mut u16).write_unaligned(value as u16),
                4 => (at as *mut u32).write_unaligned(value as u32),
                _ => (at as *mut u64).write_unaligned(value),
            }
        }
    }

    // The bulk movers have no rows; a stitched function never runs them.
    fn memory_fill(&mut self, _index: u64, _byte: u64, _length: u64) {}

    fn memory_copy(&mut self, _destination: u64, _source: u64, _length: u64) {}
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

/// The u-jit's compiled-function ABI, which every Wasm function's entry takes, stitched or not.
type WasmEntry = unsafe extern "C" fn(usize, u64, u64, *const u64, *mut u64) -> u32;

/// The frame header's words (`FRAME_HEADER_WORDS`): the memory base, then the memory length.
const BASE_WORD: usize = 1;
const LENGTH_WORD: usize = 2;
const _: () = assert!(crate::wasm_codes::FRAME_HEADER_WORDS as usize > LENGTH_WORD);

/// After a call (or a VM callout) that may have grown the memory: the frame's length word takes
/// the context's current length, read at `length_at` (`CONTEXT_MEMORY_LENGTH`, a hole), since a
/// callee's growth refreshes the context and not this frame's header. The base is reloaded from
/// the header; it never moves under a call (`World::memory_stable` admits calls only where it
/// cannot), and the pins are reloaded from the header afterwards.
#[inline(always)]
pub(crate) unsafe fn reload_memory(s: &mut S, length_at: usize) {
    unsafe {
        let header = s.frame as *mut usize;
        *header.add(LENGTH_WORD) = *((s.ctx + length_at) as *const u64) as usize;
        s.p[1] = *header.add(BASE_WORD);
        s.p[2] = *header.add(LENGTH_WORD);
    }
}

/// Calls the entry in the cell at hole B's address: the arguments are the registers from hole A
/// on, the result goes to hole 0's register, and a trap's status leaves. The call clobbers the
/// pins; afterwards the memory's length is the context's (`reload_memory`, hole `HOLE_FOURTH`).
#[inline(always)]
unsafe fn call(mut s: S) -> Out {
    unsafe {
        let entry: WasmEntry = core::mem::transmute(*(hole64::<HOLE_B>() as *const usize));
        let status = entry(s.ctx, s.p[1] as u64, s.p[2] as u64, slot::<HOLE_A>(&s) as *const u64, slot::<0>(&s) as *mut u64);
        reload_memory(&mut s, hole64::<HOLE_FOURTH>() as usize);
        if status != 0 {
            s.p[0] = status as usize;
            return Out::Exit(s);
        }
        Out::Next(s)
    }
}
