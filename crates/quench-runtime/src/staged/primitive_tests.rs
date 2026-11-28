//! The element primitives of `Stage` on the concrete stage, against Rust's own conversions, and
//! the trace the recording stage makes of them.

use super::trace::Trace;
use super::{Concrete, NumericStage, Stage};

/// Bytes whose every width reads a different value at offset zero: `0xFF`, `0x80`, ...
const BYTES: [u8; 8] = [0xFF, 0x80, 0x01, 0xC0, 0x00, 0x00, 0x80, 0x3F];

fn stage() -> Concrete { Concrete::new(std::ptr::null()) }

#[test]
fn integer_loads_extend_as_their_names_say() {
    let mut stage = stage();
    let base = BYTES.as_ptr() as u64;
    assert_eq!(stage.load_u8(crate::staged::Region::Elements.mutable(), base, 0), 0xFF);
    assert_eq!(stage.load_i8(crate::staged::Region::Elements.mutable(), base, 0), u64::from(-1_i32 as u32));
    assert_eq!(stage.load_i8(crate::staged::Region::Elements.mutable(), base, 2), 1);
    assert_eq!(stage.load_u16(crate::staged::Region::Elements.mutable(), base, 0), u64::from(u16::from_ne_bytes([0xFF, 0x80])));
    assert_eq!(stage.load_i16(crate::staged::Region::Elements.mutable(), base, 0), u64::from(i16::from_ne_bytes([0xFF, 0x80]) as i32 as u32));
    assert_eq!(stage.load_i16(crate::staged::Region::Elements.mutable(), base, 1), u64::from(i16::from_ne_bytes([0x80, 0x01]) as i32 as u32));
    assert_eq!(stage.load_i32(crate::staged::Region::Elements.mutable(), base, 0), u64::from(u32::from_ne_bytes([0xFF, 0x80, 0x01, 0xC0])));
    assert_eq!(stage.load_u32(crate::staged::Region::Elements.mutable(), base, 0), stage.load_i32(crate::staged::Region::Elements.mutable(), base, 0));
}

#[test]
fn float_loads_and_conversions_round_trip() {
    let mut stage = stage();
    let single = 1.5_f32.to_ne_bytes();
    let double = (-2.25_f64).to_ne_bytes();
    let word = stage.load_f32(crate::staged::Region::Elements.mutable(), single.as_ptr() as u64, 0);
    assert_eq!(stage.f32_to_f64(word), 1.5_f64.to_bits());
    assert_eq!(stage.load_f64(crate::staged::Region::Elements.mutable(), double.as_ptr() as u64, 0), (-2.25_f64).to_bits());
    assert_eq!(stage.f64_to_f32((0.1_f64).to_bits()), u64::from(0.1_f32.to_bits()), "binary32 rounds to nearest");
    assert_eq!(stage.u32_to_f64(u64::from(u32::MAX)), f64::from(u32::MAX).to_bits());
    assert_eq!(stage.i32_to_f64(u64::from(u32::MAX)), (-1.0_f64).to_bits());
}

#[test]
fn stores_write_their_width_and_leave_the_neighbours() {
    let mut stage = stage();
    let mut bytes = [0xAA_u8; 8];
    let base = bytes.as_mut_ptr() as u64;
    stage.store_u8(crate::staged::Region::Elements.mutable(), base, 1, 0x1234);
    stage.store_u16(crate::staged::Region::Elements.mutable(), base, 2, 0xABCDE);
    stage.store_f32(crate::staged::Region::Elements.mutable(), base, 4, u64::from(2.0_f32.to_bits()));
    assert_eq!(bytes[0..2], [0xAA, 0x34]);
    assert_eq!(bytes[2..4], 0xBCDE_u16.to_ne_bytes());
    assert_eq!(bytes[4..8], 2.0_f32.to_ne_bytes());
    let mut double = [0_u8; 8];
    stage.store_f64(crate::staged::Region::Elements.mutable(), double.as_mut_ptr() as u64, 0, 0.5_f64.to_bits());
    assert_eq!(double, 0.5_f64.to_ne_bytes());
}

#[test]
fn uint8_clamp_rounds_half_to_even_and_sends_nan_to_zero() {
    let mut stage = stage();
    let clamp = |stage: &mut Concrete, number: f64| stage.clamp_u8(number.to_bits());
    for (number, expected) in [(f64::NAN, 0), (-5.0, 0), (0.5, 0), (1.5, 2), (2.5, 2), (254.5, 254), (255.5, 255),
        (300.0, 255), (f64::INFINITY, 255), (f64::NEG_INFINITY, 0), (-0.0, 0)] {
        assert_eq!(clamp(&mut stage, number), expected, "{number}");
    }
}

#[test]
fn to_int32_wraps_like_the_language() {
    let mut stage = stage();
    for number in [0.0, -1.5, 4294967297.0, 2147483648.0, -2147483649.0, 1e20, f64::NAN, f64::INFINITY] {
        let word = stage.to_int32(number.to_bits(), ());
        assert_eq!(word, Some(u64::from(crate::units::to_int32(number) as u32)), "{number}");
    }
}

#[test]
fn the_recording_stage_names_each_primitive() {
    let mut trace = Trace::default();
    let base = trace.constant(0);
    let loaded = trace.load_f32(crate::staged::Region::Elements.mutable(), base, 4);
    let wide = trace.f32_to_f64(loaded);
    let clamped = trace.clamp_u8(wide);
    trace.store_u8(crate::staged::Region::Elements.mutable(), base, 1, clamped);
    assert_eq!(trace.ops[1..], ["load_f32 w0+4", "f32_to_f64 w1", "clamp_u8 w2", "store_u8 w0+1 w3"]);
}
