// What the Number rows convert by, made of the binary64 bits so no row names libm or a constant
// pool: ToInt32 as the staged Number rows name it and the clamp the typed elements store by. The
// runtime's own are of the value (`crate::units::to_int32`, `Concrete::clamp_u8`), and the runtime's
// `stencil_units_match` test holds the two to one another.

/// The fraction's bits, and the exponent's, of a binary64.
const FRACTION_BITS: u32 = f64::MANTISSA_DIGITS - 1;
const EXPONENT_BITS: u32 = u64::BITS - 1 - FRACTION_BITS;
const EXPONENT_MASK: u64 = (1 << EXPONENT_BITS) - 1;
/// The exponent of 1.0, and the fraction's implicit leading bit.
const EXPONENT_BIAS: i32 = f64::MAX_EXP - 1;
const IMPLICIT_BIT: u64 = 1 << FRACTION_BITS;

/// A finite or infinite binary64 as its sign, its unbiased exponent and its 53-bit significand: the
/// value is `significand * 2^(exponent - FRACTION_BITS)`. NaN has the largest exponent, as an infinity does.
pub(crate) struct Unpacked {
    pub(crate) negative: bool,
    pub(crate) exponent: i32,
    pub(crate) significand: u64,
    pub(crate) infinite_or_nan: bool,
}

#[inline(always)]
pub(crate) fn unpack(bits: u64) -> Unpacked {
    let biased = (bits >> FRACTION_BITS) & EXPONENT_MASK;
    Unpacked {
        negative: bits >> (u64::BITS - 1) != 0,
        exponent: biased as i32 - EXPONENT_BIAS,
        significand: (bits & (IMPLICIT_BIT - 1)) | IMPLICIT_BIT,
        infinite_or_nan: biased == EXPONENT_MASK,
    }
}

/// ToInt32, as the staged Number rows name it: the integer part modulo 2^32, which is the low 32 bits of the
/// significand shifted to its place (none of them where it lies past 32 bits, in NaN and the infinities, or
/// where the value is below 1 in magnitude).
#[inline(always)]
pub(crate) fn to_int32(value: f64) -> i32 {
    let Unpacked { negative, exponent, significand, .. } = unpack(value.to_bits());
    let whole = if exponent < 0 {
        0
    } else if exponent >= FRACTION_BITS as i32 {
        let shift = (exponent - FRACTION_BITS as i32) as u32;
        if shift >= u32::BITS { 0 } else { (significand << shift) as u32 }
    } else {
        (significand >> (FRACTION_BITS as i32 - exponent) as u32) as u32
    };
    (if negative { whole.wrapping_neg() } else { whole }) as i32
}

/// ToUint8Clamp of a binary64's bits: the staged body `math::clamp_u8`, which this build mounts.
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
#[inline(always)]
pub(crate) fn clamp_u8(bits: u64) -> u64 {
    u64::from(crate::staged::math::clamp_u8(f64::from_bits(bits)))
}
