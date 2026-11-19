//! The b-jit's vector rows over raw frame words (kinds `SIMD_*`, 26..=35 in `wasm_codes.rs`).
//!
//! Representation: a v128 register is two consecutive frame words, low word first (bits 0..64,
//! 64..128 of the vector, the layout `compiled_simd_kernel` reads), in the vector area past the
//! scalar registers (`wasm_stitch_simd::vector_slot`). A row's operand hole names the low word's
//! byte offset; the high word is at +8. No row boxes a vector, allocates, or calls the VM: a
//! stitched function never hands a v128 across its frame (the u-jit admits no v128 parameter,
//! result, call window or return, and `stitched_entry` stitches only a function its u-jit
//! compiled), so the words never reach the heap, the collector or a callout.
//!
//! Semantics are the staged rules', mirrored on words because the stencil build does not compile
//! `wasm::simd` (it names `WasmValue`):
//! - integer lanes: wrapping arithmetic masked to the lane width (`wasm/simd/integer.rs`);
//! - float lanes: `staged::wasm_float::float_binary` and `operate_float_unary`, the calls
//!   `F32BinaryOperator::evaluate` and the float unary kernels make (`wasm/float.rs`);
//! - a comparison's lane is all ones where it holds, zero where not (`map_lanes`, `numeric`);
//! - a swizzle index of 16 or more reads zero; a shuffle index is taken modulo 32 (`wasm/simd.rs`).
//!
//! Holes (the same meaning in every row): `0` is the destination (`HOLE_V0`), `HOLE_A` and
//! `HOLE_B` the first and second operands, `HOLE_FOURTH` and `LAST_HOLE` the rest. A vector
//! operand's hole is its low word's offset; a scalar operand's hole is its frame word's offset.
//! Row by row, with the OP tag the row is instantiated at:
//! - `SIMD_BINARY`:  D (vector) = A op B (vectors).
//! - `SIMD_UNARY`:   D = op A.
//! - `SIMD_SHUFFLE`: D = shuffle(A, B) by the sixteen byte indices in `HOLE_FOURTH` (bytes 0..8)
//!                   and `LAST_HOLE` (bytes 8..16).
//! - `SIMD_CONST`:   D = the vector whose low word is `HOLE_A` and high word `HOLE_B`.
//! - `SIMD_LOAD`:    D = the 16 bytes at the 64-bit address in `HOLE_A` (the staged rule's bounds).
//! - `SIMD_STORE`:   the 16 bytes at the 64-bit address in `HOLE_A` = D.
//! - `SIMD_SPLAT`:   D = splat(A) (A a scalar word, its low bits).
//! - `SIMD_EXTRACT`: D (scalar word) = lane `HOLE_B` of A.
//! - `SIMD_REPLACE`: D = A with lane `HOLE_FOURTH` replaced by B (a scalar word in `HOLE_B`).
//! - `SIMD_MOVE`:    D = A (a vector).
//!
//! The rows are the library's (`wasm_library.rs`), so each has its symbol `w_<kind>_<op>_1_0`
//! (width code `W64` = 1, shape `REGISTER` = 0) and is dispatched from `wasm_rows::run`. A row that traps leaves through `trap`, as the scalar
//! rows do (the access is the only trap).

use super::{StencilMemory, read, trap, write};
use crate::staged::machine::MachineStage;
use crate::staged::wasm_concrete::ConcreteWasm;
use crate::staged::wasm_float::{Float, float_binary};
use crate::staged::wasm_memory::{Access, check_access};
use crate::wasm::WasmType;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, LAST_HOLE, Out, S, hole64, slot};
use crate::wasm_codes::*;

/// A vector as its low and high words.
type Vector = [u64; 2];

/// A vector access: 16 bytes, as `Access::VECTOR` (which the stencil build does not compile).
const VECTOR: Access = Access { ty: WasmType::V128, bytes: 16, signed: false };

/// The vector in the two words at hole `H`'s offset.
#[inline(always)]
unsafe fn vread<const H: u8>(s: &S) -> Vector {
    unsafe {
        let at = slot::<H>(s) as *const u64;
        [*at, *at.add(1)]
    }
}

