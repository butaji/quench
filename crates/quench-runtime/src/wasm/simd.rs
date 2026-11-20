//! Exact vector bits and admitted SIMD lane operations; no host SIMD dependency. The operators'
//! semantics are `staged::wasm_simd_lanes` (shared with the b-jit rows); this module adds the
//! `WasmValue` boundary, the kernel table the interpreter reads, and the interpreter's fast paths.

use super::{V128_BYTES, WasmValue};
use crate::bytecode::Register;
use crate::staged::wasm_emit;
pub(crate) use crate::staged::wasm_simd_lanes::SimdOperator;
pub(crate) use crate::staged::wasm_simd_rules::SimdKernel;

pub(crate) use crate::staged::wasm_simd_rules::{bits_of, value_of};

/// The wasmparser operator of each SIMD operator, generated from the operator list: a lane
/// operator's lane immediate is read from the instruction.
macro_rules! wasm_operators {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident, $family:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        impl SimdOperator {
            pub(super) fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, u8)> {
                match op {
                    $(wasmparser::Operator::$name $({ $lane })? => Some((Self::$name, [$(*$lane)?].first().copied().unwrap_or(0))),)*
                    _ => None,
                }
            }
        }
    };
}
crate::staged::wasm_simd_lanes::simd_operator_list!(wasm_operators);

impl SimdOperator {
    /// The operator on `left` and `right` at the `WasmValue` boundary: the bits body, typed by the
    /// kernel's result.
    pub(crate) fn apply(self, left: WasmValue, right: Option<WasmValue>, index: u8) -> WasmValue {
        let right = right.map_or(0, bits_of);
        value_of(Self::KERNELS[self as usize].result, self.bits(bits_of(left), right, index))
    }
}

/// A shuffle's sixteen byte indices, applied to `left` and `right` (the operands' bytes, `left` first).
pub(crate) fn shuffle(left: u128, right: u128, indices: [u8; V128_BYTES]) -> u128 {
    crate::staged::wasm_simd_rules::shuffle_bits(left, right, indices)
}
/// The bytes a shuffle selects from: both operands'.
const SHUFFLE_SOURCE_BYTES: usize = V128_BYTES * 2;
const _: () = assert!(SHUFFLE_SOURCE_BYTES.is_power_of_two());
pub(crate) fn shuffle_indices_valid(indices: &[u8; V128_BYTES]) -> bool {
    indices
        .iter()
        .all(|&index| (index as usize) < SHUFFLE_SOURCE_BYTES)
}

#[derive(Clone, Copy)]
enum TernaryComposition {
    Bitselect,
    MultiplyAdd {
        negate: Option<SimdOperator>,
        multiply: SimdOperator,
        add: SimdOperator,
    },
    DotAdd,
}

macro_rules! ternary_compositions {
    ($($wasm:ident => $composition:expr;)+) => {
        impl TernaryComposition {
            fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<Self> {
                use SimdOperator::*;
                match op { $(wasmparser::Operator::$wasm => Some($composition),)+ _ => None }
            }
        }
    }
}
ternary_compositions! {
    V128Bitselect => Self::Bitselect;
    I8x16RelaxedLaneselect => Self::Bitselect;
    I16x8RelaxedLaneselect => Self::Bitselect;
    I32x4RelaxedLaneselect => Self::Bitselect;
    I64x2RelaxedLaneselect => Self::Bitselect;
    F32x4RelaxedMadd => Self::MultiplyAdd { negate: None, multiply: F32x4Mul, add: F32x4Add };
    F32x4RelaxedNmadd => Self::MultiplyAdd { negate: Some(F32x4Neg), multiply: F32x4Mul, add: F32x4Add };
    F64x2RelaxedMadd => Self::MultiplyAdd { negate: None, multiply: F64x2Mul, add: F64x2Add };
    F64x2RelaxedNmadd => Self::MultiplyAdd { negate: Some(F64x2Neg), multiply: F64x2Mul, add: F64x2Add };
    I32x4RelaxedDotI8x16I7x16AddS => Self::DotAdd;
}

/// Whether `op` lowers through `Lowering::simd_operator`'s pop and push alone, so an aliased
/// operand may be read from its local register: lane operators and shuffles, not the
/// ternary compositions, which write an operand position.
pub(super) fn reads_aliases(op: &wasmparser::Operator<'_>) -> bool {
    SimdOperator::from_wasm(op).is_some() || matches!(op, wasmparser::Operator::I8x16Shuffle { .. })
}

