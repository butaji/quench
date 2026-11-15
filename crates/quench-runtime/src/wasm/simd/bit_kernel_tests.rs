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
        let selector = (index as u32) << super::SELECTOR_LANE_BITS;
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