/// Writes `value` to the two words at hole `H`'s offset.
#[inline(always)]
unsafe fn vwrite<const H: u8>(s: &S, value: Vector) {
    unsafe {
        let at = slot::<H>(s) as *mut u64;
        *at = value[0];
        *at.add(1) = value[1];
    }
}

/// `mask` of the low `width` bits.
#[inline(always)]
pub(super) fn mask(width: u32) -> u64 {
    if width >= 64 { u64::MAX } else { (1u64 << width) - 1 }
}

/// All ones of a `width`-bit lane where `holds`, zero where not.
#[inline(always)]
fn when(holds: bool, width: u32) -> u64 {
    if holds { mask(width) } else { 0 }
}

/// Lane `index` of `value`, `width` bits wide, zero-extended.
#[inline(always)]
fn lane(value: Vector, width: u32, index: u32) -> u64 {
    let bit = width * index;
    let word = if bit < 64 { value[0] >> bit } else { value[1] >> (bit - 64) };
    word & mask(width)
}

/// `value` with lane `index` set to `lane`'s low `width` bits.
#[inline(always)]
fn put(value: &mut Vector, width: u32, index: u32, lane: u64) {
    let m = mask(width);
    let bit = width * index;
    if bit < 64 {
        value[0] = (value[0] & !(m << bit)) | ((lane & m) << bit);
    } else {
        let bit = bit - 64;
        value[1] = (value[1] & !(m << bit)) | ((lane & m) << bit);
    }
}

/// The lane `width` bits wide read as signed, sign-extended.
#[inline(always)]
pub(super) fn signed(bits: u64, width: u32) -> i64 {
    let shift = 64 - width;
    ((bits << shift) as i64) >> shift
}

/// Each lane of `left` and `right` through `operation`, `width` bits wide.
#[inline(always)]
fn lanes(left: Vector, right: Vector, width: u32, operation: impl Fn(u64, u64) -> u64) -> Vector {
    let mut result = [0u64; 2];
    let mut index = 0;
    while index < 128 / width {
        put(&mut result, width, index, operation(lane(left, width, index), lane(right, width, index)));
        index += 1;
    }
    result
}

/// Each lane of `value` through `operation`, `width` bits wide.
#[inline(always)]
fn lanes_one(value: Vector, width: u32, operation: impl Fn(u64) -> u64) -> Vector {
    let mut result = [0u64; 2];
    let mut index = 0;
    while index < 128 / width {
        put(&mut result, width, index, operation(lane(value, width, index)));
        index += 1;
    }
    result
}

/// The float binary operator on two lanes of `width` bits (32 or 64), as `F32BinaryOperator::evaluate`
/// computes it: a comparison's lane is all ones where it holds, an arithmetic lane its result's bits.
#[inline(always)]
fn float_lane(operator: FloatBinary, float: Float, left: u64, right: u64, width: u32) -> u64 {
    let word = float_binary(&mut ConcreteWasm, operator, float, left, right);
    match operator {
        FloatBinary::Equal
        | FloatBinary::NotEqual
        | FloatBinary::Less
        | FloatBinary::Greater
        | FloatBinary::LessEqual
        | FloatBinary::GreaterEqual => when(word != 0, width),
        _ => word & mask(width),
    }
}

/// The float unary operator on one lane's bits.
#[inline(always)]
fn float_unary(operator: FloatUnary, float: Float, bits: u64) -> u64 {
    ConcreteWasm.operate_float_unary(operator, float, bits)
}

/// Byte `index` of the 32 bytes of `both` (the two operands' words, low first), or zero. A
/// swizzle reads `both` as its left operand alone, with an index of 16 or more zero; a shuffle
/// masks its index to 32.
#[inline(always)]
fn select(both: [u64; 4], indices: [u64; 2], swizzle: bool) -> Vector {
    let mut out = [0u64; 2];
    let mut word = 0;
    while word < 2 {
        let mut byte = 0;
        while byte < 8 {
            let raw = ((indices[word & 1] >> (8 * byte)) & 0xff) as usize;
            let index = if swizzle { raw } else { raw & 31 };
            let value = (both[(index >> 3) & 3] >> ((index & 7) * 8)) & 0xff;
            let value = if swizzle && index >= 16 { 0 } else { value };
            out[word & 1] |= value << (8 * byte);
            byte += 1;
        }
        word += 1;
    }
    out
}