impl super::Lowering<'_> {
    fn emit_simd(
        &mut self,
        op: SimdOperator,
        lane: u8,
        result: Register,
        left: Register,
        right: Register,
    ) -> Result<(), crate::Diagnostic> {
        self.emit_emission(wasm_emit::simd(result, left, right, op.selector(lane)))
    }
    pub(super) fn simd_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        if self.ternary_simd_operator(operator)? {
            return Ok(true);
        }
        if let Some((op, lane)) = SimdOperator::from_wasm(operator) {
            if SimdOperator::from_selector(op.selector(lane)).is_none() {
                return Err(crate::Diagnostic::unsupported(
                    self.name,
                    "invalid SIMD lane",
                ));
            }
            if self.path == super::Reachability::Dead {
                return Ok(true);
            }
            let right = if op.right_type().is_some() {
                self.pop()?
            } else {
                0
            };
            let left = self.pop()?;
            let result = self.push()?;
            self.emit_simd(op, lane, result, left, right)?;
            self.produced(result);
            return Ok(true);
        }
        match operator {
            wasmparser::Operator::I8x16Shuffle { lanes } => {
                if !shuffle_indices_valid(lanes) {
                    return Err(crate::Diagnostic::unsupported(
                        self.name,
                        "invalid SIMD shuffle",
                    ));
                }
                if self.path == super::Reachability::Dead {
                    return Ok(true);
                }
                let right = self.pop()?;
                let left = self.pop()?;
                let result = self.push()?;
                let constant =
                    self.scalar_constant(WasmValue::V128(u128::from_le_bytes(*lanes)))?;
                self.emit_emission(wasm_emit::simd_shuffle(result, left, right, constant))?;
                self.produced(result);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

impl super::Lowering<'_> {
    fn ternary_simd_operator(
        &mut self,
        op: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        let Some(composition) = TernaryComposition::from_wasm(op) else {
            return Ok(false);
        };
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let third = self.pop()?;
        let right = self.pop()?;
        let left = self.pop()?;
        let depth = self.depth;
        self.depth = third + 1;
        let first = self.push()?;
        match composition {
            TernaryComposition::Bitselect => {
                let second = self.push()?;
                self.emit_simd(SimdOperator::V128And, 0, first, left, third)?;
                self.emit_simd(SimdOperator::V128AndNot, 0, second, right, third)?;
                self.emit_simd(SimdOperator::V128Or, 0, left, first, second)?;
            }
            TernaryComposition::MultiplyAdd {
                negate,
                multiply,
                add,
            } => {
                let input = if let Some(negate) = negate {
                    self.emit_simd(negate, 0, first, left, 0)?;
                    first
                } else {
                    left
                };
                self.emit_simd(multiply, 0, first, input, right)?;
                self.emit_simd(add, 0, left, first, third)?;
            }
            TernaryComposition::DotAdd => {
                self.emit_simd(
                    SimdOperator::I16x8RelaxedDotI8x16I7x16S,
                    0,
                    first,
                    left,
                    right,
                )?;
                self.emit_simd(SimdOperator::I32x4ExtAddPairwiseI16x8S, 0, first, first, 0)?;
                self.emit_simd(SimdOperator::I32x4Add, 0, left, first, third)?;
            }
        }
        self.depth = depth;
        self.push()?;
        Ok(true)
    }
}

/// The lane operators an unboxed execution computes on bits directly, without the
/// `WasmValue` boundary of `SimdKernel`: each is exactly its operator's kernel, which
/// `bit_kernels_match_their_operators` checks lane by lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BitKernel {
    And,
    AndNot,
    Or,
    Xor,
    I32x4Add,
    I32x4Sub,
    I64x2Add,
    I64x2Sub,
    I64x2Mul,
    /// The u64 products of the low two u32 lanes (`_mm_mul_epu32` past its shuffles).
    I64x2ExtMulLowI32x4U,
    /// A lane shift whose count the caller fixed; the kernel reduces it modulo the lane
    /// width, as the operator does.
    Shift(BitShift),
}

