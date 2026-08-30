//! f32/f64 Native kernels. Bits are the representation; IEEE ops are derived.

pub const CANON_F32: u32 = 0x7fc0_0000;
pub const CANON_F64: u64 = 0x7ff8_0000_0000_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UnF32 {
    Abs,
    Neg,
    Ceil,
    Floor,
    Trunc,
    Nearest,
    Sqrt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BinF32 {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Copysign,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UnF64 {
    Abs,
    Neg,
    Ceil,
    Floor,
    Trunc,
    Nearest,
    Sqrt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BinF64 {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Copysign,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl UnF32 {
    pub fn apply(self, bits: u32) -> u32 {
        let x = f32::from_bits(bits);
        match self {
            Self::Abs => x.abs().to_bits(),
            Self::Neg => (-x).to_bits(),
            Self::Ceil => canon_f32(x.ceil()),
            Self::Floor => canon_f32(x.floor()),
            Self::Trunc => canon_f32(x.trunc()),
            Self::Nearest => canon_f32(x.round_ties_even()),
            Self::Sqrt => canon_f32(x.sqrt()),
        }
    }
}

impl BinF32 {
    pub fn is_rel(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Le | Self::Ge
        )
    }

    pub fn apply(self, lhs: u32, rhs: u32) -> u32 {
        let a = f32::from_bits(lhs);
        let b = f32::from_bits(rhs);
        match self {
            Self::Add => canon_f32(a + b),
            Self::Sub => canon_f32(a - b),
            Self::Mul => canon_f32(a * b),
            Self::Div => canon_f32(a / b),
            Self::Min => min_f32(a, b),
            Self::Max => max_f32(a, b),
            Self::Copysign => a.copysign(b).to_bits(),
            Self::Eq => u32::from(a == b),
            Self::Ne => u32::from(a != b),
            Self::Lt => u32::from(a < b),
            Self::Gt => u32::from(a > b),
            Self::Le => u32::from(a <= b),
            Self::Ge => u32::from(a >= b),
        }
    }
}

impl UnF64 {
    pub fn apply(self, bits: u64) -> u64 {
        let x = f64::from_bits(bits);
        match self {
            Self::Abs => x.abs().to_bits(),
            Self::Neg => (-x).to_bits(),
            Self::Ceil => canon_f64(x.ceil()),
            Self::Floor => canon_f64(x.floor()),
            Self::Trunc => canon_f64(x.trunc()),
            Self::Nearest => canon_f64(x.round_ties_even()),
            Self::Sqrt => canon_f64(x.sqrt()),
        }
    }
}

impl BinF64 {
    pub fn is_rel(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Le | Self::Ge
        )
    }

    pub fn apply(self, lhs: u64, rhs: u64) -> u64 {
        let a = f64::from_bits(lhs);
        let b = f64::from_bits(rhs);
        match self {
            Self::Add => canon_f64(a + b),
            Self::Sub => canon_f64(a - b),
            Self::Mul => canon_f64(a * b),
            Self::Div => canon_f64(a / b),
            Self::Min => min_f64(a, b),
            Self::Max => max_f64(a, b),
            Self::Copysign => a.copysign(b).to_bits(),
            Self::Eq => u64::from(a == b),
            Self::Ne => u64::from(a != b),
            Self::Lt => u64::from(a < b),
            Self::Gt => u64::from(a > b),
            Self::Le => u64::from(a <= b),
            Self::Ge => u64::from(a >= b),
        }
    }
}

pub(crate) fn canon_f32(x: f32) -> u32 {
    if x.is_nan() {
        CANON_F32
    } else {
        x.to_bits()
    }
}

pub(crate) fn canon_f64(x: f64) -> u64 {
    if x.is_nan() {
        CANON_F64
    } else {
        x.to_bits()
    }
}

fn min_f32(a: f32, b: f32) -> u32 {
    if a.is_nan() || b.is_nan() {
        return CANON_F32;
    }
    if a == 0.0 && b == 0.0 && (a.is_sign_negative() || b.is_sign_negative()) {
        return (-0.0f32).to_bits();
    }
    if a < b { a } else { b }.to_bits()
}

fn max_f32(a: f32, b: f32) -> u32 {
    if a.is_nan() || b.is_nan() {
        return CANON_F32;
    }
    if a == 0.0 && b == 0.0 && (a.is_sign_positive() || b.is_sign_positive()) {
        return 0.0f32.to_bits();
    }
    if a > b { a } else { b }.to_bits()
}

fn min_f64(a: f64, b: f64) -> u64 {
    if a.is_nan() || b.is_nan() {
        return CANON_F64;
    }
    if a == 0.0 && b == 0.0 && (a.is_sign_negative() || b.is_sign_negative()) {
        return (-0.0f64).to_bits();
    }
    if a < b { a } else { b }.to_bits()
}

fn max_f64(a: f64, b: f64) -> u64 {
    if a.is_nan() || b.is_nan() {
        return CANON_F64;
    }
    if a == 0.0 && b == 0.0 && (a.is_sign_positive() || b.is_sign_positive()) {
        return 0.0f64.to_bits();
    }
    if a > b { a } else { b }.to_bits()
}