/// `D = A op B` on vectors, the operator `OP` names.
#[inline(always)]
pub(crate) unsafe fn binary<const OP: u32>(s: S) -> Out {
    unsafe {
        let (left, right) = (vread::<HOLE_A>(&s), vread::<HOLE_B>(&s));
        let value = match OP {
            SIMD_V128_AND => [left[0] & right[0], left[1] & right[1]],
            SIMD_V128_ANDNOT => [left[0] & !right[0], left[1] & !right[1]],
            SIMD_V128_OR => [left[0] | right[0], left[1] | right[1]],
            SIMD_V128_XOR => [left[0] ^ right[0], left[1] ^ right[1]],
            SIMD_I8X16_SWIZZLE => select([left[0], left[1], 0, 0], right, true),

            SIMD_I8X16_ADD => lanes(left, right, 8, |x, y| x.wrapping_add(y)),
            SIMD_I8X16_SUB => lanes(left, right, 8, |x, y| x.wrapping_sub(y)),
            SIMD_I8X16_EQ => lanes(left, right, 8, |x, y| when(x == y, 8)),
            SIMD_I8X16_NE => lanes(left, right, 8, |x, y| when(x != y, 8)),
            SIMD_I8X16_LT_S => lanes(left, right, 8, |x, y| when(signed(x, 8) < signed(y, 8), 8)),
            SIMD_I8X16_GT_S => lanes(left, right, 8, |x, y| when(signed(x, 8) > signed(y, 8), 8)),
            SIMD_I8X16_LE_S => lanes(left, right, 8, |x, y| when(signed(x, 8) <= signed(y, 8), 8)),
            SIMD_I8X16_GE_S => lanes(left, right, 8, |x, y| when(signed(x, 8) >= signed(y, 8), 8)),
            SIMD_I8X16_LT_U => lanes(left, right, 8, |x, y| when(x < y, 8)),
            SIMD_I8X16_GT_U => lanes(left, right, 8, |x, y| when(x > y, 8)),
            SIMD_I8X16_LE_U => lanes(left, right, 8, |x, y| when(x <= y, 8)),
            SIMD_I8X16_GE_U => lanes(left, right, 8, |x, y| when(x >= y, 8)),

            SIMD_I16X8_ADD => lanes(left, right, 16, |x, y| x.wrapping_add(y)),
            SIMD_I16X8_SUB => lanes(left, right, 16, |x, y| x.wrapping_sub(y)),
            SIMD_I16X8_MUL => lanes(left, right, 16, |x, y| x.wrapping_mul(y)),
            SIMD_I16X8_EQ => lanes(left, right, 16, |x, y| when(x == y, 16)),
            SIMD_I16X8_NE => lanes(left, right, 16, |x, y| when(x != y, 16)),

            SIMD_I32X4_ADD => lanes(left, right, 32, |x, y| x.wrapping_add(y)),
            SIMD_I32X4_SUB => lanes(left, right, 32, |x, y| x.wrapping_sub(y)),
            SIMD_I32X4_MUL => lanes(left, right, 32, |x, y| x.wrapping_mul(y)),
            SIMD_I32X4_EQ => lanes(left, right, 32, |x, y| when(x == y, 32)),
            SIMD_I32X4_NE => lanes(left, right, 32, |x, y| when(x != y, 32)),

            SIMD_I64X2_ADD => lanes(left, right, 64, |x, y| x.wrapping_add(y)),
            SIMD_I64X2_SUB => lanes(left, right, 64, |x, y| x.wrapping_sub(y)),
            SIMD_I64X2_MUL => lanes(left, right, 64, |x, y| x.wrapping_mul(y)),

            SIMD_F32X4_ADD => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Add, Float::F32, x, y, 32)),
            SIMD_F32X4_SUB => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Subtract, Float::F32, x, y, 32)),
            SIMD_F32X4_MUL => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Multiply, Float::F32, x, y, 32)),
            SIMD_F32X4_DIV => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Divide, Float::F32, x, y, 32)),
            SIMD_F32X4_EQ => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Equal, Float::F32, x, y, 32)),
            SIMD_F32X4_NE => lanes(left, right, 32, |x, y| float_lane(FloatBinary::NotEqual, Float::F32, x, y, 32)),
            SIMD_F32X4_LT => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Less, Float::F32, x, y, 32)),
            SIMD_F32X4_GT => lanes(left, right, 32, |x, y| float_lane(FloatBinary::Greater, Float::F32, x, y, 32)),
            SIMD_F32X4_LE => lanes(left, right, 32, |x, y| float_lane(FloatBinary::LessEqual, Float::F32, x, y, 32)),
            SIMD_F32X4_GE => lanes(left, right, 32, |x, y| float_lane(FloatBinary::GreaterEqual, Float::F32, x, y, 32)),

            SIMD_F64X2_ADD => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Add, Float::F64, x, y, 64)),
            SIMD_F64X2_SUB => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Subtract, Float::F64, x, y, 64)),
            SIMD_F64X2_MUL => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Multiply, Float::F64, x, y, 64)),
            SIMD_F64X2_DIV => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Divide, Float::F64, x, y, 64)),
            SIMD_F64X2_EQ => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Equal, Float::F64, x, y, 64)),
            SIMD_F64X2_NE => lanes(left, right, 64, |x, y| float_lane(FloatBinary::NotEqual, Float::F64, x, y, 64)),
            SIMD_F64X2_LT => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Less, Float::F64, x, y, 64)),
            SIMD_F64X2_GT => lanes(left, right, 64, |x, y| float_lane(FloatBinary::Greater, Float::F64, x, y, 64)),
            SIMD_F64X2_LE => lanes(left, right, 64, |x, y| float_lane(FloatBinary::LessEqual, Float::F64, x, y, 64)),
            SIMD_F64X2_GE => lanes(left, right, 64, |x, y| float_lane(FloatBinary::GreaterEqual, Float::F64, x, y, 64)),

            _ => [0, 0],
        };
        vwrite::<0>(&s, value);
    }
    Out::Next(s)
}

