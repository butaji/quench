//! Remaining SIMD kernels. One op, one function.

use super::simd::SimdOp;

pub fn apply(op: SimdOp, a: u128, b: u128, c: u128) -> u128 {
    match op {
        SimdOp::I8x16Popcnt => zip8(a, |x| x.count_ones() as u8),
        SimdOp::I8x16NarrowS => narrow8(a, b, true),
        SimdOp::I8x16NarrowU => narrow8(a, b, false),
        SimdOp::I8x16AvgrU => zip8b(a, b, |x, y| ((x as u16 + y as u16 + 1) / 2) as u8),
        SimdOp::I16x8AvgrU => zip16b(a, b, |x, y| ((x as u32 + y as u32 + 1) / 2) as u16),
        SimdOp::I16x8Q15Mulr => zip16b(a, b, q15),
        SimdOp::I16x8NarrowS => narrow16(a, b, true),
        SimdOp::I16x8NarrowU => narrow16(a, b, false),
        SimdOp::I16x8ExtAddPS => extadd8(a, true),
        SimdOp::I16x8ExtAddPU => extadd8(a, false),
        SimdOp::I16x8ExtLowS => extend8(a, false, true),
        SimdOp::I16x8ExtHighS => extend8(a, true, true),
        SimdOp::I16x8ExtLowU => extend8(a, false, false),
        SimdOp::I16x8ExtHighU => extend8(a, true, false),
        SimdOp::I16x8ExtMulLowS => extmul8(a, b, false, true),
        SimdOp::I16x8ExtMulHighS => extmul8(a, b, true, true),
        SimdOp::I16x8ExtMulLowU => extmul8(a, b, false, false),
        SimdOp::I16x8ExtMulHighU => extmul8(a, b, true, false),
        SimdOp::I32x4ExtAddPS => extadd16(a, true),
        SimdOp::I32x4ExtAddPU => extadd16(a, false),
        SimdOp::I32x4ExtLowS => extend16(a, false, true),
        SimdOp::I32x4ExtHighS => extend16(a, true, true),
        SimdOp::I32x4ExtLowU => extend16(a, false, false),
        SimdOp::I32x4ExtHighU => extend16(a, true, false),
        SimdOp::I32x4Dot => dot16(a, b),
        SimdOp::I32x4ExtMulLowS => extmul16(a, b, false, true),
        SimdOp::I32x4ExtMulHighS => extmul16(a, b, true, true),
        SimdOp::I32x4ExtMulLowU => extmul16(a, b, false, false),
        SimdOp::I32x4ExtMulHighU => extmul16(a, b, true, false),
        SimdOp::I64x2Abs => zip64(a, |x| (x as i64).unsigned_abs()),
        SimdOp::I64x2ExtLowS => extend32(a, false, true),
        SimdOp::I64x2ExtHighS => extend32(a, true, true),
        SimdOp::I64x2ExtLowU => extend32(a, false, false),
        SimdOp::I64x2ExtHighU => extend32(a, true, false),
        SimdOp::I64x2ExtMulLowS => extmul32(a, b, false, true),
        SimdOp::I64x2ExtMulHighS => extmul32(a, b, true, true),
        SimdOp::I64x2ExtMulLowU => extmul32(a, b, false, false),
        SimdOp::I64x2ExtMulHighU => extmul32(a, b, true, false),
        SimdOp::F32x4Ceil => zipf32(a, f32::ceil),
        SimdOp::F32x4Floor => zipf32(a, f32::floor),
        SimdOp::F32x4Trunc => zipf32(a, f32::trunc),
        SimdOp::F32x4Nearest => zipf32(a, f32::round_ties_even),
        SimdOp::F32x4PMin => zipf32b(a, b, |x, y| if y < x { y } else { x }),
        SimdOp::F32x4PMax => zipf32b(a, b, |x, y| if x < y { y } else { x }),
        SimdOp::F64x2Ceil => zipf64(a, f64::ceil),
        SimdOp::F64x2Floor => zipf64(a, f64::floor),
        SimdOp::F64x2Trunc => zipf64(a, f64::trunc),
        SimdOp::F64x2Nearest => zipf64(a, f64::round_ties_even),
        SimdOp::F64x2PMin => zipf64b(a, b, |x, y| if y < x { y } else { x }),
        SimdOp::F64x2PMax => zipf64b(a, b, |x, y| if x < y { y } else { x }),
        SimdOp::I32x4TruncSatF64S => trunc_f64(a, true),
        SimdOp::I32x4TruncSatF64U => trunc_f64(a, false),
        SimdOp::F64x2ConvertLowS => convert_low(a, true),
        SimdOp::F64x2ConvertLowU => convert_low(a, false),
        SimdOp::F32x4DemoteZero => demote(a),
        SimdOp::F64x2PromoteLow => promote(a),
        SimdOp::RelaxedLane8 => laneselect(a, b, c, 1),
        SimdOp::RelaxedLane16 => laneselect(a, b, c, 2),
        SimdOp::RelaxedLane32 => laneselect(a, b, c, 4),
        SimdOp::RelaxedLane64 => laneselect(a, b, c, 8),
        SimdOp::RelaxedMaddF32 => maddf32(a, b, c, false),
        SimdOp::RelaxedNmaddF32 => maddf32(a, b, c, true),
        SimdOp::RelaxedMaddF64 => maddf64(a, b, c, false),
        SimdOp::RelaxedNmaddF64 => maddf64(a, b, c, true),
        SimdOp::I16x8RelaxedDot => relaxed_dot_i16(a, b),
        SimdOp::I32x4RelaxedDotAdd => relaxed_dot_i32_add(a, b, c),
        _ => 0,
    }
}