/// A lane shift by a fixed count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BitShift {
    I32x4Shl,
    I32x4ShrU,
    I32x4ShrS,
    I64x2Shl,
    I64x2ShrU,
    I64x2ShrS,
    /// `x << k | x >> (64 - k)` on each u64 lane, as `i64x2.shl`, `i64x2.shr_u` and
    /// `v128.or` compute it: a rotation by `k` modulo the lane width.
    I64x2Rotl,
}

/// The u64 lanes of `bits`, low first.
#[inline(always)]
fn u64_lanes(bits: u128) -> [u64; 2] {
    [bits as u64, (bits >> u64::BITS) as u64]
}
#[inline(always)]
fn from_u64_lanes([low, high]: [u64; 2]) -> u128 {
    u128::from(low) | (u128::from(high) << u64::BITS)
}
#[inline(always)]
fn u32_lanes(bits: u128) -> [u32; 4] {
    std::array::from_fn(|lane| (bits >> (u32::BITS * lane as u32)) as u32)
}
#[inline(always)]
fn from_u32_lanes(lanes: [u32; 4]) -> u128 {
    lanes
        .iter()
        .enumerate()
        .fold(0, |bits, (lane, &value)| bits | (u128::from(value) << (u32::BITS * lane as u32)))
}

impl BitKernel {
    /// The bit kernel of `selector`'s operator: one whose operands are vectors, or a shift
    /// whose count the caller knows (`count`).
    pub(crate) fn of(selector: u32, count: Option<i32>) -> Option<Self> {
        use SimdOperator as S;
        let (operator, _) = SimdOperator::from_selector(selector)?;
        let shift = |shift: BitShift| count.map(|_| Self::Shift(shift));
        match operator {
            S::V128And => Some(Self::And),
            S::V128AndNot => Some(Self::AndNot),
            S::V128Or => Some(Self::Or),
            S::V128Xor => Some(Self::Xor),
            S::I32x4Add => Some(Self::I32x4Add),
            S::I32x4Sub => Some(Self::I32x4Sub),
            S::I64x2Add => Some(Self::I64x2Add),
            S::I64x2Sub => Some(Self::I64x2Sub),
            S::I64x2Mul => Some(Self::I64x2Mul),
            S::I64x2ExtMulLowI32x4U => Some(Self::I64x2ExtMulLowI32x4U),
            S::I32x4Shl => shift(BitShift::I32x4Shl),
            S::I32x4ShrU => shift(BitShift::I32x4ShrU),
            S::I32x4ShrS => shift(BitShift::I32x4ShrS),
            S::I64x2Shl => shift(BitShift::I64x2Shl),
            S::I64x2ShrU => shift(BitShift::I64x2ShrU),
            S::I64x2ShrS => shift(BitShift::I64x2ShrS),
            _ => None,
        }
    }

    /// The operator on `left` and `right`: a vector, or for a shift the count in its low bits.
    #[inline(always)]
    pub(crate) fn apply(self, left: u128, right: u128) -> u128 {
        let pairs = |operation: fn(u64, u64) -> u64| {
            let (a, b) = (u64_lanes(left), u64_lanes(right));
            from_u64_lanes([operation(a[0], b[0]), operation(a[1], b[1])])
        };
        let quads = |operation: fn(u32, u32) -> u32| {
            let (a, b) = (u32_lanes(left), u32_lanes(right));
            from_u32_lanes(std::array::from_fn(|lane| operation(a[lane], b[lane])))
        };
        match self {
            Self::And => left & right,
            Self::AndNot => left & !right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
            Self::I32x4Add => quads(u32::wrapping_add),
            Self::I32x4Sub => quads(u32::wrapping_sub),
            Self::I64x2Add => pairs(u64::wrapping_add),
            Self::I64x2Sub => pairs(u64::wrapping_sub),
            Self::I64x2Mul => pairs(u64::wrapping_mul),
            Self::I64x2ExtMulLowI32x4U => {
                let (a, b) = (u32_lanes(left), u32_lanes(right));
                from_u64_lanes([
                    u64::from(a[0]) * u64::from(b[0]),
                    u64::from(a[1]) * u64::from(b[1]),
                ])
            }
            Self::Shift(shift) => {
                let count = right as u32;
                let (narrow, wide) = (count % u32::BITS, count % u64::BITS);
                match shift {
                    BitShift::I32x4Shl => from_u32_lanes(u32_lanes(left).map(|lane| lane << narrow)),
                    BitShift::I32x4ShrU => from_u32_lanes(u32_lanes(left).map(|lane| lane >> narrow)),
                    BitShift::I32x4ShrS => {
                        from_u32_lanes(u32_lanes(left).map(|lane| ((lane as i32) >> narrow) as u32))
                    }
                    BitShift::I64x2Shl => from_u64_lanes(u64_lanes(left).map(|lane| lane << wide)),
                    BitShift::I64x2ShrU => from_u64_lanes(u64_lanes(left).map(|lane| lane >> wide)),
                    BitShift::I64x2ShrS => {
                        from_u64_lanes(u64_lanes(left).map(|lane| ((lane as i64) >> wide) as u64))
                    }
                    BitShift::I64x2Rotl => from_u64_lanes(u64_lanes(left).map(|lane| lane.rotate_left(wide))),
                }
            }
        }
    }
}