/// `D = op A` on a vector, the operator `OP` names.
#[inline(always)]
pub(crate) unsafe fn unary<const OP: u32>(s: S) -> Out {
    unsafe {
        let value = vread::<HOLE_A>(&s);
        let result = match OP {
            SIMD_V128_NOT => [!value[0], !value[1]],
            // The kernel's: the negation of a negative lane, the lane otherwise.
            SIMD_I8X16_ABS => lanes_one(value, 8, |x| if signed(x, 8) < 0 { 0u64.wrapping_sub(x) } else { x }),
            SIMD_I8X16_NEG => lanes_one(value, 8, |x| 0u64.wrapping_sub(x)),
            SIMD_F32X4_ABS => lanes_one(value, 32, |x| float_unary(FloatUnary::Absolute, Float::F32, x)),
            SIMD_F32X4_NEG => lanes_one(value, 32, |x| float_unary(FloatUnary::Negate, Float::F32, x)),
            SIMD_F64X2_ABS => lanes_one(value, 64, |x| float_unary(FloatUnary::Absolute, Float::F64, x)),
            SIMD_F64X2_NEG => lanes_one(value, 64, |x| float_unary(FloatUnary::Negate, Float::F64, x)),
            _ => [0, 0],
        };
        vwrite::<0>(&s, result);
    }
    Out::Next(s)
}

/// `D = shuffle(A, B)` by the sixteen byte indices in holes `HOLE_FOURTH` and `LAST_HOLE`.
#[inline(always)]
pub(crate) unsafe fn shuffle(s: S) -> Out {
    unsafe {
        let (left, right) = (vread::<HOLE_A>(&s), vread::<HOLE_B>(&s));
        let indices = [hole64::<HOLE_FOURTH>(), hole64::<LAST_HOLE>()];
        vwrite::<0>(&s, select([left[0], left[1], right[0], right[1]], indices, false));
    }
    Out::Next(s)
}

/// `D = ` the vector whose low word is hole `HOLE_A` and high word `HOLE_B`.
#[inline(always)]
pub(crate) unsafe fn constant(s: S) -> Out {
    unsafe { vwrite::<0>(&s, [hole64::<HOLE_A>(), hole64::<HOLE_B>()]) };
    Out::Next(s)
}

