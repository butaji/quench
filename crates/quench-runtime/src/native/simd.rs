//! v128 lane kernels. The vector is one Native slot; lanes are derived views.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimdOp {
    And,
    Or,
    Xor,
    Not,
    AndNot,
    Bitselect,
    I8x16Splat,
    I16x8Splat,
    I32x4Splat,
    I64x2Splat,
    F32x4Splat,
    F64x2Splat,
    I8x16Add,
    I8x16Sub,
    I8x16Neg,
    I8x16Eq,
    I8x16Ne,
    I16x8Add,
    I16x8Sub,
    I16x8Neg,
    I16x8Eq,
    I16x8Ne,
    I32x4Add,
    I32x4Sub,
    I32x4Mul,
    I32x4Neg,
    I32x4Eq,
    I32x4Ne,
    I64x2Add,
    I64x2Sub,
    I64x2Mul,
    I64x2Neg,
    I64x2Eq,
    I64x2Ne,
    F32x4Add,
    F32x4Sub,
    F32x4Mul,
    F32x4Div,
    F64x2Add,
    F64x2Sub,
    F64x2Mul,
    F64x2Div,
    I8x16Abs,
    I8x16MinS,
    I8x16MinU,
    I8x16MaxS,
    I8x16MaxU,
    I32x4Abs,
    I32x4MinS,
    I32x4MinU,
    I32x4MaxS,
    I32x4MaxU,
    F32x4Abs,
    F32x4Neg,
    F32x4Sqrt,
    F32x4Min,
    F32x4Max,
    F32x4Eq,
    F32x4Ne,
    F32x4Lt,
    F32x4Gt,
    F32x4Le,
    F32x4Ge,
    F64x2Abs,
    F64x2Neg,
    F64x2Sqrt,
    F64x2Min,
    F64x2Max,
    F64x2Eq,
    F64x2Ne,
    F64x2Lt,
    F64x2Gt,
    F64x2Le,
    F64x2Ge,
    Swizzle,
    I8x16Mul,
    I16x8Mul,
    I16x8Abs,
    I16x8MinS,
    I16x8MinU,
    I16x8MaxS,
    I16x8MaxU,
    I8x16AddSatS,
    I8x16AddSatU,
    I8x16SubSatS,
    I8x16SubSatU,
    I16x8AddSatS,
    I16x8AddSatU,
    I16x8SubSatS,
    I16x8SubSatU,
    I8x16Shl,
    I8x16ShrS,
    I8x16ShrU,
    I16x8Shl,
    I16x8ShrS,
    I16x8ShrU,
    I32x4Shl,
    I32x4ShrS,
    I32x4ShrU,
    I64x2Shl,
    I64x2ShrS,
    I64x2ShrU,
    I8x16AnyTrue,
    I16x8AnyTrue,
    I32x4AnyTrue,
    I64x2AnyTrue,
    I8x16AllTrue,
    I16x8AllTrue,
    I32x4AllTrue,
    I64x2AllTrue,
    I8x16Bitmask,
    I16x8Bitmask,
    I32x4Bitmask,
    I64x2Bitmask,
    I8x16ExtractS,
    I8x16ExtractU,
    I16x8ExtractS,
    I16x8ExtractU,
    I32x4Extract,
    I64x2Extract,
    F32x4Extract,
    F64x2Extract,
    I8x16Replace,
    I16x8Replace,
    I32x4Replace,
    I64x2Replace,
    F32x4Replace,
    F64x2Replace,
    F32x4ConvertI32S,
    F32x4ConvertI32U,
    I32x4TruncSatF32S,
    I32x4TruncSatF32U,
    I8x16LtS,
    I8x16LtU,
    I8x16GtS,
    I8x16GtU,
    I8x16LeS,
    I8x16LeU,
    I8x16GeS,
    I8x16GeU,
    I16x8LtS,
    I16x8LtU,
    I16x8GtS,
    I16x8GtU,
    I16x8LeS,
    I16x8LeU,
    I16x8GeS,
    I16x8GeU,
    I32x4LtS,
    I32x4LtU,
    I32x4GtS,
    I32x4GtU,
    I32x4LeS,
    I32x4LeU,
    I32x4GeS,
    I32x4GeU,
    I64x2Lt,
    I64x2Gt,
    I64x2Le,
    I64x2Ge,
    I8x16Popcnt,
    I8x16NarrowS,
    I8x16NarrowU,
    I8x16AvgrU,
    I16x8AvgrU,
    I16x8Q15Mulr,
    I16x8NarrowS,
    I16x8NarrowU,
    I16x8ExtAddPS,
    I16x8ExtAddPU,
    I16x8ExtLowS,
    I16x8ExtHighS,
    I16x8ExtLowU,
    I16x8ExtHighU,
    I16x8ExtMulLowS,
    I16x8ExtMulHighS,
    I16x8ExtMulLowU,
    I16x8ExtMulHighU,
    I32x4ExtAddPS,
    I32x4ExtAddPU,
    I32x4ExtLowS,
    I32x4ExtHighS,
    I32x4ExtLowU,
    I32x4ExtHighU,
    I32x4Dot,
    I32x4ExtMulLowS,
    I32x4ExtMulHighS,
    I32x4ExtMulLowU,
    I32x4ExtMulHighU,
    I64x2Abs,
    I64x2ExtLowS,
    I64x2ExtHighS,
    I64x2ExtLowU,
    I64x2ExtHighU,
    I64x2ExtMulLowS,
    I64x2ExtMulHighS,
    I64x2ExtMulLowU,
    I64x2ExtMulHighU,
    F32x4Ceil,
    F32x4Floor,
    F32x4Trunc,
    F32x4Nearest,
    F32x4PMin,
    F32x4PMax,
    F64x2Ceil,
    F64x2Floor,
    F64x2Trunc,
    F64x2Nearest,
    F64x2PMin,
    F64x2PMax,
    I32x4TruncSatF64S,
    I32x4TruncSatF64U,
    F64x2ConvertLowS,
    F64x2ConvertLowU,
    F32x4DemoteZero,
    F64x2PromoteLow,
    RelaxedLane8,
    RelaxedLane16,
    RelaxedLane32,
    RelaxedLane64,
    RelaxedMaddF32,
    RelaxedNmaddF32,
    RelaxedMaddF64,
    RelaxedNmaddF64,
    I16x8RelaxedDot,
    I32x4RelaxedDotAdd,
}