fn zip8(a: u128, f: impl Fn(u8) -> u8) -> u128 {
    let mut o = a.to_le_bytes();
    for x in &mut o {
        *x = f(*x);
    }
    u128::from_le_bytes(o)
}

fn zip8b(a: u128, b: u128, f: impl Fn(u8, u8) -> u8) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..16 {
        o[i] = f(aa[i], bb[i]);
    }
    u128::from_le_bytes(o)
}

fn zip16b(a: u128, b: u128, f: impl Fn(u16, u16) -> u16) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..8 {
        let x = u16::from_le_bytes([aa[i * 2], aa[i * 2 + 1]]);
        let y = u16::from_le_bytes([bb[i * 2], bb[i * 2 + 1]]);
        o[i * 2..i * 2 + 2].copy_from_slice(&f(x, y).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn zip64(a: u128, f: impl Fn(u64) -> u64) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let x = u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        o[i * 8..i * 8 + 8].copy_from_slice(&f(x).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn zipf32(a: u128, f: impl Fn(f32) -> f32) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..4 {
        let x = f32::from_bits(u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()));
        o[i * 4..i * 4 + 4].copy_from_slice(&f(x).to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn zipf32b(a: u128, b: u128, f: impl Fn(f32, f32) -> f32) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..4 {
        let x = f32::from_bits(u32::from_le_bytes(aa[i * 4..i * 4 + 4].try_into().unwrap()));
        let y = f32::from_bits(u32::from_le_bytes(bb[i * 4..i * 4 + 4].try_into().unwrap()));
        o[i * 4..i * 4 + 4].copy_from_slice(&f(x, y).to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn zipf64(a: u128, f: impl Fn(f64) -> f64) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let x = f64::from_bits(u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap()));
        o[i * 8..i * 8 + 8].copy_from_slice(&f(x).to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn zipf64b(a: u128, b: u128, f: impl Fn(f64, f64) -> f64) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let x = f64::from_bits(u64::from_le_bytes(aa[i * 8..i * 8 + 8].try_into().unwrap()));
        let y = f64::from_bits(u64::from_le_bytes(bb[i * 8..i * 8 + 8].try_into().unwrap()));
        o[i * 8..i * 8 + 8].copy_from_slice(&f(x, y).to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn q15(x: u16, y: u16) -> u16 {
    let p = (x as i16 as i32) * (y as i16 as i32) + 0x4000;
    (p >> 15).clamp(i16::MIN as i32, i16::MAX as i32) as i16 as u16
}

fn sat8(v: i32, signed: bool) -> u8 {
    if signed {
        v.clamp(i8::MIN as i32, i8::MAX as i32) as i8 as u8
    } else {
        v.clamp(0, u8::MAX as i32) as u8
    }
}

fn sat16(v: i64, signed: bool) -> u16 {
    if signed {
        v.clamp(i16::MIN as i64, i16::MAX as i64) as i16 as u16
    } else {
        v.clamp(0, u16::MAX as i64) as u16
    }
}

fn lanes16(a: u128) -> [i32; 8] {
    let b = a.to_le_bytes();
    let mut o = [0i32; 8];
    for i in 0..8 {
        o[i] = i16::from_le_bytes([b[i * 2], b[i * 2 + 1]]) as i32;
    }
    o
}

fn lanes16u(a: u128) -> [i32; 8] {
    let b = a.to_le_bytes();
    let mut o = [0i32; 8];
    for i in 0..8 {
        o[i] = u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]) as i32;
    }
    o
}

fn narrow8(a: u128, b: u128, signed: bool) -> u128 {
    let mut o = [0u8; 16];
    let src = [a, b];
    for (half, v) in src.into_iter().enumerate() {
        let lanes = lanes16(v);
        for i in 0..8 {
            o[half * 8 + i] = sat8(lanes[i], signed);
        }
    }
    u128::from_le_bytes(o)
}

fn lanes32(a: u128) -> [i64; 4] {
    let b = a.to_le_bytes();
    let mut o = [0i64; 4];
    for i in 0..4 {
        o[i] = i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()) as i64;
    }
    o
}

fn lanes32u(a: u128) -> [i64; 4] {
    let b = a.to_le_bytes();
    let mut o = [0i64; 4];
    for i in 0..4 {
        o[i] = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()) as i64;
    }
    o
}

fn narrow16(a: u128, b: u128, signed: bool) -> u128 {
    let mut o = [0u8; 16];
    let src = [a, b];
    for (half, v) in src.into_iter().enumerate() {
        let lanes = lanes32(v);
        for i in 0..4 {
            let w = sat16(lanes[i], signed).to_le_bytes();
            o[half * 8 + i * 2..half * 8 + i * 2 + 2].copy_from_slice(&w);
        }
    }
    u128::from_le_bytes(o)
}

fn extend8(a: u128, high: bool, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let start = if high { 8 } else { 0 };
    let mut o = [0u8; 16];
    for i in 0..8 {
        let v = if signed {
            b[start + i] as i8 as i16 as u16
        } else {
            b[start + i] as u16
        };
        o[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn extend16(a: u128, high: bool, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let start = if high { 4 } else { 0 };
    let mut o = [0u8; 16];
    for i in 0..4 {
        let s = (start + i) * 2;
        let v = u16::from_le_bytes([b[s], b[s + 1]]);
        let w = if signed {
            v as i16 as i32 as u32
        } else {
            v as u32
        };
        o[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn extend32(a: u128, high: bool, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let start = if high { 2 } else { 0 };
    let mut o = [0u8; 16];
    for i in 0..2 {
        let s = (start + i) * 4;
        let v = u32::from_le_bytes(b[s..s + 4].try_into().unwrap());
        let w = if signed {
            v as i32 as i64 as u64
        } else {
            v as u64
        };
        o[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn extmul8(a: u128, b: u128, high: bool, signed: bool) -> u128 {
    zip16b(
        extend8(a, high, signed),
        extend8(b, high, signed),
        |x, y| x.wrapping_mul(y),
    )
}

fn extmul16(a: u128, b: u128, high: bool, signed: bool) -> u128 {
    let aa = extend16(a, high, signed).to_le_bytes();
    let bb = extend16(b, high, signed).to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..4 {
        let x = u32::from_le_bytes(aa[i * 4..i * 4 + 4].try_into().unwrap());
        let y = u32::from_le_bytes(bb[i * 4..i * 4 + 4].try_into().unwrap());
        o[i * 4..i * 4 + 4].copy_from_slice(&x.wrapping_mul(y).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn extmul32(a: u128, b: u128, high: bool, signed: bool) -> u128 {
    zip64b(
        extend32(a, high, signed),
        extend32(b, high, signed),
        |x, y| x.wrapping_mul(y),
    )
}

fn zip64b(a: u128, b: u128, f: impl Fn(u64, u64) -> u64) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let x = u64::from_le_bytes(aa[i * 8..i * 8 + 8].try_into().unwrap());
        let y = u64::from_le_bytes(bb[i * 8..i * 8 + 8].try_into().unwrap());
        o[i * 8..i * 8 + 8].copy_from_slice(&f(x, y).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn extadd8(a: u128, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..8 {
        let (x, y) = if signed {
            (b[i * 2] as i8 as i16, b[i * 2 + 1] as i8 as i16)
        } else {
            (b[i * 2] as i16, b[i * 2 + 1] as i16)
        };
        o[i * 2..i * 2 + 2].copy_from_slice(&(x.wrapping_add(y) as u16).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn extadd16(a: u128, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..4 {
        let s = i * 4;
        let x = u16::from_le_bytes([b[s], b[s + 1]]);
        let y = u16::from_le_bytes([b[s + 2], b[s + 3]]);
        let v = if signed {
            (x as i16 as i32).wrapping_add(y as i16 as i32) as u32
        } else {
            (x as i32).wrapping_add(y as i32) as u32
        };
        o[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn dot16(a: u128, b: u128) -> u128 {
    let aa = lanes16(a);
    let bb = lanes16(b);
    let mut o = [0u8; 16];
    for i in 0..4 {
        let v = aa[i * 2]
            .wrapping_mul(bb[i * 2])
            .wrapping_add(aa[i * 2 + 1].wrapping_mul(bb[i * 2 + 1]));
        o[i * 4..i * 4 + 4].copy_from_slice(&(v as u32).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn trunc_f64(a: u128, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let x = f64::from_bits(u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap()));
        let v = if signed {
            trunc_sat_s64(x)
        } else {
            trunc_sat_u64(x)
        };
        o[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn trunc_sat_s64(x: f64) -> u32 {
    if x.is_nan() {
        0
    } else if x >= i32::MAX as f64 {
        i32::MAX as u32
    } else if x <= i32::MIN as f64 {
        i32::MIN as u32
    } else {
        x as i32 as u32
    }
}

fn trunc_sat_u64(x: f64) -> u32 {
    if x.is_nan() || x <= 0.0 {
        0
    } else if x >= u32::MAX as f64 {
        u32::MAX
    } else {
        x as u32
    }
}

fn convert_low(a: u128, signed: bool) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let v = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        let f = if signed { v as i32 as f64 } else { v as f64 };
        o[i * 8..i * 8 + 8].copy_from_slice(&f.to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn demote(a: u128) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let f = f64::from_bits(u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())) as f32;
        o[i * 4..i * 4 + 4].copy_from_slice(&f.to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn promote(a: u128) -> u128 {
    let b = a.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let f = f32::from_bits(u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())) as f64;
        o[i * 8..i * 8 + 8].copy_from_slice(&f.to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn laneselect(a: u128, b: u128, c: u128, width: usize) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let cc = c.to_le_bytes();
    let mut o = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        let msb = cc[i + width - 1] & 0x80 != 0;
        let src = if msb { &aa } else { &bb };
        o[i..i + width].copy_from_slice(&src[i..i + width]);
        i += width;
    }
    u128::from_le_bytes(o)
}

fn relaxed_dot_i16(a: u128, b: u128) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..8 {
        let p0 = i32::from(aa[i * 2] as i8) * i32::from(bb[i * 2] as i8);
        let p1 = i32::from(aa[i * 2 + 1] as i8) * i32::from(bb[i * 2 + 1] as i8);
        o[i * 2..i * 2 + 2].copy_from_slice(&(p0.wrapping_add(p1) as i16).to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn relaxed_dot_i32_add(a: u128, b: u128, c: u128) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let cc = c.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..4 {
        let mut s = i32::from_le_bytes(cc[i * 4..i * 4 + 4].try_into().unwrap());
        for k in 0..4 {
            let p = i32::from(aa[i * 4 + k] as i8) * i32::from(bb[i * 4 + k] as i8);
            s = s.wrapping_add(p);
        }
        o[i * 4..i * 4 + 4].copy_from_slice(&s.to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn maddf32(a: u128, b: u128, c: u128, neg: bool) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let cc = c.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..4 {
        let x = f32::from_bits(u32::from_le_bytes(aa[i * 4..i * 4 + 4].try_into().unwrap()));
        let y = f32::from_bits(u32::from_le_bytes(bb[i * 4..i * 4 + 4].try_into().unwrap()));
        let z = f32::from_bits(u32::from_le_bytes(cc[i * 4..i * 4 + 4].try_into().unwrap()));
        let v = if neg { -(x * y) + z } else { x * y + z };
        o[i * 4..i * 4 + 4].copy_from_slice(&v.to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}

fn maddf64(a: u128, b: u128, c: u128, neg: bool) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let cc = c.to_le_bytes();
    let mut o = [0u8; 16];
    for i in 0..2 {
        let x = f64::from_bits(u64::from_le_bytes(aa[i * 8..i * 8 + 8].try_into().unwrap()));
        let y = f64::from_bits(u64::from_le_bytes(bb[i * 8..i * 8 + 8].try_into().unwrap()));
        let z = f64::from_bits(u64::from_le_bytes(cc[i * 8..i * 8 + 8].try_into().unwrap()));
        let v = if neg { -(x * y) + z } else { x * y + z };
        o[i * 8..i * 8 + 8].copy_from_slice(&v.to_bits().to_le_bytes());
    }
    u128::from_le_bytes(o)
}
