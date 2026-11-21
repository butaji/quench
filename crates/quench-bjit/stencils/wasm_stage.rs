//! The stencil machines' implementations of the staged Wasm traits: the primitives the rules of
//! `crate::staged::wasm*` run on, over the frame's pins (`wasm_frame.rs`), the context's global
//! area and grow callout, and the compiled entry a call makes. The semantics stay in the staged
//! rules (every trap, check and order); here is only how a stencil moves bytes and words. Not a
//! rows file: the rows' scanner rule does not apply to these primitives.

use crate::staged::machine::MachineStage;
use crate::staged::wasm::Width;
use crate::staged::wasm_concrete::{ConcreteWasm, delegated, scalar_stage};
use crate::staged::wasm_float::{
    Float, Truncation, copysign_bits, saturated, truncation_limits, unsigned_to_f32, unsigned_to_f64, widen,
};
use crate::staged::wasm_instance::{Storage, WasmInstance};
use crate::staged::wasm_memory::{Access, WasmMemory, WasmMemoryGrow, WasmMemoryWrite};
use crate::staged::wasm_simd_lanes::SimdOperator;
use crate::staged::wasm_simd_rules::{Native, SELECTOR_LANE_BITS, WasmSimd, native_bits, shuffle_bits};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::wasm::{V128_BYTES, WasmTrap};
use crate::words::{HOLE_B, S, hole64, slot};

/// The stencil's vector stage: the shared bodies of `wasm_simd_rules` on the row words (vectors `u128`,
/// scalars `u64`), the native rows, kernels and shuffle the interpreter runs too.
pub(crate) struct StencilSimd;

/// `kernel_bits` for a row: the operator half of the selector is the row's constant, so the lookup
/// folds; the lane half may be a hole, which the emitter admits (`SimdKernel::of`) before it picks the
/// row, so the row does not check it again (a check there would leave a panic call in the row).
#[inline(always)]
fn admitted_bits(selector: u32, left: u128, right: Option<u128>) -> u128 {
    match SimdOperator::KERNELS.get((selector >> SELECTOR_LANE_BITS) as usize) {
        Some(kernel) => kernel.apply_bits(left, right.unwrap_or(0), selector as u8),
        None => 0,
    }
}

impl WasmSimd for StencilSimd {
    type Value = u128;
    type Scalar = u64;

    #[inline(always)]
    fn constant_vector(&mut self, bits: u128) -> u128 {
        bits
    }

    #[inline(always)]
    fn vector_native(&mut self, native: Native, left: u128, right: Option<u128>) -> u128 {
        native_bits(native, left, right.unwrap_or(0))
    }

    #[inline(always)]
    fn vector_kernel(&mut self, selector: u32, left: u128, right: Option<u128>) -> u128 {
        admitted_bits(selector, left, right)
    }

    /// The kernel's bits, the scalar in their low word.
    #[inline(always)]
    fn scalar_kernel(&mut self, selector: u32, left: u128, right: Option<u128>) -> u64 {
        admitted_bits(selector, left, right) as u64
    }

    #[inline(always)]
    fn vector_shuffle(&mut self, lanes: [u8; V128_BYTES], left: u128, right: u128) -> u128 {
        shuffle_bits(left, right, lanes)
    }
}

/// The linear memory a Wasm function runs over: its base in the second pin, its length in bytes
/// in the third, as the entry loads them (`wasm_entry.rs`). Unguarded: the staged rule checks every
/// access against the length, as the interpreter's does.
pub(crate) struct StencilMemory {
    pub(crate) base: *mut u8,
    pub(crate) length: u64,
}

scalar_stage!(StencilMemory);

impl StencilMemory {
    /// The frame's memory over the base and length pins (`s.p[1]`, `s.p[2]`), as the load, store and
    /// bulk rows read them: the entry loads them and `reload_memory` keeps them current after a call.
    #[inline(always)]
    pub(crate) fn of(s: &S) -> Self {
        Self { base: s.p[1] as *mut u8, length: s.p[2] as u64 }
    }

    #[inline(always)]
    pub(crate) fn at(&self, index: u64, offset: u32) -> *mut u8 {
        self.base.wrapping_add((index + u64::from(offset)) as usize)
    }
}

impl WasmMemory for StencilMemory {
    type Vector = u128;

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