impl SimdOp {
    pub fn apply(self, a: u128, b: u128) -> u128 {
        match self {
            Self::And => a & b,
            Self::Or => a | b,
            Self::Xor => a ^ b,
            Self::Not => !a,
            Self::AndNot => a & !b,
            Self::Bitselect => a,
            Self::I8x16Splat => splat8(a as u8),
            Self::I16x8Splat => splat16(a as u16),
            Self::I32x4Splat => splat32(a as u32),
            Self::I64x2Splat => splat64(a as u64),
            Self::F32x4Splat => splat32(a as u32),
            Self::F64x2Splat => splat64(a as u64),
            Self::I8x16Add => zip8(a, b, u8::wrapping_add),
            Self::I8x16Sub => zip8(a, b, u8::wrapping_sub),
            Self::I8x16Neg => zip8(0, a, |z, x| z.wrapping_sub(x)),
            Self::I8x16Eq => zip8(a, b, |x, y| if x == y { 0xff } else { 0 }),
            Self::I8x16Ne => zip8(a, b, |x, y| if x != y { 0xff } else { 0 }),
            Self::I16x8Add => zip16(a, b, u16::wrapping_add),
            Self::I16x8Sub => zip16(a, b, u16::wrapping_sub),
            Self::I16x8Neg => zip16(0, a, |z, x| z.wrapping_sub(x)),
            Self::I16x8Eq => zip16(a, b, |x, y| if x == y { 0xffff } else { 0 }),
            Self::I16x8Ne => zip16(a, b, |x, y| if x != y { 0xffff } else { 0 }),
            Self::I32x4Add => zip32(a, b, u32::wrapping_add),
            Self::I32x4Sub => zip32(a, b, u32::wrapping_sub),
            Self::I32x4Mul => zip32(a, b, u32::wrapping_mul),
            Self::I32x4Neg => zip32(0, a, |z, x| z.wrapping_sub(x)),
            Self::I32x4Eq => zip32(a, b, |x, y| if x == y { 0xffff_ffff } else { 0 }),
            Self::I32x4Ne => zip32(a, b, |x, y| if x != y { 0xffff_ffff } else { 0 }),
            Self::I64x2Add => zip64(a, b, u64::wrapping_add),
            Self::I64x2Sub => zip64(a, b, u64::wrapping_sub),
            Self::I64x2Mul => zip64(a, b, u64::wrapping_mul),
            Self::I64x2Neg => zip64(0, a, |z, x| z.wrapping_sub(x)),
            Self::I64x2Eq => zip64(a, b, |x, y| if x == y { u64::MAX } else { 0 }),
            Self::I64x2Ne => zip64(a, b, |x, y| if x != y { u64::MAX } else { 0 }),
            Self::F32x4Add => zipf32(a, b, |x, y| x + y),
            Self::F32x4Sub => zipf32(a, b, |x, y| x - y),
            Self::F32x4Mul => zipf32(a, b, |x, y| x * y),
            Self::F32x4Div => zipf32(a, b, |x, y| x / y),
            Self::F64x2Add => zipf64(a, b, |x, y| x + y),
            Self::F64x2Sub => zipf64(a, b, |x, y| x - y),
            Self::F64x2Mul => zipf64(a, b, |x, y| x * y),
            Self::F64x2Div => zipf64(a, b, |x, y| x / y),
            Self::I8x16Abs => zip8(a, 0, |x, _| (x as i8).unsigned_abs()),
            Self::I8x16MinS => zip8(a, b, |x, y| (x as i8).min(y as i8) as u8),
            Self::I8x16MinU => zip8(a, b, u8::min),
            Self::I8x16MaxS => zip8(a, b, |x, y| (x as i8).max(y as i8) as u8),
            Self::I8x16MaxU => zip8(a, b, u8::max),
            Self::I32x4Abs => zip32(a, 0, |x, _| (x as i32).unsigned_abs()),
            Self::I32x4MinS => zip32(a, b, |x, y| (x as i32).min(y as i32) as u32),
            Self::I32x4MinU => zip32(a, b, u32::min),
            Self::I32x4MaxS => zip32(a, b, |x, y| (x as i32).max(y as i32) as u32),
            Self::I32x4MaxU => zip32(a, b, u32::max),
            Self::F32x4Abs => zipf32(a, 0, |x, _| x.abs()),
            Self::F32x4Neg => zipf32(a, 0, |x, _| -x),
            Self::F32x4Sqrt => zipf32(a, 0, |x, _| x.sqrt()),
            Self::F32x4Min => zipf32(a, b, wasm_minf32),
            Self::F32x4Max => zipf32(a, b, wasm_maxf32),
            Self::F32x4Eq => zip32(a, b, |x, y| {
                u32::from(f32::from_bits(x) == f32::from_bits(y)).wrapping_neg()
            }),
            Self::F32x4Ne => zip32(a, b, |x, y| {
                u32::from(f32::from_bits(x) != f32::from_bits(y)).wrapping_neg()
            }),
            Self::F32x4Lt => zip32(a, b, |x, y| {
                u32::from(f32::from_bits(x) < f32::from_bits(y)).wrapping_neg()
            }),
            Self::F32x4Gt => zip32(a, b, |x, y| {
                u32::from(f32::from_bits(x) > f32::from_bits(y)).wrapping_neg()
            }),
            Self::F32x4Le => zip32(a, b, |x, y| {
                u32::from(f32::from_bits(x) <= f32::from_bits(y)).wrapping_neg()
            }),
            Self::F32x4Ge => zip32(a, b, |x, y| {
                u32::from(f32::from_bits(x) >= f32::from_bits(y)).wrapping_neg()
            }),
            Self::F64x2Abs => zipf64(a, 0, |x, _| x.abs()),
            Self::F64x2Neg => zipf64(a, 0, |x, _| -x),
            Self::F64x2Sqrt => zipf64(a, 0, |x, _| x.sqrt()),
            Self::F64x2Min => zipf64(a, b, wasm_minf64),
            Self::F64x2Max => zipf64(a, b, wasm_maxf64),
            Self::F64x2Eq => zip64(a, b, |x, y| {
                u64::from(f64::from_bits(x) == f64::from_bits(y)).wrapping_neg()
            }),
            Self::F64x2Ne => zip64(a, b, |x, y| {
                u64::from(f64::from_bits(x) != f64::from_bits(y)).wrapping_neg()
            }),
            Self::F64x2Lt => zip64(a, b, |x, y| {
                u64::from(f64::from_bits(x) < f64::from_bits(y)).wrapping_neg()
            }),
            Self::F64x2Gt => zip64(a, b, |x, y| {
                u64::from(f64::from_bits(x) > f64::from_bits(y)).wrapping_neg()
            }),
            Self::F64x2Le => zip64(a, b, |x, y| {
                u64::from(f64::from_bits(x) <= f64::from_bits(y)).wrapping_neg()
            }),
            Self::F64x2Ge => zip64(a, b, |x, y| {
                u64::from(f64::from_bits(x) >= f64::from_bits(y)).wrapping_neg()
            }),
            other => crate::native::simd_extra::apply(other, a, b, 0, 0),
        }
    }

