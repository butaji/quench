//! `Array.prototype.sort` without a comparator: SortCompare orders two values by the code
//! units of their ToString.
use super::*;

impl Vm {
    /// Sorts `values` (none `undefined`) as SortCompare without a comparator orders them.
    /// ToString of a value that is no object runs no guest code, so when none is one each
    /// value's string is made once and the values follow their strings' order; an object's
    /// ToString is called at each comparison, as the comparison calls it.
    pub(super) fn sort_by_string_order(&mut self, p: &ResidualProgram, values: &mut Vec<Value>) -> Result<(), JsError> {
        // A lone value is never compared, so it is not converted.
        if values.len() <= 1 || values.iter().any(|value| self.is_object_like(*value)) {
            return crate::vm::sort::try_stable_sort_by(values, |left, right| {
                let (left, right) = (self.coerce_js_string(p, *left)?, self.coerce_js_string(p, *right)?);
                Ok(left.order(&right))
            });
        }
        // Integers order by their decimal text without making it: one key each
        // (`int_string_key`), then a stable sort of the keys.
        if values.iter().all(|value| value.as_int().is_some()) {
            let mut keyed: Vec<(u64, Value)> = values.iter().map(|value| (int_string_key(value.as_int().unwrap_or(0)), *value)).collect();
            keyed.sort_by_key(|(key, _)| *key);
            for (slot, (_, value)) in values.iter_mut().zip(keyed) {
                *slot = value;
            }
            return Ok(());
        }
        let keys = values.iter().map(|value| self.coerce_js_string(p, *value)).collect::<Result<Vec<_>, _>>()?;
        let mut order: Vec<usize> = (0..values.len()).collect();
        crate::vm::sort::try_stable_sort_by(&mut order, |left, right| Ok::<_, JsError>(keys[*left].order(&keys[*right])))?;
        *values = order.iter().map(|index| values[*index]).collect();
        Ok(())
    }
}

/// The digits of a magnitude below ten, and what each power of ten adds.
const ONE_DIGIT: u32 = 1;
/// The base of the text the key orders.
const TEXT_RADIX: u64 = 10;
/// The most decimal digits of an int32's magnitude.
const INT_DIGITS: u32 = 10;
/// Bits of `int_string_key` below the scaled digits: the digit count.
const DIGIT_COUNT_BITS: u32 = 4;
/// Bit of `int_string_key` that sets a non-negative value's text after every negative one's.
const SIGN_BIT: u32 = 40;

/// A key whose order is the order of `value`'s decimal text (ToString, then code unit
/// order): `-` sorts before every digit, so the sign comes first; then the magnitude's digits
/// left-aligned to ten places, which order as the texts do up to a prefix; then the digit
/// count, by which a prefix comes first.
fn int_string_key(value: i32) -> u64 {
    let magnitude = u64::from(value.unsigned_abs());
    let digits = magnitude.checked_ilog10().map_or(ONE_DIGIT, |log| log + ONE_DIGIT);
    let scaled = magnitude * TEXT_RADIX.pow(INT_DIGITS - digits);
    (u64::from(value >= 0) << SIGN_BIT) | (scaled << DIGIT_COUNT_BITS) | u64::from(digits)
}

#[cfg(test)]
#[path = "array_default_sort_tests.rs"]
mod tests;
