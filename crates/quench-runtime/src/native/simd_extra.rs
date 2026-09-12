//! Sat, shift, boolean, lane, and 3-input SIMD kernels.

use super::simd::SimdOp;

pub fn apply(op: SimdOp, a: u128, b: u128, c: u128, lane: u8) -> u128 {
    match op {
        SimdOp::Bitselect => (a & c) | (b & !c),
        SimdOp::Swizzle => swizzle(a, b),
        SimdOp::I8x16Mul => zip8(a, b, u8::wrapping_mul),
        SimdOp::I16x8Mul => zip16(a, b, u16::wrapping_mul),
        SimdOp::I16x8Abs => zip16(a, 0, |x, _| (x as i16).unsigned_abs()),
        SimdOp::I16x8MinS => zip16(a, b, |x, y| (x as i16).min(y as i16) as u16),
        SimdOp::I16x8MinU => zip16(a, b, u16::min),
        SimdOp::I16x8MaxS => zip16(a, b, |x, y| (x as i16).max(y as i16) as u16),
        SimdOp::I16x8MaxU => zip16(a, b, u16::max),
        SimdOp::I8x16AddSatS => zip8(a, b, |x, y| (x as i8).saturating_add(y as i8) as u8),
        SimdOp::I8x16AddSatU => zip8(a, b, u8::saturating_add),
        SimdOp::I8x16SubSatS => zip8(a, b, |x, y| (x as i8).saturating_sub(y as i8) as u8),
        SimdOp::I8x16SubSatU => zip8(a, b, u8::saturating_sub),
        SimdOp::I16x8AddSatS => zip16(a, b, |x, y| (x as i16).saturating_add(y as i16) as u16),
        SimdOp::I16x8AddSatU => zip16(a, b, u16::saturating_add),
        SimdOp::I16x8SubSatS => zip16(a, b, |x, y| (x as i16).saturating_sub(y as i16) as u16),
        SimdOp::I16x8SubSatU => zip16(a, b, u16::saturating_sub),
        SimdOp::I8x16Shl => zip8(a, 0, |x, _| x.wrapping_shl(b as u32 % 8)),
        SimdOp::I8x16ShrS => zip8(a, 0, |x, _| ((x as i8) >> (b as u32 % 8)) as u8),
        SimdOp::I8x16ShrU => zip8(a, 0, |x, _| x >> (b as u32 % 8)),
        SimdOp::I16x8Shl => zip16(a, 0, |x, _| x.wrapping_shl(b as u32 % 16)),
        SimdOp::I16x8ShrS => zip16(a, 0, |x, _| ((x as i16) >> (b as u32 % 16)) as u16),
        SimdOp::I16x8ShrU => zip16(a, 0, |x, _| x >> (b as u32 % 16)),
        SimdOp::I32x4Shl => zip32(a, 0, |x, _| x.wrapping_shl(b as u32 % 32)),
        SimdOp::I32x4ShrS => zip32(a, 0, |x, _| ((x as i32) >> (b as u32 % 32)) as u32),
        SimdOp::I32x4ShrU => zip32(a, 0, |x, _| x >> (b as u32 % 32)),
        SimdOp::I64x2Shl => zip64(a, 0, |x, _| x.wrapping_shl(b as u32 % 64)),
        SimdOp::I64x2ShrS => zip64(a, 0, |x, _| ((x as i64) >> (b as u32 % 64)) as u64),
        SimdOp::I64x2ShrU => zip64(a, 0, |x, _| x >> (b as u32 % 64)),
        SimdOp::I8x16AnyTrue => u128::from(any8(a)),
        SimdOp::I16x8AnyTrue => u128::from(any16(a)),
        SimdOp::I32x4AnyTrue => u128::from(any32(a)),
        SimdOp::I64x2AnyTrue => u128::from(any64(a)),
        SimdOp::I8x16AllTrue => u128::from(all8(a)),
        SimdOp::I16x8AllTrue => u128::from(all16(a)),
        SimdOp::I32x4AllTrue => u128::from(all32(a)),
        SimdOp::I64x2AllTrue => u128::from(all64(a)),
        SimdOp::I8x16Bitmask => bitmask8(a),
        SimdOp::I16x8Bitmask => bitmask16(a),
        SimdOp::I32x4Bitmask => bitmask32(a),
        SimdOp::I64x2Bitmask => bitmask64(a),
        SimdOp::I8x16ExtractS => lane8(a, lane) as i8 as i32 as u32 as u128,
        SimdOp::I8x16ExtractU => lane8(a, lane) as u128,
        SimdOp::I16x8ExtractS => lane16(a, lane) as i16 as i32 as u32 as u128,
        SimdOp::I16x8ExtractU => lane16(a, lane) as u128,
        SimdOp::I32x4Extract => lane32(a, lane) as u128,
        SimdOp::I64x2Extract => lane64(a, lane) as u128,
        SimdOp::F32x4Extract => lane32(a, lane) as u128,
        SimdOp::F64x2Extract => lane64(a, lane) as u128,
        SimdOp::I8x16Replace => replace8(a, lane, b as u8),
        SimdOp::I16x8Replace => replace16(a, lane, b as u16),
        SimdOp::I32x4Replace => replace32(a, lane, b as u32),
        SimdOp::I64x2Replace => replace64(a, lane, b),
        SimdOp::F32x4Replace => replace32(a, lane, b as u32),
        SimdOp::F64x2Replace => replace64(a, lane, b),
        SimdOp::F32x4ConvertI32S => zip32(a, 0, |x, _| (x as i32 as f32).to_bits()),
        SimdOp::F32x4ConvertI32U => zip32(a, 0, |x, _| (x as f32).to_bits()),
        SimdOp::I32x4TruncSatF32S => zip32(a, 0, |x, _| trunc_sat_s(f32::from_bits(x))),
        SimdOp::I32x4TruncSatF32U => zip32(a, 0, |x, _| trunc_sat_u(f32::from_bits(x))),
        SimdOp::I8x16LtS => zip8(a, b, |x, y| u8::from((x as i8) < (y as i8)).wrapping_neg()),
        SimdOp::I8x16LtU => zip8(a, b, |x, y| u8::from(x < y).wrapping_neg()),
        SimdOp::I8x16GtS => zip8(a, b, |x, y| u8::from((x as i8) > (y as i8)).wrapping_neg()),
        SimdOp::I8x16GtU => zip8(a, b, |x, y| u8::from(x > y).wrapping_neg()),
        SimdOp::I8x16LeS => zip8(a, b, |x, y| u8::from((x as i8) <= (y as i8)).wrapping_neg()),
        SimdOp::I8x16LeU => zip8(a, b, |x, y| u8::from(x <= y).wrapping_neg()),
        SimdOp::I8x16GeS => zip8(a, b, |x, y| u8::from((x as i8) >= (y as i8)).wrapping_neg()),
        SimdOp::I8x16GeU => zip8(a, b, |x, y| u8::from(x >= y).wrapping_neg()),
        SimdOp::I16x8LtS => zip16(a, b, |x, y| {
            u16::from((x as i16) < (y as i16)).wrapping_neg()
        }),
        SimdOp::I16x8LtU => zip16(a, b, |x, y| u16::from(x < y).wrapping_neg()),
        SimdOp::I16x8GtS => zip16(a, b, |x, y| {
            u16::from((x as i16) > (y as i16)).wrapping_neg()
        }),
        SimdOp::I16x8GtU => zip16(a, b, |x, y| u16::from(x > y).wrapping_neg()),
        SimdOp::I16x8LeS => zip16(a, b, |x, y| {
            u16::from((x as i16) <= (y as i16)).wrapping_neg()
        }),
        SimdOp::I16x8LeU => zip16(a, b, |x, y| u16::from(x <= y).wrapping_neg()),
        SimdOp::I16x8GeS => zip16(a, b, |x, y| {
            u16::from((x as i16) >= (y as i16)).wrapping_neg()
        }),
        SimdOp::I16x8GeU => zip16(a, b, |x, y| u16::from(x >= y).wrapping_neg()),
        SimdOp::I32x4LtS => zip32(a, b, |x, y| {
            u32::from((x as i32) < (y as i32)).wrapping_neg()
        }),
        SimdOp::I32x4LtU => zip32(a, b, |x, y| u32::from(x < y).wrapping_neg()),
        SimdOp::I32x4GtS => zip32(a, b, |x, y| {
            u32::from((x as i32) > (y as i32)).wrapping_neg()
        }),
        SimdOp::I32x4GtU => zip32(a, b, |x, y| u32::from(x > y).wrapping_neg()),
        SimdOp::I32x4LeS => zip32(a, b, |x, y| {
            u32::from((x as i32) <= (y as i32)).wrapping_neg()
        }),
        SimdOp::I32x4LeU => zip32(a, b, |x, y| u32::from(x <= y).wrapping_neg()),
        SimdOp::I32x4GeS => zip32(a, b, |x, y| {
            u32::from((x as i32) >= (y as i32)).wrapping_neg()
        }),
        SimdOp::I32x4GeU => zip32(a, b, |x, y| u32::from(x >= y).wrapping_neg()),
        SimdOp::I64x2Lt => zip64(a, b, |x, y| {
            u64::from((x as i64) < (y as i64)).wrapping_neg()
        }),
        SimdOp::I64x2Gt => zip64(a, b, |x, y| {
            u64::from((x as i64) > (y as i64)).wrapping_neg()
        }),
        SimdOp::I64x2Le => zip64(a, b, |x, y| {
            u64::from((x as i64) <= (y as i64)).wrapping_neg()
        }),
        SimdOp::I64x2Ge => zip64(a, b, |x, y| {
            u64::from((x as i64) >= (y as i64)).wrapping_neg()
        }),
        other => crate::native::simd_more::apply(other, a, b, c),
    }
}

