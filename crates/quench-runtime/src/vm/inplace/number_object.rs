//! In-place fast paths for `Number.prototype.toFixed` and `Number.prototype.toString` on Numbers,
//! and for `Object.getPrototypeOf` of an ordinary object: each completes the call with no guest
//! code and no raised error, or answers `None` so the general native runs.
use super::super::*;
use smallvec::SmallVec;

/// The largest digit count `toFixed` takes here; the general path takes up to 100.
const MAX_FIXED_DIGITS: usize = 20;
/// `toFixed` writes exponential form from this magnitude on, which the general path writes.
const FIXED_EXPONENTIAL_FROM: f64 = 1e21;
/// Inline capacity of a `toFixed` text: a sign, 39 integer digits, a point and 20 digits.
const FIXED_INLINE_LEN: usize = 64;
/// Inline capacity of a Number's decimal text (`number_to_string::write`).
const NUMBER_INLINE_LEN: usize = 32;
/// The radix of `Number.prototype.toString` with no argument, and of its decimal text.
const DECIMAL_RADIX: i32 = 10;
/// The smallest radix `toString` takes here.
const MIN_RADIX: i32 = 2;
/// The largest radix `toString` takes here.
const MAX_RADIX: i32 = 36;
/// The digit characters of a radix up to 36, lowercase.
use super::super::string_extra::RADIX_DIGITS;
/// A sign and the 32 binary digits of an int32.
const INT_RADIX_TEXT_LEN: usize = 33;
/// 10^0 through 10^MAX_FIXED_DIGITS, each exact in a u128.
const POWERS_OF_TEN: [u128; MAX_FIXED_DIGITS + 1] = {
    let mut powers = [1u128; MAX_FIXED_DIGITS + 1];
    let mut index = 1;
    while index < powers.len() {
        powers[index] = powers[index - 1] * 10;
        index += 1;
    }
    powers
};

impl<H: Host> Vm<H> {
    /// `Number.prototype.toFixed(digits)` of a Number receiver (a double or an int32) with
    /// `digits` absent or an int32 in 0..=20. Finite magnitudes below 1e21 whose rounding fits
    /// 128 bits are written by `fixed_text`; NaN, the infinities, larger magnitudes and any
    /// other case answer `None`.
    pub(in crate::vm) fn plain_number_to_fixed(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let number = receiver.as_number()?;
        if !number.is_finite() || number.abs() >= FIXED_EXPONENTIAL_FROM {
            return None;
        }
        let digits = fixed_digit_count(args.first().copied())?;
        // A negative zero prints as zero, as the general path has it.
        let number = if number == 0.0 { 0.0 } else { number };
        let text = fixed_text(number, digits)?;
        Some(self.heap.alloc_ascii_run(&text))
    }

    /// `Number.prototype.toString(radix)` of a Number receiver. Radix 10 or absent writes the
    /// Number's shortest decimal text (`number_to_string::write`), as the general path does. An
    /// int32 receiver with a radix in 2..=36 writes its digits in that radix; any other case
    /// answers `None`.
    pub(in crate::vm) fn plain_number_to_string(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let radix = match args.first().copied() {
            None => DECIMAL_RADIX,
            Some(value) if value.is_undefined() => DECIMAL_RADIX,
            Some(value) => value.as_int()?,
        };
        if radix == DECIMAL_RADIX {
            let number = receiver.as_number()?;
            let mut text = SmallVec::<[u8; NUMBER_INLINE_LEN]>::new();
            crate::number_to_string::write(number, &mut text);
            return Some(self.heap.alloc_ascii_run(&text));
        }
        if !(MIN_RADIX..=MAX_RADIX).contains(&radix) {
            return None;
        }
        let value = receiver.as_int()?;
        let mut buffer = [0u8; INT_RADIX_TEXT_LEN];
        let start = write_int_radix(value, radix as u32, &mut buffer);
        Some(self.heap.alloc_ascii_run(&buffer[start..]))
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

/// The `toFixed(digits)` text of a finite `number` below 1e21, whose sign is the number's own
/// (a negative value that rounds to zero keeps its sign, as the spec's step 6 gives it). The
/// magnitude is rounded half up at `digits` places in exact integer arithmetic (`fixed_rounded`),
/// then laid out with its point. `None` where `fixed_rounded` does not apply.
fn fixed_text(number: f64, digits: usize) -> Option<SmallVec<[u8; FIXED_INLINE_LEN]>> {
    let rounded = fixed_rounded(number, digits)?;
    let mut integer_buffer = itoa::Buffer::new();
    let integer = integer_buffer.format(rounded).as_bytes();
    let mut text: SmallVec<[u8; FIXED_INLINE_LEN]> = SmallVec::new();
    if number.is_sign_negative() {
        text.push(b'-');
    }
    if digits == 0 {
        text.extend_from_slice(integer);
        return Some(text);
    }
    // At least one integer digit: leading zeros pad a short text before the point.
    let start = text.len();
    let width = integer.len().max(digits + 1);
    text.resize(start + width - integer.len(), b'0');
    text.extend_from_slice(integer);
    text.insert(start + width - digits, b'.');
    Some(text)
}

/// `round(|number| * 10^digits)` as an integer, a tie taking the larger, computed from the
/// exact significand and binary exponent in 128 bits. `None` when the product or its shift does
/// not fit, leaving the value to the general path's big-integer arithmetic.
fn fixed_rounded(number: f64, digits: usize) -> Option<u128> {
    /// Right shifts at or past this bit count are left to the general path.
    const MAX_RIGHT_SHIFT: u32 = 127;
    let (significand, exponent) = crate::float_layout::binary64_parts(number);
    let scaled = u128::from(significand).checked_mul(*POWERS_OF_TEN.get(digits)?)?;
    if exponent >= 0 {
        let shift = exponent as u32;
        return (shift < scaled.leading_zeros()).then(|| scaled << shift);
    }
    let shift = exponent.unsigned_abs();
    if shift >= MAX_RIGHT_SHIFT {
        return None;
    }
    Some((scaled + (1u128 << (shift - 1))) >> shift)
}

/// Writes `value` in `radix` (2..=36) into the end of `buffer`, with a leading `-` for a
/// negative value, and returns where the text starts.
fn write_int_radix(value: i32, radix: u32, buffer: &mut [u8; INT_RADIX_TEXT_LEN]) -> usize {
    let mut magnitude = value.unsigned_abs();
    let mut start = buffer.len();
    loop {
        start -= 1;
        buffer[start] = RADIX_DIGITS[(magnitude % radix) as usize];
        magnitude /= radix;
        if magnitude == 0 {
            break;
        }
    }
    if value < 0 {
        start -= 1;
        buffer[start] = b'-';
    }
    start
}