    /// The 16 bytes at `index + offset`, one byte at a time: a stencil calls nothing, so no copy.
    #[inline(always)]
    fn memory_load_vector(&mut self, index: u64, offset: u32) -> u128 {
        let at = self.at(index, offset);
        let (mut word, mut byte) = (0u128, 0);
        while byte < V128_BYTES {
            // SAFETY: the rule checked the vector's bytes inside the memory.
            word |= u128::from(unsafe { *at.add(byte) }) << (u8::BITS as usize * byte);
            byte += 1;
        }
        word
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

    /// The 16 bytes of `value` at `index + offset`, one byte at a time (as the load).
    #[inline(always)]
    fn memory_store_vector(&mut self, index: u64, offset: u32, value: u128) {
        let at = self.at(index, offset);
        let mut byte = 0;
        while byte < V128_BYTES {
            // SAFETY: the rule checked the vector's bytes inside the memory.
            unsafe { *at.add(byte) = (value >> (u8::BITS as usize * byte)) as u8 };
            byte += 1;
        }
    }

    /// `length` copies of the low byte of `byte` at `index`; `fill` has checked the range.
    #[inline(always)]
    fn memory_fill(&mut self, index: u64, byte: u64, length: u64) {
        // SAFETY: `fill` checked `index + length` inside the memory.
        unsafe { fill_bytes(self.at(index, 0), byte as u8, length as usize) }
    }

    /// `length` bytes from `source` to `destination` (memmove semantics); `copy` has checked both
    /// ranges.
    #[inline(always)]
    fn memory_copy(&mut self, destination: u64, source: u64, length: u64) {
        // SAFETY: `copy` checked both ranges inside the memory, which `self.base` starts.
        unsafe { copy_bytes(self.at(source, 0), self.at(destination, 0), length as usize) }
    }
}

// A stencil may call nothing outside itself, so the movers cannot be `memset`/`memmove`, which
// LLVM lowers `write_bytes`/`copy` to: x86-64 moves with `rep stosb`/`rep movsb` (fast for any
// length on current cores), and a copy whose destination overlaps past its source runs backward
// one byte at a time; other targets move bytes through volatile accesses, which LLVM keeps.
#[inline(always)]
unsafe fn fill_bytes(destination: *mut u8, byte: u8, length: usize) {
    unsafe {
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!("rep stosb", inout("rdi") destination => _, inout("rcx") length => _, in("al") byte, options(nostack, preserves_flags));
        #[cfg(not(target_arch = "x86_64"))]
        for at in 0..length {
            core::ptr::write_volatile(destination.add(at), byte);
        }
    }
}

#[inline(always)]
unsafe fn copy_bytes(source: *const u8, destination: *mut u8, length: usize) {
    unsafe {
        let overlapping_after = (destination as usize).wrapping_sub(source as usize) < length;
        #[cfg(target_arch = "x86_64")]
        if !overlapping_after {
            core::arch::asm!("rep movsb", inout("rsi") source => _, inout("rdi") destination => _, inout("rcx") length => _, options(nostack, preserves_flags));
            return;
        }
        if overlapping_after {
            for at in (0..length).rev() {
                core::ptr::write_volatile(destination.add(at), core::ptr::read_volatile(source.add(at)));
            }
        } else {
            for at in 0..length {
                core::ptr::write_volatile(destination.add(at), core::ptr::read_volatile(source.add(at)));
            }
        }
    }
}

/// The grow callout the context's `memory_grow` slot holds: (context, pages) -> the size in pages
/// before as a word, or -1 (all ones) where the memory cannot grow. Trap-free.
pub(crate) type Grow = unsafe extern "C" fn(usize, u64) -> u64;

/// The memory a grow row grows: the context's callout, which the staged `WasmMemoryGrow` rule calls
/// for the growth. The growth is the one primitive the rule leaves to its stage.
pub(crate) struct GrowStage {
    context: usize,
    grow: Grow,
}

scalar_stage!(GrowStage);

impl GrowStage {
    /// The stage of the callout hole B's offset names in the context.
    #[inline(always)]
    pub(crate) unsafe fn of(s: &S) -> Self {
        // SAFETY: the stitcher writes hole B as the offset of a `Grow` slot in the context.
        let grow: Grow = unsafe { core::mem::transmute(*((s.ctx + hole64::<HOLE_B>() as usize) as *const usize)) };
        Self { context: s.ctx, grow }
    }
}

impl WasmMemoryGrow for GrowStage {
    #[inline(always)]
    fn memory_grow(&mut self, pages: u64) -> u64 {
        // The pages operand is an i32 word, zero-extended in its frame word.
        // SAFETY: the callout is the context's `memory_grow` slot, which takes the context and the
        // pages and is trap-free.
        let before = unsafe { (self.grow)(self.context, u64::from(pages as u32)) };
        // The result is an i32 word (`WrapI64` in the u-jit), zero-extended as every i32 register.
        self.convert(ScalarConversionOperator::WrapI64, before)
    }
}

/// The instance's globals as a row sees them: the context's global area, which holds one cell
/// pointer per environment slot. Its cells are indexed by environment slot.
pub(crate) struct GlobalStage {
    pub(crate) area: usize,
}

scalar_stage!(GlobalStage);

impl GlobalStage {
    /// The native slot of the global bound at environment slot `slot`.
    #[inline(always)]
    unsafe fn cell(&self, slot: u32) -> *mut u64 {
        // SAFETY: the context's area holds one cell pointer per environment slot the program binds.
        unsafe {
            let area = self.area as *const *const *mut u64;
            *(*area).add(slot as usize)
        }
    }
}

impl WasmInstance for GlobalStage {
    #[inline(always)]
    fn global_load(&mut self, slot: u32, storage: Storage) -> u64 {
        // SAFETY: `cell` is the global's live slot, which the heap never moves.
        storage.narrow(unsafe { *self.cell(slot) })
    }

