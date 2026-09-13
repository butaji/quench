//! Numeric conversions. Trap on invalid trunc; saturating variants clamp.

use crate::native::float::{canon_f32, canon_f64, CANON_F32, CANON_F64};
use crate::unwind::Trap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConvOp {
    I32WrapI64,
    I64ExtendI32S,
    I64ExtendI32U,
    I32TruncF32S,
    I32TruncF32U,
    I32TruncF64S,
    I32TruncF64U,
    I64TruncF32S,
    I64TruncF32U,
    I64TruncF64S,
    I64TruncF64U,
    I32TruncSatF32S,
    I32TruncSatF32U,
    I32TruncSatF64S,
    I32TruncSatF64U,
    I64TruncSatF32S,
    I64TruncSatF32U,
    I64TruncSatF64S,
    I64TruncSatF64U,
    F32ConvertI32S,
    F32ConvertI32U,
    F32ConvertI64S,
    F32ConvertI64U,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI64U,
    F64PromoteF32,
    I32ReinterpretF32,
    I64ReinterpretF64,
    F32ReinterpretI32,
    F64ReinterpretI64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bits {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
}

impl ConvOp {
    pub fn apply(self, src: Bits) -> Result<Bits, Trap> {
        match (self, src) {
            (Self::I32WrapI64, Bits::I64(v)) => Ok(Bits::I32(v as i32)),
            (Self::I64ExtendI32S, Bits::I32(v)) => Ok(Bits::I64(v as i64)),
            (Self::I64ExtendI32U, Bits::I32(v)) => Ok(Bits::I64(v as u32 as i64)),
            (Self::I32TruncF32S, Bits::F32(b)) => {
                trunc_i32(f32::from_bits(b) as f64, true).map(Bits::I32)
            }
            (Self::I32TruncF32U, Bits::F32(b)) => {
                trunc_i32(f32::from_bits(b) as f64, false).map(Bits::I32)
            }
            (Self::I32TruncF64S, Bits::F64(b)) => trunc_i32(f64::from_bits(b), true).map(Bits::I32),
            (Self::I32TruncF64U, Bits::F64(b)) => {
                trunc_i32(f64::from_bits(b), false).map(Bits::I32)
            }
            (Self::I64TruncF32S, Bits::F32(b)) => {
                trunc_i64(f32::from_bits(b) as f64, true).map(Bits::I64)
            }
            (Self::I64TruncF32U, Bits::F32(b)) => {
                trunc_i64(f32::from_bits(b) as f64, false).map(Bits::I64)
            }
            (Self::I64TruncF64S, Bits::F64(b)) => trunc_i64(f64::from_bits(b), true).map(Bits::I64),
            (Self::I64TruncF64U, Bits::F64(b)) => {
                trunc_i64(f64::from_bits(b), false).map(Bits::I64)
            }
            (Self::I32TruncSatF32S, Bits::F32(b)) => {
                Ok(Bits::I32(sat_i32(f32::from_bits(b) as f64, true)))
            }
            (Self::I32TruncSatF32U, Bits::F32(b)) => {
                Ok(Bits::I32(sat_i32(f32::from_bits(b) as f64, false)))
            }
            (Self::I32TruncSatF64S, Bits::F64(b)) => {
                Ok(Bits::I32(sat_i32(f64::from_bits(b), true)))
            }
            (Self::I32TruncSatF64U, Bits::F64(b)) => {
                Ok(Bits::I32(sat_i32(f64::from_bits(b), false)))
            }
            (Self::I64TruncSatF32S, Bits::F32(b)) => {
                Ok(Bits::I64(sat_i64(f32::from_bits(b) as f64, true)))
            }
            (Self::I64TruncSatF32U, Bits::F32(b)) => {
                Ok(Bits::I64(sat_i64(f32::from_bits(b) as f64, false)))
            }
            (Self::I64TruncSatF64S, Bits::F64(b)) => {
                Ok(Bits::I64(sat_i64(f64::from_bits(b), true)))
            }
            (Self::I64TruncSatF64U, Bits::F64(b)) => {
                Ok(Bits::I64(sat_i64(f64::from_bits(b), false)))
            }
            (Self::F32ConvertI32S, Bits::I32(v)) => Ok(Bits::F32(canon_f32(v as f32))),
            (Self::F32ConvertI32U, Bits::I32(v)) => Ok(Bits::F32(canon_f32(v as u32 as f32))),
            (Self::F32ConvertI64S, Bits::I64(v)) => Ok(Bits::F32(canon_f32(v as f32))),
            (Self::F32ConvertI64U, Bits::I64(v)) => Ok(Bits::F32(canon_f32(v as u64 as f32))),
            (Self::F32DemoteF64, Bits::F64(b)) => Ok(Bits::F32(demote(b))),
            (Self::F64ConvertI32S, Bits::I32(v)) => Ok(Bits::F64(canon_f64(v as f64))),
            (Self::F64ConvertI32U, Bits::I32(v)) => Ok(Bits::F64(canon_f64(v as u32 as f64))),
            (Self::F64ConvertI64S, Bits::I64(v)) => Ok(Bits::F64(canon_f64(v as f64))),
            (Self::F64ConvertI64U, Bits::I64(v)) => Ok(Bits::F64(canon_f64(v as u64 as f64))),
            (Self::F64PromoteF32, Bits::F32(b)) => Ok(Bits::F64(promote(b))),
            (Self::I32ReinterpretF32, Bits::F32(b)) => Ok(Bits::I32(b as i32)),
            (Self::I64ReinterpretF64, Bits::F64(b)) => Ok(Bits::I64(b as i64)),
            (Self::F32ReinterpretI32, Bits::I32(v)) => Ok(Bits::F32(v as u32)),
            (Self::F64ReinterpretI64, Bits::I64(v)) => Ok(Bits::F64(v as u64)),
            _ => Err(Trap::Unimplemented),
        }
    }
}

fn trunc_i32(x: f64, signed: bool) -> Result<i32, Trap> {
    if x.is_nan() {
        return Err(Trap::InvalidConversion);
    }
    if !x.is_finite() {
        return Err(Trap::IntegerOverflow);
    }
    let t = x.trunc();
    if signed {
        if !(i32::MIN as f64..=i32::MAX as f64).contains(&t) {
            return Err(Trap::IntegerOverflow);
        }
        Ok(t as i32)
    } else if !(0.0..=u32::MAX as f64).contains(&t) {
        Err(Trap::IntegerOverflow)
    } else {
        Ok(t as u32 as i32)
    }
}

fn trunc_i64(x: f64, signed: bool) -> Result<i64, Trap> {
    if x.is_nan() {
        return Err(Trap::InvalidConversion);
    }
    if !x.is_finite() {
        return Err(Trap::IntegerOverflow);
    }
    let t = x.trunc();
    if signed {
        if t < i64::MIN as f64 || t >= i64::MAX as f64 + 1.0 {
            return Err(Trap::IntegerOverflow);
        }
        Ok(t as i64)
    } else if t < 0.0 || t >= u64::MAX as f64 + 1.0 {
        Err(Trap::IntegerOverflow)
    } else {
        Ok(t as u64 as i64)
    }
}

fn sat_i32(x: f64, signed: bool) -> i32 {
    if x.is_nan() {
        return 0;
    }
    let t = x.trunc();
    if signed {
        if t <= i32::MIN as f64 {
            i32::MIN
        } else if t >= i32::MAX as f64 {
            i32::MAX
        } else {
            t as i32
        }
    } else if t <= 0.0 {
        0
    } else if t >= u32::MAX as f64 {
        u32::MAX as i32
    } else {
        t as u32 as i32
    }
}

fn sat_i64(x: f64, signed: bool) -> i64 {
    if x.is_nan() {
        return 0;
    }
    let t = x.trunc();
    if signed {
        if t <= i64::MIN as f64 {
            i64::MIN
        } else if t >= i64::MAX as f64 {
            i64::MAX
        } else {
            t as i64
        }
    } else if t <= 0.0 {
        0
    } else if t >= u64::MAX as f64 {
        u64::MAX as i64
    } else {
        t as u64 as i64
    }
}

fn demote(bits: u64) -> u32 {
    let x = f64::from_bits(bits) as f32;
    if x.is_nan() {
        CANON_F32
    } else {
        x.to_bits()
    }
}

fn promote(bits: u32) -> u64 {
    let x = f32::from_bits(bits) as f64;
    if x.is_nan() {
        CANON_F64
    } else {
        x.to_bits()
    }
}
