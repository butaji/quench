//! Integer coercion used by native Node bindings after Core ToNumber.
pub(crate) fn uint32(number: f64) -> u32 {
    use crate::modules::zlib_shared_vm::UINT32_MODULUS;
    if !number.is_finite() || number == 0.0 {
        0
    } else {
        number.trunc().rem_euclid(UINT32_MODULUS) as u32
    }
}