fn trunc_sat_s(x: f32) -> u32 {
    if !x.is_finite() {
        return if x.is_nan() {
            0
        } else if x.is_sign_positive() {
            i32::MAX as u32
        } else {
            i32::MIN as u32
        };
    }
    x as i32 as u32
}

fn trunc_sat_u(x: f32) -> u32 {
    if !x.is_finite() {
        return if x.is_nan() || x.is_sign_negative() {
            0
        } else {
            u32::MAX
        };
    }
    if x < 0.0 {
        0
    } else {
        x as u32
    }
}

pub fn returns_i32(op: SimdOp) -> bool {
    matches!(
        op,
        SimdOp::I8x16AnyTrue
            | SimdOp::I16x8AnyTrue
            | SimdOp::I32x4AnyTrue
            | SimdOp::I64x2AnyTrue
            | SimdOp::I8x16AllTrue
            | SimdOp::I16x8AllTrue
            | SimdOp::I32x4AllTrue
            | SimdOp::I64x2AllTrue
            | SimdOp::I8x16Bitmask
            | SimdOp::I16x8Bitmask
            | SimdOp::I32x4Bitmask
            | SimdOp::I64x2Bitmask
            | SimdOp::I8x16ExtractS
            | SimdOp::I8x16ExtractU
            | SimdOp::I16x8ExtractS
            | SimdOp::I16x8ExtractU
            | SimdOp::I32x4Extract
    )
}