    pub fn apply_ex(self, a: u128, b: u128, c: u128, lane: u8) -> u128 {
        match self {
            Self::And
            | Self::Or
            | Self::Xor
            | Self::Not
            | Self::AndNot
            | Self::I8x16Splat
            | Self::I16x8Splat
            | Self::I32x4Splat
            | Self::I64x2Splat
            | Self::F32x4Splat
            | Self::F64x2Splat
            | Self::I8x16Add
            | Self::I8x16Sub
            | Self::I8x16Neg
            | Self::I8x16Eq
            | Self::I8x16Ne
            | Self::I16x8Add
            | Self::I16x8Sub
            | Self::I16x8Neg
            | Self::I16x8Eq
            | Self::I16x8Ne
            | Self::I32x4Add
            | Self::I32x4Sub
            | Self::I32x4Mul
            | Self::I32x4Neg
            | Self::I32x4Eq
            | Self::I32x4Ne
            | Self::I64x2Add
            | Self::I64x2Sub
            | Self::I64x2Mul
            | Self::I64x2Neg
            | Self::I64x2Eq
            | Self::I64x2Ne
            | Self::F32x4Add
            | Self::F32x4Sub
            | Self::F32x4Mul
            | Self::F32x4Div
            | Self::F64x2Add
            | Self::F64x2Sub
            | Self::F64x2Mul
            | Self::F64x2Div
            | Self::I8x16Abs
            | Self::I8x16MinS
            | Self::I8x16MinU
            | Self::I8x16MaxS
            | Self::I8x16MaxU
            | Self::I32x4Abs
            | Self::I32x4MinS
            | Self::I32x4MinU
            | Self::I32x4MaxS
            | Self::I32x4MaxU
            | Self::F32x4Abs
            | Self::F32x4Neg
            | Self::F32x4Sqrt
            | Self::F32x4Min
            | Self::F32x4Max
            | Self::F32x4Eq
            | Self::F32x4Ne
            | Self::F32x4Lt
            | Self::F32x4Gt
            | Self::F32x4Le
            | Self::F32x4Ge
            | Self::F64x2Abs
            | Self::F64x2Neg
            | Self::F64x2Sqrt
            | Self::F64x2Min
            | Self::F64x2Max
            | Self::F64x2Eq
            | Self::F64x2Ne
            | Self::F64x2Lt
            | Self::F64x2Gt
            | Self::F64x2Le
            | Self::F64x2Ge => self.apply(a, b),
            other => crate::native::simd_extra::apply(other, a, b, c, lane),
        }
    }