    #[inline(always)]
    fn global_store(&mut self, slot: u32, storage: Storage, value: u64) {
        // SAFETY: as `global_load`.
        unsafe { *self.cell(slot) = storage.narrow(value) }
    }
}

/// The stage of a constant-free row: `ConcreteWasm`, with the primitives that compare or convert
/// against a float constant computing from integer bits instead (`staged::wasm_float`). The
/// stencil build cannot load a float constant (a constant-pool entry a row cannot relocate).
pub(crate) struct ConstantFree;

impl crate::staged::machine::Machine for ConstantFree {
    type Word = u64;
}

impl MachineStage for ConstantFree {
    type Exit = WasmTrap;

    delegated! {
        constant(width: Width, value: i64) -> u64;
        operate(operator: Binary, width: Width, left: u64, right: u64) -> u64;
        operate_unary(operator: Unary, width: Width, value: u64) -> u64;
        operate_float_unary(operator: FloatUnary, float: Float, value: u64) -> u64;
        float_unordered(float: Float, left: u64, right: u64) -> u64;
        float_constant(float: Float, bits: u64) -> u64;
        select(flag: u64, taken: u64, other: u64) -> u64;
        trap_when(flag: u64, trap: WasmTrap) -> Result<(), WasmTrap>;
    }

    /// `ConcreteWasm::operate_float`, with CopySign as the sign-bit operation (`copysign_bits`):
    /// `f64::copysign` lowers to a sign-mask constant.
    #[inline(always)]
    fn operate_float(&mut self, operator: FloatBinary, float: Float, left: u64, right: u64) -> u64 {
        match operator {
            FloatBinary::CopySign => copysign_bits(float, left, right),
            _ => ConcreteWasm.operate_float(operator, float, left, right),
        }
    }

    /// `ConcreteWasm::truncation_overflows`, with the bounds from bits. `trunc` is `roundsd` on the
    /// stencil targets (x86-64-v2 and up), so it loads no constant.
    #[inline(always)]
    fn truncation_overflows(&mut self, truncation: Truncation, value: u64) -> u64 {
        let truncated = core::f64::math::trunc(widen(truncation.float, value));
        let (lower, limit) = truncation_limits(truncation.width.bits(), truncation.signed);
        u64::from(truncated < lower || truncated >= limit)
    }

    /// `ConcreteWasm::convert`: the float-to-integer conversions through `saturated`, the unsigned
    /// 64-bit conversions through the halving forms, the rest as `ConcreteWasm`.
    #[inline(always)]
    fn convert(&mut self, operator: ScalarConversionOperator, value: u64) -> u64 {
        if let Some((truncation, _)) = operator.float_to_integer() {
            return saturated(widen(truncation.float, value), truncation.width.bits(), truncation.signed);
        }
        match operator {
            ScalarConversionOperator::ConvertI64UnsignedF32 => u64::from(unsigned_to_f32(value).to_bits()),
            ScalarConversionOperator::ConvertI64UnsignedF64 => unsigned_to_f64(value).to_bits(),
            _ => ConcreteWasm.convert(operator, value),
        }
    }
}