/// A shuffle's lane indices in the coarsest lanes they move whole: aligned u32 lanes of the
/// two operands, or bytes. Each form selects exactly what `shuffle` selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShuffleForm {
    /// Per result u32 lane, the operands' u32 lane it copies (right's from `WORD_LANES`).
    Words([u8; WORD_LANES]),
    Bytes([u8; V128_BYTES]),
}

/// A vector's u32 lanes.
const WORD_LANES: usize = V128_BYTES / size_of::<u32>();
/// The u32 lanes a shuffle selects from: both operands'.
const SHUFFLE_SOURCE_WORDS: usize = WORD_LANES * 2;

/// `i64x2.extmul_low_i32x4_u` of two vectors each shuffled from one operand by u32 lanes
/// (`_mm_mul_epu32`): per result u64 lane, the product of the operands' u32 lanes it names.
#[inline(always)]
pub(crate) fn multiply_words(left: u128, right: u128, left_words: [u8; 2], right_words: [u8; 2]) -> u128 {
    let (left, right) = (u32_lanes(left), u32_lanes(right));
    let product = |lane: usize| {
        u64::from(left[usize::from(left_words[lane]) % WORD_LANES])
            * u64::from(right[usize::from(right_words[lane]) % WORD_LANES])
    };
    from_u64_lanes([product(0), product(1)])
}

impl ShuffleForm {
    pub(crate) fn of(indices: [u8; V128_BYTES]) -> Self {
        let width = size_of::<u32>();
        let words: Option<Vec<u8>> = indices
            .chunks_exact(width)
            .map(|word| {
                let first = word[0] & (SHUFFLE_SOURCE_BYTES - 1) as u8;
                ((first as usize).is_multiple_of(width)
                    && word.iter().zip(first..).all(|(&index, expected)| {
                        index & (SHUFFLE_SOURCE_BYTES - 1) as u8 == expected
                    }))
                .then_some(first / width as u8)
            })
            .collect();
        match words {
            Some(words) => Self::Words(words.try_into().expect("four words")),
            None => Self::Bytes(indices),
        }
    }

    #[inline(always)]
    pub(crate) fn apply(self, left: u128, right: u128) -> u128 {
        match self {
            Self::Words(words) => {
                let (left, right) = (u32_lanes(left), u32_lanes(right));
                let sources = [left[0], left[1], left[2], left[3], right[0], right[1], right[2], right[3]];
                let pick = |lane: usize| u128::from(sources[usize::from(words[lane]) % SHUFFLE_SOURCE_WORDS]);
                pick(0) | (pick(1) << u32::BITS) | (pick(2) << (2 * u32::BITS)) | (pick(3) << (3 * u32::BITS))
            }
            Self::Bytes(indices) => shuffle(left, right, indices),
        }
    }
}

#[cfg(test)]
mod bit_kernel_tests {
    use super::{BitKernel, ShuffleForm, SimdKernel, SimdOperator};