pub fn returns_i64(op: SimdOp) -> bool {
    matches!(op, SimdOp::I64x2Extract)
}

pub fn returns_f32(op: SimdOp) -> bool {
    matches!(op, SimdOp::F32x4Extract)
}

pub fn returns_f64(op: SimdOp) -> bool {
    matches!(op, SimdOp::F64x2Extract)
}

pub fn is_shift(op: SimdOp) -> bool {
    matches!(
        op,
        SimdOp::I8x16Shl
            | SimdOp::I8x16ShrS
            | SimdOp::I8x16ShrU
            | SimdOp::I16x8Shl
            | SimdOp::I16x8ShrS
            | SimdOp::I16x8ShrU
            | SimdOp::I32x4Shl
            | SimdOp::I32x4ShrS
            | SimdOp::I32x4ShrU
            | SimdOp::I64x2Shl
            | SimdOp::I64x2ShrS
            | SimdOp::I64x2ShrU
    )
}

fn zip8(a: u128, b: u128, f: impl Fn(u8, u8) -> u8) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = f(aa[i], bb[i]);
    }
    u128::from_le_bytes(out)
}

fn zip16(a: u128, b: u128, f: impl Fn(u16, u16) -> u16) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..8 {
        let x = u16::from_le_bytes([aa[i * 2], aa[i * 2 + 1]]);
        let y = u16::from_le_bytes([bb[i * 2], bb[i * 2 + 1]]);
        out[i * 2..i * 2 + 2].copy_from_slice(&f(x, y).to_le_bytes());
    }
    u128::from_le_bytes(out)
}

fn zip32(a: u128, b: u128, f: impl Fn(u32, u32) -> u32) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..4 {
        let x = u32::from_le_bytes(aa[i * 4..i * 4 + 4].try_into().unwrap());
        let y = u32::from_le_bytes(bb[i * 4..i * 4 + 4].try_into().unwrap());
        out[i * 4..i * 4 + 4].copy_from_slice(&f(x, y).to_le_bytes());
    }
    u128::from_le_bytes(out)
}