/// `D = v128.load` at the 64-bit address in `HOLE_A`; traps out of bounds, writing nothing.
#[inline(always)]
pub(crate) unsafe fn load(s: S) -> Out {
    let index = unsafe { read::<HOLE_A>(&s) };
    let mut memory = StencilMemory::of(&s);
    match check_access(&mut memory, VECTOR, index, 0, false) {
        Ok(()) => {
            let value = unsafe {
                let at = memory.at(index, 0) as *const u64;
                [at.read_unaligned(), at.add(1).read_unaligned()]
            };
            unsafe { vwrite::<0>(&s, value) };
            Out::Next(s)
        }
        Err(error) => trap(s, error),
    }
}

/// `v128.store D` at the 64-bit address in `HOLE_A`; traps out of bounds, writing nothing.
#[inline(always)]
pub(crate) unsafe fn store(s: S) -> Out {
    let index = unsafe { read::<HOLE_A>(&s) };
    let value = unsafe { vread::<0>(&s) };
    let mut memory = StencilMemory::of(&s);
    match check_access(&mut memory, VECTOR, index, 0, false) {
        Ok(()) => {
            unsafe {
                let at = memory.at(index, 0) as *mut u64;
                at.write_unaligned(value[0]);
                at.add(1).write_unaligned(value[1]);
            }
            Out::Next(s)
        }
        Err(error) => trap(s, error),
    }
}

/// `D = splat(A)`, the lane width the operator `OP` names; A's low bits are the lane.
#[inline(always)]
pub(crate) unsafe fn splat<const OP: u32>(s: S) -> Out {
    let scalar = unsafe { read::<HOLE_A>(&s) };
    let width = match OP {
        SIMD_I8X16_SPLAT => 8,
        SIMD_I16X8_SPLAT => 16,
        SIMD_I32X4_SPLAT | SIMD_F32X4_SPLAT => 32,
        _ => 64,
    };
    let value = lanes_one([0, 0], width, |_| scalar);
    unsafe { vwrite::<0>(&s, value) };
    Out::Next(s)
}

/// `D = ` lane `HOLE_B` of A, as the scalar word the operator `OP` names (a signed lane sign-extends
/// to an i32, as the kernel does).
#[inline(always)]
pub(crate) unsafe fn extract<const OP: u32>(s: S) -> Out {
    let value = unsafe { vread::<HOLE_A>(&s) };
    let index = hole64::<HOLE_B>() as u32;
    let word = match OP {
        SIMD_I8X16_EXTRACT_S => signed(lane(value, 8, index), 8) as u32 as u64,
        SIMD_I8X16_EXTRACT_U => lane(value, 8, index),
        SIMD_I16X8_EXTRACT_S => signed(lane(value, 16, index), 16) as u32 as u64,
        SIMD_I16X8_EXTRACT_U => lane(value, 16, index),
        SIMD_I32X4_EXTRACT | SIMD_F32X4_EXTRACT => lane(value, 32, index),
        _ => lane(value, 64, index),
    };
    unsafe { write::<0>(&s, word) };
    Out::Next(s)
}

/// `D = A` with lane `HOLE_FOURTH` replaced by B's low bits, the lane width the operator `OP` names.
#[inline(always)]
pub(crate) unsafe fn replace<const OP: u32>(s: S) -> Out {
    let (value, scalar, index) = unsafe { (vread::<HOLE_A>(&s), read::<HOLE_B>(&s), hole64::<HOLE_FOURTH>() as u32) };
    let width = match OP {
        SIMD_I8X16_REPLACE => 8,
        SIMD_I16X8_REPLACE => 16,
        SIMD_I32X4_REPLACE | SIMD_F32X4_REPLACE => 32,
        _ => 64,
    };
    let mut result = value;
    put(&mut result, width, index, scalar);
    unsafe { vwrite::<0>(&s, result) };
    Out::Next(s)
}

/// `D = A`, a vector.
#[inline(always)]
pub(crate) unsafe fn moves(s: S) -> Out {
    unsafe {
        let value = vread::<HOLE_A>(&s);
        vwrite::<0>(&s, value);
    }
    Out::Next(s)
}