    pub fn arity(self) -> u8 {
        match self {
            Self::Not
            | Self::I8x16Splat
            | Self::I16x8Splat
            | Self::I32x4Splat
            | Self::I64x2Splat
            | Self::F32x4Splat
            | Self::F64x2Splat
            | Self::I8x16Neg
            | Self::I16x8Neg
            | Self::I32x4Neg
            | Self::I64x2Neg
            | Self::I8x16Abs
            | Self::I32x4Abs
            | Self::F32x4Abs
            | Self::F32x4Neg
            | Self::F32x4Sqrt
            | Self::F64x2Abs
            | Self::F64x2Neg
            | Self::F64x2Sqrt
            | Self::I16x8Abs
            | Self::I8x16AnyTrue
            | Self::I16x8AnyTrue
            | Self::I32x4AnyTrue
            | Self::I64x2AnyTrue
            | Self::I8x16AllTrue
            | Self::I16x8AllTrue
            | Self::I32x4AllTrue
            | Self::I64x2AllTrue
            | Self::I8x16Bitmask
            | Self::I16x8Bitmask
            | Self::I32x4Bitmask
            | Self::I64x2Bitmask
            | Self::I8x16ExtractS
            | Self::I8x16ExtractU
            | Self::I16x8ExtractS
            | Self::I16x8ExtractU
            | Self::I32x4Extract
            | Self::I64x2Extract
            | Self::F32x4Extract
            | Self::F64x2Extract
            | Self::F32x4ConvertI32S
            | Self::F32x4ConvertI32U
            | Self::I32x4TruncSatF32S
            | Self::I32x4TruncSatF32U
            | Self::I8x16Popcnt
            | Self::I16x8ExtAddPS
            | Self::I16x8ExtAddPU
            | Self::I16x8ExtLowS
            | Self::I16x8ExtHighS
            | Self::I16x8ExtLowU
            | Self::I16x8ExtHighU
            | Self::I32x4ExtAddPS
            | Self::I32x4ExtAddPU
            | Self::I32x4ExtLowS
            | Self::I32x4ExtHighS
            | Self::I32x4ExtLowU
            | Self::I32x4ExtHighU
            | Self::I64x2Abs
            | Self::I64x2ExtLowS
            | Self::I64x2ExtHighS
            | Self::I64x2ExtLowU
            | Self::I64x2ExtHighU
            | Self::F32x4Ceil
            | Self::F32x4Floor
            | Self::F32x4Trunc
            | Self::F32x4Nearest
            | Self::F64x2Ceil
            | Self::F64x2Floor
            | Self::F64x2Trunc
            | Self::F64x2Nearest
            | Self::I32x4TruncSatF64S
            | Self::I32x4TruncSatF64U
            | Self::F64x2ConvertLowS
            | Self::F64x2ConvertLowU
            | Self::F32x4DemoteZero
            | Self::F64x2PromoteLow => 1,
            Self::Bitselect
            | Self::RelaxedLane8
            | Self::RelaxedLane16
            | Self::RelaxedLane32
            | Self::RelaxedLane64
            | Self::RelaxedMaddF32
            | Self::RelaxedNmaddF32
            | Self::RelaxedMaddF64
            | Self::RelaxedNmaddF64
            | Self::I32x4RelaxedDotAdd => 3,
            _ => 2,
        }
    }
}

