//! In-place fast paths for `Number.prototype.toFixed` and `Number.prototype.toString` on Numbers,
//! and for `Object.getPrototypeOf` of an ordinary object: each completes the call with no guest
//! code and no raised error, or answers `None` so the general native runs.
use super::super::*;
use crate::vm::number::{FIXED_EXPONENTIAL_FROM, fixed_text};
use smallvec::SmallVec;

/// The largest digit count `toFixed` takes here; the general path takes up to 100.
const MAX_FIXED_DIGITS: usize = 20;
/// Inline capacity of a Number's decimal text (`number_to_string::write`).
const NUMBER_INLINE_LEN: usize = 32;
/// The radix of `Number.prototype.toString` with no argument, and of its decimal text.
const DECIMAL_RADIX: i32 = 10;
/// The smallest radix `toString` takes here.
const MIN_RADIX: i32 = 2;
/// The largest radix `toString` takes here.
const MAX_RADIX: i32 = 36;
use super::super::string_extra::with_radix_text;

impl Vm {
    /// `Number.prototype.toFixed(digits)` of a Number receiver (a double or an int32) with
    /// `digits` absent or an int32 in 0..=20. Finite magnitudes below 1e21 are written by
    /// `fixed_text`; NaN, the infinities, larger magnitudes and any other case answer `None`.
    pub(in crate::vm) fn plain_number_to_fixed(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let number = receiver.as_number()?;
        if !number.is_finite() || number.abs() >= FIXED_EXPONENTIAL_FROM {
            return None;
        }
        let digits = fixed_digit_count(args.first().copied())?;
        Some(self.heap.alloc_ascii_run(&fixed_text(number, digits)))
    }

    /// `Number.prototype.toString(radix)` of a Number receiver. Radix 10 or absent writes the
    /// Number's shortest decimal text (`number_to_string::write`), as the general path does. A
    /// finite Number with a radix in 2..=36 writes its digits in that radix (`radix_text`); NaN,
    /// the infinities and any other case answer `None`.
    pub(in crate::vm) fn plain_number_to_string(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let radix = match args.first().copied() {
            None => DECIMAL_RADIX,
            Some(value) if value.is_undefined() => DECIMAL_RADIX,
            Some(value) => value.as_int()?,
        };
        let number = receiver.as_number()?;
        if radix == DECIMAL_RADIX {
            let mut text = SmallVec::<[u8; NUMBER_INLINE_LEN]>::new();
            crate::number_to_string::write(number, &mut text);
            return Some(self.heap.alloc_ascii_run(&text));
        }
        if !(MIN_RADIX..=MAX_RADIX).contains(&radix) || !number.is_finite() {
            return None;
        }
        Some(with_radix_text(number, radix as u32, |text| self.heap.alloc_ascii_run(text)))
    }

    /// `Object.getPrototypeOf(object)` of an ordinary object (`Heap::ordinary_object`), whose
    /// prototype is the record's own link. Proxies, primitives and every other argument answer
    /// `None`, which the general native handles (the receiver is ignored, as the call ignores it).
    pub(in crate::vm) fn plain_object_get_prototype_of(&mut self, args: &[Value]) -> Option<Value> {
        let target = args.first().copied()?;
        if self.proxy_parts(target).is_some() {
            return None;
        }
        Some(self.heap.ordinary_object(target)?.proto())
    }
}

/// The digit count of a `toFixed` argument: 0 when absent or `undefined`, else an int32 in
/// 0..=MAX_FIXED_DIGITS. `None` for any other argument, which the general path converts.
fn fixed_digit_count(digits: Option<Value>) -> Option<usize> {
    match digits {
        None => Some(0),
        Some(value) if value.is_undefined() => Some(0),
        Some(value) => usize::try_from(value.as_int()?).ok().filter(|&count| count <= MAX_FIXED_DIGITS),
    }
}
