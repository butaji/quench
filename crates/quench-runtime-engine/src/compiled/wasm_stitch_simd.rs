//! The b-jit's vector steps: each `Simd` step, the v128 constants and the v128 moves of a stitched
//! function, planned as the rows of `quench-bjit/stencils/wasm_rows_simd.rs` (kinds `SIMD_*`).
//!
//! Representation: a vector register is two frame words, low word first, in the vector area past
//! the scalar registers: register `r` at `vector_slot(registers, r) = slot(registers) + 16 * r`.
//! That is the u-jit's bits (a `VECTOR_IR` value, low word first, as `compiled_simd_kernel` reads
//! them), not the interpreter's frame word: a Value handle to a boxed vector. The b-jit frame needs
//! no handle because no v128 crosses a stitched function's frame. The u-jit admits no v128
//! parameter, result, call window or return (`Kind::of` is `None` for V128), and `stitched_entry`
//! stitches only a function its u-jit compiled, so every v128 lives inside the function.
//!
//! A scalar register's word is `slot(r)` and a vector's is in the vector area, so one register
//! number used as both never aliases. A scalar step's operand is typed by its instruction, a vector
//! step's likewise, and a move is the one step whose type comes from the u-jit's plan
//! (`World::vector_move`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::wasm_stitch_simd_more;
use super::{Planned, WasmCode, World, codes, leak, row, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_instance::Instance;
use quench_runtime::staged::wasm_simd::Simd;
use quench_runtime::staged::wasm_step::Step;
use quench_runtime::wasm::V128_BYTES;
use quench_runtime::wasm::integer::I64BinaryOperator as Binary;
use quench_runtime::wasm::simd::SimdOperator;
use quench_bjit::padded_holes;

/// The frame byte of vector register `register` in a function of `registers` registers: its low
/// word (the high word is 8 more).
pub(crate) const fn vector_slot(registers: u16, register: u16) -> u64 {
    slot(registers) + 16 * register as u64
}

/// The frame's bytes of a function of `registers` registers: the scalar frame (`frame_bytes`), and
/// with vectors the vector area after it, padded as `frame_bytes` pads.
pub(crate) fn frame_bytes(registers: u16, vectors: bool) -> u64 {
    if !vectors {
        return super::frame_bytes(registers);
    }
    super::padded_frame(vector_slot(registers, registers))
}

/// A vector's bytes as its two words, low first (the little-endian bits of the u128).
fn words(bytes: [u8; V128_BYTES]) -> [u64; 2] {
    let mut low = [0u8; 8];
    let mut high = [0u8; 8];
    low.copy_from_slice(&bytes[..8]);
    high.copy_from_slice(&bytes[8..]);
    [u64::from_le_bytes(low), u64::from_le_bytes(high)]
}

/// A row of kind `kind` and operator `operator` (shape register, width 64: the library's row of
/// `wasm_library.rs`), its value holes as the row reads them: hole 0 the destination, then the operands
/// in `HOLE_A`, `HOLE_B`, `HOLE_FOURTH`, `LAST_HOLE`.
pub(crate) fn vector_row<const N: usize>((kind, operator): (u8, u32), holes: [u64; N]) -> Planned {
    let code: WasmCode = (kind, operator, codes::W64, codes::REGISTER);
    Planned { stencil: leak(codes::wasm_name(code)), holes: padded_holes(holes), taken: None }
}

/// The rows of the vector step `step` at `instruction`, or `None` where the step has none: an
/// operator outside the library's kinds, or a shuffle whose indices are not a vector constant.
pub(crate) fn simd(instruction: WideInstruction, step: Simd, world: &World<'_>) -> Option<Vec<Planned>> {
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    let vector = |register: u16| vector_slot(world.registers, register);
    let planned = match step {
        Simd::Operator { selector } => {
            let (operator, lane) = SimdOperator::from_selector(selector)?;
            let tag = operator as u32;
            if codes::SIMD_UNARY_OPS.contains(&tag) {
                vector_row((codes::SIMD_UNARY, tag), [vector(a), vector(b)])
            } else if codes::SIMD_BINARY_OPS.contains(&tag) {
                vector_row((codes::SIMD_BINARY, tag), [vector(a), vector(b), vector(c)])
            } else if codes::SIMD_SPLAT_OPS.contains(&tag) {
                vector_row((codes::SIMD_SPLAT, tag), [vector(a), slot(b)])
            } else if codes::SIMD_EXTRACT_OPS.contains(&tag) {
                vector_row((codes::SIMD_EXTRACT, tag), [slot(a), vector(b), u64::from(lane)])
            } else if codes::SIMD_REPLACE_OPS.contains(&tag) {
                vector_row((codes::SIMD_REPLACE, tag), [vector(a), vector(b), slot(c), u64::from(lane)])
            } else {
                return None;
            }
        }
        Simd::Shuffle => {
            let [low, high] = words((world.vector_constant)(instruction.constant_index())?);
            vector_row((codes::SIMD_SHUFFLE, 0), [vector(a), vector(b), vector(c), low, high])
        }
        // `a = b + offset`: the 32-bit address (zero-extended in its word) plus the static offset,
        // as the existing 64-bit immediate add.
        Simd::Address => {
            let offset = (world.constant)(instruction.constant_index())?;
            u32::try_from(offset).ok()?;
            return Some(vec![row((codes::BINARY, Binary::Add as u32, codes::W64, codes::IMMEDIATE), [slot(a), slot(b), offset], None)]);
        }
        Simd::Load => vector_row((codes::SIMD_LOAD, 0), [vector(a), slot(c)]),
        Simd::Store => vector_row((codes::SIMD_STORE, 0), [vector(a), slot(c)]),
    };
    Some(vec![planned])
}

/// The rows of a v128 constant or a move of vectors at `pc`, or `None` for any other instance step
/// (the caller plans a scalar one): `plan()` tries this first.
pub(crate) fn instance(pc: usize, instruction: WideInstruction, step: Instance, world: &World<'_>) -> Option<Vec<Planned>> {
    let (a, b) = (instruction.a(), instruction.b());
    if step == Instance::Move && (world.vector_move)(pc) {
        let (to, from) = (vector_slot(world.registers, a), vector_slot(world.registers, b));
        return Some(vec![vector_row((codes::SIMD_MOVE, 0), [to, from])]);
    }
    let registers = step.constant_registers(instruction)?;
    let [low, high] = words((world.vector_constant)(instruction.constant_index())?);
    Some(registers.map(|register| vector_row((codes::SIMD_CONST, 0), [vector_slot(world.registers, register), low, high])).collect())
}

/// Whether a function holds a vector: a vector step, a v128 constant or a move of vectors. Its
/// frame then has the vector area (`frame_bytes`).
pub(crate) fn uses_vectors(instructions: &[WideInstruction], world: &World<'_>) -> bool {
    instructions.iter().enumerate().any(|(pc, &instruction)| match Step::of(instruction.op(), instruction.imm()) {
        Some(Step::Simd(_)) => true,
        Some(Step::Instance(step)) => {
            (step == Instance::Move && (world.vector_move)(pc))
                || (step.constant_registers(instruction).is_some() && (world.vector_constant)(instruction.constant_index()).is_some())
        }
        _ => wasm_stitch_simd_more::is_vector_load(instruction),
    })
}

/// Each operator tag `wasm_runtime_codes.rs` names is the runtime's `SimdOperator` discriminant, checked at
/// compile time: a reordered operator table fails the build here, not in a row.
macro_rules! tags {
    ($($operator:ident = $code:ident;)*) => {
        $(const _: () = assert!(SimdOperator::$operator as u32 == codes::$code);)*
    };
}
tags! {
    V128Not = SIMD_V128_NOT;
    V128And = SIMD_V128_AND;
    V128AndNot = SIMD_V128_ANDNOT;
    V128Or = SIMD_V128_OR;
    V128Xor = SIMD_V128_XOR;
    I8x16Swizzle = SIMD_I8X16_SWIZZLE;
    I8x16Splat = SIMD_I8X16_SPLAT;
    I16x8Splat = SIMD_I16X8_SPLAT;
    I32x4Splat = SIMD_I32X4_SPLAT;
    I64x2Splat = SIMD_I64X2_SPLAT;
    F32x4Splat = SIMD_F32X4_SPLAT;
    F64x2Splat = SIMD_F64X2_SPLAT;
    I8x16ExtractLaneS = SIMD_I8X16_EXTRACT_S;
    I8x16ExtractLaneU = SIMD_I8X16_EXTRACT_U;
    I16x8ExtractLaneS = SIMD_I16X8_EXTRACT_S;
    I16x8ExtractLaneU = SIMD_I16X8_EXTRACT_U;
    I32x4ExtractLane = SIMD_I32X4_EXTRACT;
    I64x2ExtractLane = SIMD_I64X2_EXTRACT;
    F32x4ExtractLane = SIMD_F32X4_EXTRACT;
    F64x2ExtractLane = SIMD_F64X2_EXTRACT;
    I8x16ReplaceLane = SIMD_I8X16_REPLACE;
    I16x8ReplaceLane = SIMD_I16X8_REPLACE;
    I32x4ReplaceLane = SIMD_I32X4_REPLACE;
    I64x2ReplaceLane = SIMD_I64X2_REPLACE;
    F32x4ReplaceLane = SIMD_F32X4_REPLACE;
    F64x2ReplaceLane = SIMD_F64X2_REPLACE;
    F32x4Add = SIMD_F32X4_ADD;
    F32x4Sub = SIMD_F32X4_SUB;
    F32x4Mul = SIMD_F32X4_MUL;
    F32x4Div = SIMD_F32X4_DIV;
    F32x4Eq = SIMD_F32X4_EQ;
    F32x4Ne = SIMD_F32X4_NE;
    F32x4Lt = SIMD_F32X4_LT;
    F32x4Gt = SIMD_F32X4_GT;
    F32x4Le = SIMD_F32X4_LE;
    F32x4Ge = SIMD_F32X4_GE;
    F32x4Abs = SIMD_F32X4_ABS;
    F32x4Neg = SIMD_F32X4_NEG;
    F64x2Add = SIMD_F64X2_ADD;
    F64x2Sub = SIMD_F64X2_SUB;
    F64x2Mul = SIMD_F64X2_MUL;
    F64x2Div = SIMD_F64X2_DIV;
    F64x2Eq = SIMD_F64X2_EQ;
    F64x2Ne = SIMD_F64X2_NE;
    F64x2Lt = SIMD_F64X2_LT;
    F64x2Gt = SIMD_F64X2_GT;
    F64x2Le = SIMD_F64X2_LE;
    F64x2Ge = SIMD_F64X2_GE;
    F64x2Abs = SIMD_F64X2_ABS;
    F64x2Neg = SIMD_F64X2_NEG;
    I8x16Eq = SIMD_I8X16_EQ;
    I8x16Ne = SIMD_I8X16_NE;
    I8x16LtS = SIMD_I8X16_LT_S;
    I8x16GtS = SIMD_I8X16_GT_S;
    I8x16LeS = SIMD_I8X16_LE_S;
    I8x16GeS = SIMD_I8X16_GE_S;
    I8x16LtU = SIMD_I8X16_LT_U;
    I8x16GtU = SIMD_I8X16_GT_U;
    I8x16LeU = SIMD_I8X16_LE_U;
    I8x16GeU = SIMD_I8X16_GE_U;
    I8x16Add = SIMD_I8X16_ADD;
    I8x16Sub = SIMD_I8X16_SUB;
    I8x16Abs = SIMD_I8X16_ABS;
    I8x16Neg = SIMD_I8X16_NEG;
    I16x8Eq = SIMD_I16X8_EQ;
    I16x8Ne = SIMD_I16X8_NE;
    I16x8Add = SIMD_I16X8_ADD;
    I16x8Sub = SIMD_I16X8_SUB;
    I16x8Mul = SIMD_I16X8_MUL;
    I32x4Eq = SIMD_I32X4_EQ;
    I32x4Ne = SIMD_I32X4_NE;
    I32x4Add = SIMD_I32X4_ADD;
    I32x4Sub = SIMD_I32X4_SUB;
    I32x4Mul = SIMD_I32X4_MUL;
    I64x2Add = SIMD_I64X2_ADD;
    I64x2Sub = SIMD_I64X2_SUB;
    I64x2Mul = SIMD_I64X2_MUL;
}
