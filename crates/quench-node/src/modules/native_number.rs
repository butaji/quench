//! Integer coercion used by native Node bindings after Core ToNumber.
pub(crate) fn uint32(number: f64) -> u32 {
    const UINT32_MODULUS: f64 = u32::MAX as f64 + 1.0;
    if !number.is_finite() || number == 0.0 {
        0
    } else {
        number.trunc().rem_euclid(UINT32_MODULUS) as u32
    }
}