    #[test]
    fn shuffle_forms_select_what_the_shuffle_selects() {
        let (left, right) = (0x0f0e_0d0c_0b0a_0908_0706_0504_0302_0100_u128, 0x1f1e_1d1c_1b1a_1918_1716_1514_1312_1110_u128);
        let words = |lanes: [u8; 4]| -> [u8; 16] {
            std::array::from_fn(|byte| lanes[byte / 4] * 4 + (byte % 4) as u8)
        };
        // u32 lane picks from both operands (`_mm_mul_epu32`'s, a rotation of u64 lanes),
        // indices past the operands' bytes, which validation excludes but the mask reduces,
        // and byte patterns no word form covers.
        let cases = [
            words([0, 2, 0, 2]),
            words([2, 3, 4, 5]),
            words([7, 6, 1, 0]),
            std::array::from_fn(|byte| byte as u8 + 32),
            std::array::from_fn(|byte| (byte as u8 * 7) % 32),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        ];
        for indices in cases {
            assert_eq!(ShuffleForm::of(indices).apply(left, right), super::shuffle(left, right, indices), "{indices:?}");
        }
        assert!(matches!(ShuffleForm::of(words([0, 2, 0, 2])), ShuffleForm::Words(_)));
        assert!(matches!(ShuffleForm::of(cases[5]), ShuffleForm::Bytes(_)));
    }

    /// The operator `name` applied to bits, through its kernel.
    fn operator(name: SimdOperator, left: u128, right: u128) -> u128 {
        let (kernel, lane) = SimdKernel::of(name.selector(0)).expect("operator kernel");
        kernel.apply_bits(left, right, lane)
    }

    #[test]
    fn fused_kernels_match_their_compositions() {
        use super::{BitShift, multiply_words};
        for &x in &EDGES {
            // A rotation by k, including by 0 and by the lane width, which wrap to the identity.
            for k in [0u32, 1, 16, 24, 32, 63, 64, 65, 127] {
                let rotated = BitKernel::Shift(BitShift::I64x2Rotl).apply(x, u128::from(k));
                let composed = operator(SimdOperator::V128Or, operator(SimdOperator::I64x2Shl, x, u128::from(k)), operator(SimdOperator::I64x2ShrU, x, u128::from(64u32.wrapping_sub(k))));
                assert_eq!(rotated, composed, "rotl {x:#x} by {k}");
            }
            for &y in &EDGES {
                for (left, right) in [([0u8, 2], [0u8, 2]), ([1, 3], [2, 0]), ([4, 6], [7, 5])] {
                    let shuffle = |value: u128, words: [u8; 2]| {
                        ShuffleForm::of(std::array::from_fn(|byte| words[(byte / 4) % 2] * 4 + (byte % 4) as u8)).apply(value, value)
                    };
                    let composed = operator(SimdOperator::I64x2ExtMulLowI32x4U, shuffle(x, left), shuffle(y, right));
                    assert_eq!(multiply_words(x, y, left, right), composed, "{x:#x} {y:#x} {left:?} {right:?}");
                }
            }
        }
    }

    /// Vectors whose lanes reach every carry, sign and width boundary.
    const EDGES: [u128; 8] = [
        0,
        u128::MAX,
        0x8000_0000_0000_0000_8000_0000_0000_0000,
        0x7fff_ffff_ffff_ffff_7fff_ffff_ffff_ffff,
        0xffff_ffff_0000_0001_8000_0000_7fff_ffff,
        0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
        0x0000_0001_ffff_ffff_0000_0000_ffff_fffe,
        0xdead_beef_0bad_f00d_8000_0001_0000_0003,
    ];
    /// Shift counts at and past each lane width, which the operators reduce modulo it.
    const COUNTS: [i32; 12] = [0, 1, 7, 31, 32, 33, 63, 64, 65, 127, -1, i32::MIN];

    #[test]
    fn bit_kernels_match_their_operators() {
        for (index, _) in SimdOperator::ALL.iter().enumerate() {
            let selector = (index as u32) << crate::staged::wasm_simd_rules::SELECTOR_LANE_BITS;
            let (kernel, lane) = SimdKernel::of(selector).expect("operator kernel");
            for count in [None, Some(0)] {
                let Some(bits) = BitKernel::of(selector, count) else {
                    continue;
                };
                for &left in &EDGES {
                    let rights: Vec<u128> = match bits {
                        BitKernel::Shift(_) => COUNTS.iter().map(|&count| u128::from(count as u32)).collect(),
                        _ => EDGES.to_vec(),
                    };
                    for right in rights {
                        assert_eq!(
                            bits.apply(left, right),
                            kernel.apply_bits(left, right, lane),
                            "{bits:?} {left:#x} {right:#x}"
                        );
                    }
                }
            }
        }
    }
}