fn splat8(x: u8) -> u128 {
    u128::from_le_bytes([x; 16])
}
fn splat16(x: u16) -> u128 {
    let b = x.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..8 {
        out[i * 2] = b[0];
        out[i * 2 + 1] = b[1];
    }
    u128::from_le_bytes(out)
}
fn splat32(x: u32) -> u128 {
    let b = x.to_le_bytes();
    let mut out = [0u8; 16];
    for i in 0..4 {
        out[i * 4..i * 4 + 4].copy_from_slice(&b);
    }
    u128::from_le_bytes(out)
}
fn splat64(x: u64) -> u128 {
    let b = x.to_le_bytes();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&b);
    out[8..].copy_from_slice(&b);
    u128::from_le_bytes(out)
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

fn zipf32(a: u128, b: u128, f: impl Fn(f32, f32) -> f32) -> u128 {
    zip32(a, b, |x, y| {
        f(f32::from_bits(x), f32::from_bits(y)).to_bits()
    })
}

fn zipf64(a: u128, b: u128, f: impl Fn(f64, f64) -> f64) -> u128 {
    zip64(a, b, |x, y| {
        f(f64::from_bits(x), f64::from_bits(y)).to_bits()
    })
}

fn wasm_minf32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::from_bits(super::float::CANON_F32)
    } else {
        a.min(b)
    }
}

fn wasm_maxf32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::from_bits(super::float::CANON_F32)
    } else {
        a.max(b)
    }
}

fn wasm_minf64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::from_bits(super::float::CANON_F64)
    } else {
        a.min(b)
    }
}

fn wasm_maxf64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::from_bits(super::float::CANON_F64)
    } else {
        a.max(b)
    }
}