fn zip64(a: u128, b: u128, f: impl Fn(u64, u64) -> u64) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..2 {
        let x = u64::from_le_bytes(aa[i * 8..i * 8 + 8].try_into().unwrap());
        let y = u64::from_le_bytes(bb[i * 8..i * 8 + 8].try_into().unwrap());
        out[i * 8..i * 8 + 8].copy_from_slice(&f(x, y).to_le_bytes());
    }
    u128::from_le_bytes(out)
}

fn swizzle(a: u128, b: u128) -> u128 {
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..16 {
        let idx = bb[i] as usize;
        out[i] = if idx < 16 { aa[idx] } else { 0 };
    }
    u128::from_le_bytes(out)
}

fn lane8(a: u128, lane: u8) -> u8 {
    a.to_le_bytes()[(lane as usize) & 15]
}
fn lane16(a: u128, lane: u8) -> u16 {
    let b = a.to_le_bytes();
    let i = (lane as usize & 7) * 2;
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn lane32(a: u128, lane: u8) -> u32 {
    let b = a.to_le_bytes();
    let i = (lane as usize & 3) * 4;
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn lane64(a: u128, lane: u8) -> u64 {
    let b = a.to_le_bytes();
    let i = (lane as usize & 1) * 8;
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

fn replace8(a: u128, lane: u8, val: u8) -> u128 {
    let mut b = a.to_le_bytes();
    b[(lane as usize) & 15] = val;
    u128::from_le_bytes(b)
}
fn replace16(a: u128, lane: u8, val: u16) -> u128 {
    let mut b = a.to_le_bytes();
    let i = (lane as usize & 7) * 2;
    b[i..i + 2].copy_from_slice(&val.to_le_bytes());
    u128::from_le_bytes(b)
}
fn replace32(a: u128, lane: u8, val: u32) -> u128 {
    let mut b = a.to_le_bytes();
    let i = (lane as usize & 3) * 4;
    b[i..i + 4].copy_from_slice(&val.to_le_bytes());
    u128::from_le_bytes(b)
}
fn replace64(a: u128, lane: u8, val: u128) -> u128 {
    let mut b = a.to_le_bytes();
    let i = (lane as usize & 1) * 8;
    b[i..i + 8].copy_from_slice(&(val as u64).to_le_bytes());
    u128::from_le_bytes(b)
}

fn any8(a: u128) -> u32 {
    u32::from(a.to_le_bytes().iter().any(|x| *x != 0))
}
fn all8(a: u128) -> u32 {
    u32::from(a.to_le_bytes().iter().all(|x| *x != 0))
}
fn any16(a: u128) -> u32 {
    u32::from((0..8).any(|i| lane16(a, i) != 0))
}
fn all16(a: u128) -> u32 {
    u32::from((0..8).all(|i| lane16(a, i) != 0))
}
fn any32(a: u128) -> u32 {
    u32::from((0..4).any(|i| lane32(a, i) != 0))
}
fn all32(a: u128) -> u32 {
    u32::from((0..4).all(|i| lane32(a, i) != 0))
}
fn any64(a: u128) -> u32 {
    u32::from((0..2).any(|i| lane64(a, i) != 0))
}
fn all64(a: u128) -> u32 {
    u32::from((0..2).all(|i| lane64(a, i) != 0))
}

fn bitmask8(a: u128) -> u128 {
    let b = a.to_le_bytes();
    let mut m = 0u32;
    for i in 0..16 {
        if b[i] & 0x80 != 0 {
            m |= 1 << i;
        }
    }
    m as u128
}
fn bitmask16(a: u128) -> u128 {
    let mut m = 0u32;
    for i in 0..8 {
        if lane16(a, i) & 0x8000 != 0 {
            m |= 1 << i;
        }
    }
    m as u128
}
fn bitmask32(a: u128) -> u128 {
    let mut m = 0u32;
    for i in 0..4 {
        if lane32(a, i) & 0x8000_0000 != 0 {
            m |= 1 << i;
        }
    }
    m as u128
}
fn bitmask64(a: u128) -> u128 {
    let mut m = 0u32;
    for i in 0..2 {
        if lane64(a, i) & 0x8000_0000_0000_0000 != 0 {
            m |= 1 << i;
        }
    }
    m as u128
}
