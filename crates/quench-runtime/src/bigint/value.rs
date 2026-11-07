//! The BigInt value a heap cell holds: sign and magnitude with its digits in the cell, as
//! V8's `BigInt` and JSC's `JSBigInt` keep theirs inline after the header. The cell has
//! room for `INLINE_DIGITS`, a 256-bit magnitude; a longer magnitude spills to one
//! allocation of exactly its length.

use super::digits::{self, Digit, DIGIT_BITS};
use super::Error;
use std::cmp::Ordering;
use std::ptr::NonNull;

/// Digits held in the cell: what fits beside the length and sign in a cell's payload.
pub(crate) const INLINE_DIGITS: usize = 4;

/// The largest magnitude, in bits, an operation may produce: V8's `BigInt::kMaxLengthBits`,
/// past which operations throw "Maximum BigInt size exceeded".
pub(crate) const MAX_BITS: usize = 1 << 30;
const MAX_DIGITS: usize = MAX_BITS / DIGIT_BITS as usize;

/// Multiplications whose shorter operand reaches this many digits leave schoolbook for
/// `num-bigint`'s Karatsuba and Toom-3, at its own Karatsuba threshold.
const SCHOOLBOOK_DIGITS: usize = 32;

union Storage {
    inline: [Digit; INLINE_DIGITS],
    spilled: NonNull<Digit>,
}

pub(crate) struct JsBigInt {
    /// Normalized digit count: the top digit is nonzero, and zero has none. Above
    /// `INLINE_DIGITS`, the digits are `spilled`.
    len: u32,
    /// Never set for zero.
    negative: bool,
    storage: Storage,
}

// SAFETY: a spilled magnitude is owned by its one value and never shared or mutated.
unsafe impl Send for JsBigInt {}
unsafe impl Sync for JsBigInt {}

impl JsBigInt {
    pub(crate) const ZERO: Self = Self { len: 0, negative: false, storage: Storage { inline: [0; INLINE_DIGITS] } };

    #[inline(always)]
    fn spills(len: usize) -> bool {
        len > INLINE_DIGITS
    }

    /// The magnitude's digits, least significant first.
    #[inline(always)]
    pub(crate) fn digits(&self) -> &[Digit] {
        let len = self.len as usize;
        // SAFETY: `len` selects the live union field, and spilled storage holds `len` digits.
        unsafe {
            if Self::spills(len) {
                std::slice::from_raw_parts(self.storage.spilled.as_ptr(), len)
            } else {
                self.storage.inline.get_unchecked(..len)
            }
        }
    }

    /// Whether the digits are all in the value, so dropping it releases nothing.
    #[inline(always)]
    pub(crate) fn is_inline(&self) -> bool {
        !Self::spills(self.len as usize)
    }

    #[inline(always)]
    pub(crate) fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// The value of sign `negative` and magnitude `magnitude`, which may carry zero top
    /// digits.
    pub(crate) fn from_digits(negative: bool, magnitude: &[Digit]) -> Self {
        let len = digits::normalized_len(magnitude);
        let magnitude = &magnitude[..len];
        let storage = if Self::spills(len) {
            let spilled: Box<[Digit]> = magnitude.into();
            // SAFETY: a boxed slice's pointer is non-null; `Drop` rebuilds the box from `len`.
            Storage { spilled: unsafe { NonNull::new_unchecked(Box::into_raw(spilled).cast()) } }
        } else {
            let mut inline = [0; INLINE_DIGITS];
            // A loop, not `copy_from_slice`: at most `INLINE_DIGITS` stores, with no call.
            for (slot, &digit) in inline.iter_mut().zip(magnitude) {
                *slot = digit;
            }
            Storage { inline }
        };
        Self { len: len as u32, negative: negative && len != 0, storage }
    }

    /// The value `write` leaves in `capacity` zeroed digits, signed `negative`, of which it
    /// returns the normalized length. Up to one digit past the cell's (a carry), the digits
    /// are written on the stack and taken into the cell whole.
    #[inline(always)]
    fn build(negative: bool, capacity: usize, write: impl FnOnce(&mut [Digit]) -> usize) -> Self {
        if capacity > INLINE_DIGITS + 1 {
            return digits::with_scratch(capacity, |out| {
                let len = write(out);
                Self::from_digits(negative, &out[..len])
            });
        }
        let mut out = [0; INLINE_DIGITS + 1];
        let len = write(&mut out[..capacity]);
        if Self::spills(len) {
            return Self::from_digits(negative, &out[..len]);
        }
        let inline = *out.first_chunk().expect("the buffer holds the cell's digits");
        Self { len: len as u32, negative: negative && len != 0, storage: Storage { inline } }
    }

    /// Fails for a magnitude past `MAX_BITS`.
    fn within_limit(digits: usize) -> Result<(), Error> {
        if digits > MAX_DIGITS { Err(Error::TooLarge) } else { Ok(()) }
    }

    fn negated(mut self) -> Self {
        self.negative = !self.negative && !self.is_zero();
        self
    }

    /// `|a| + |b|` signed `negative`.
    fn magnitude_sum(negative: bool, a: &[Digit], b: &[Digit]) -> Self {
        let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
        Self::build(negative, long.len() + 1, |out| digits::add(long, short, out))
    }

    /// `|a| − |b|` with `a`'s sign `negative`, which flips when `|b|` is larger.
    fn magnitude_difference(negative: bool, a: &[Digit], b: &[Digit]) -> Self {
        match digits::cmp(a, b) {
            Ordering::Equal => Self::ZERO,
            Ordering::Greater => Self::build(negative, a.len(), |out| digits::sub(a, b, out)),
            Ordering::Less => Self::build(!negative, b.len(), |out| digits::sub(b, a, out)),
        }
    }

    pub(crate) fn add(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            Self::magnitude_sum(self.negative, self.digits(), other.digits())
        } else {
            Self::magnitude_difference(self.negative, self.digits(), other.digits())
        }
    }

    pub(crate) fn sub(&self, other: &Self) -> Self {
        if self.negative != other.negative {
            Self::magnitude_sum(self.negative, self.digits(), other.digits())
        } else {
            Self::magnitude_difference(self.negative, self.digits(), other.digits())
        }
    }

    pub(crate) fn mul(&self, other: &Self) -> Result<Self, Error> {
        let (a, b) = (self.digits(), other.digits());
        if a.is_empty() || b.is_empty() {
            return Ok(Self::ZERO);
        }
        let negative = self.negative != other.negative;
        // The product has this many digits or one fewer.
        Self::within_limit(a.len() + b.len() - 1)?;
        if a.len().min(b.len()) >= SCHOOLBOOK_DIGITS {
            return Ok(Self::from(&(self.to_num() * other.to_num())));
        }
        Ok(Self::build(negative, a.len() + b.len(), |out| digits::mul(a, b, out)))
    }

    /// Truncating division and its remainder, whose sign is the dividend's.
    pub(crate) fn div_rem(&self, other: &Self, want_quotient: bool) -> Result<Self, Error> {
        let (a, b) = (self.digits(), other.digits());
        if b.is_empty() {
            return Err(Error::DivisionByZero);
        }
        if digits::cmp(a, b) == Ordering::Less {
            return Ok(if want_quotient { Self::ZERO } else { self.clone() });
        }
        // The wanted part is built in place; the other takes scratch.
        let quotient_len = a.len() - b.len() + 1;
        Ok(if want_quotient {
            Self::build(self.negative != other.negative, quotient_len, |quotient| {
                digits::with_scratch(b.len(), |remainder| digits::div_rem(a, b, quotient, remainder).0)
            })
        } else {
            Self::build(self.negative, b.len(), |remainder| {
                digits::with_scratch(quotient_len, |quotient| digits::div_rem(a, b, quotient, remainder).1)
            })
        })
    }

    pub(crate) fn pow(&self, exponent: &Self) -> Result<Self, Error> {
        if exponent.negative {
            return Err(Error::NegativeExponent);
        }
        let magnitude = self.digits();
        // 0, 1 and −1 to any power stay within one digit.
        match magnitude {
            [] => return Ok(if exponent.is_zero() { Self::from(1_u8) } else { Self::ZERO }),
            [1] => {
                let odd = exponent.digits().first().is_some_and(|digit| digit & 1 == 1);
                return Ok(if odd { self.clone() } else { Self::from(1_u8) });
            }
            _ => {}
        }
        // Any other base gains at least one bit per unit of exponent.
        let exponent = match exponent.digits() {
            [] => 0,
            [digit] => *digit,
            _ => return Err(Error::TooLarge),
        };
        let bits = magnitude.len() * DIGIT_BITS as usize - magnitude[magnitude.len() - 1].leading_zeros() as usize;
        if (bits as u128 - 1).saturating_mul(u128::from(exponent)) + 1 > MAX_BITS as u128 {
            return Err(Error::TooLarge);
        }
        let exponent = u32::try_from(exponent).map_err(|_| Error::TooLarge)?;
        Ok(Self::from(&self.to_num().pow(exponent)))
    }

    /// `self << count` (`left`) or `self >> count`, a negative count reversing it; a right
    /// shift rounds toward negative infinity.
    pub(crate) fn shift(&self, count: &Self, left: bool) -> Result<Self, Error> {
        let left = left != count.negative;
        let a = self.digits();
        let bits = match count.digits() {
            [] => return Ok(self.clone()),
            [digit] => usize::try_from(*digit).unwrap_or(usize::MAX),
            _ => usize::MAX,
        };
        if a.is_empty() {
            return Ok(Self::ZERO);
        }
        if left {
            let whole = bits / DIGIT_BITS as usize;
            Self::within_limit(a.len().saturating_add(whole))?;
            return Ok(Self::build(self.negative, a.len() + whole + 1, |out| digits::shl(a, bits, out)));
        }
        let capacity = a.len().saturating_sub(bits / DIGIT_BITS as usize);
        let mut lost = false;
        let shifted = Self::build(false, capacity, |out| {
            let (len, dropped) = digits::shr(a, bits, out);
            lost = dropped;
            len
        });
        Ok(if self.negative {
            let magnitude = if lost { shifted.add(&Self::from(1_u8)) } else { shifted };
            magnitude.negated()
        } else {
            shifted
        })
    }

    /// A bitwise operator, in two's complement: non-negative operands digit by digit,
    /// any other through `num-bigint`.
    pub(crate) fn bitwise(&self, other: &Self, operator: Bitwise) -> Self {
        if self.negative || other.negative {
            let (a, b) = (self.to_num(), other.to_num());
            return Self::from(&match operator {
                Bitwise::And => a & b,
                Bitwise::Or => a | b,
                Bitwise::Xor => a ^ b,
            });
        }
        let (a, b) = (self.digits(), other.digits());
        let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
        let capacity = if operator == Bitwise::And { short.len() } else { long.len() };
        Self::build(false, capacity, |out| {
            for (index, slot) in out.iter_mut().enumerate() {
                let (x, y) = (long[index], short.get(index).copied().unwrap_or(0));
                *slot = match operator {
                    Bitwise::And => x & y,
                    Bitwise::Or => x | y,
                    Bitwise::Xor => x ^ y,
                };
            }
            digits::normalized_len(out)
        })
    }

    pub(crate) fn negate(&self) -> Self {
        self.clone().negated()
    }

    /// `~self`: −self − 1.
    pub(crate) fn not(&self) -> Self {
        self.add(&Self::from(1_u8)).negated()
    }

    pub(crate) fn to_num(&self) -> num_bigint::BigInt {
        let magnitude = num_bigint::BigUint::from_slice(
            &self.digits().iter().flat_map(|&digit| [digit as u32, (digit >> u32::BITS) as u32]).collect::<Vec<_>>(),
        );
        let sign = match (self.is_zero(), self.negative) {
            (true, _) => num_bigint::Sign::NoSign,
            (false, true) => num_bigint::Sign::Minus,
            (false, false) => num_bigint::Sign::Plus,
        };
        num_bigint::BigInt::from_biguint(sign, magnitude)
    }

    /// The Number nearest the value, ties to even; magnitudes past the finite range are
    /// infinite.
    pub(crate) fn to_f64(&self) -> f64 {
        let magnitude = match self.digits() {
            [] => 0.0,
            [digit] => *digit as f64,
            _ => return super::to_f64(&self.to_num()),
        };
        if self.negative { -magnitude } else { magnitude }
    }

    pub(crate) fn to_str_radix(&self, radix: u32) -> String {
        self.to_num().to_str_radix(radix)
    }

    /// Bytes the magnitude's spilled digits occupy beyond the cell, for heap accounting.
    pub(crate) fn spilled_bytes(&self) -> usize {
        if Self::spills(self.len as usize) { self.len as usize * size_of::<Digit>() } else { 0 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bitwise {
    And,
    Or,
    Xor,
}

impl Drop for JsBigInt {
    fn drop(&mut self) {
        let len = self.len as usize;
        if Self::spills(len) {
            // SAFETY: `from_digits` made this pointer from a boxed slice of `len` digits.
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.storage.spilled.as_ptr(), len)) });
        }
    }
}

impl Clone for JsBigInt {
    fn clone(&self) -> Self {
        if Self::spills(self.len as usize) {
            Self::from_digits(self.negative, self.digits())
        } else {
            // SAFETY: an unspilled value's storage is plain digits.
            Self { len: self.len, negative: self.negative, storage: Storage { inline: unsafe { self.storage.inline } } }
        }
    }
}

impl PartialEq for JsBigInt {
    fn eq(&self, other: &Self) -> bool {
        self.negative == other.negative && self.digits() == other.digits()
    }
}

impl Eq for JsBigInt {}

impl Ord for JsBigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => digits::cmp(self.digits(), other.digits()),
            (true, true) => digits::cmp(other.digits(), self.digits()),
        }
    }
}

impl PartialOrd for JsBigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for JsBigInt {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.negative.hash(state);
        self.digits().hash(state);
    }
}

impl std::fmt::Display for JsBigInt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_str_radix(10))
    }
}

impl std::fmt::Debug for JsBigInt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self}n")
    }
}

impl From<&num_bigint::BigInt> for JsBigInt {
    fn from(value: &num_bigint::BigInt) -> Self {
        Self::from_digits(value.sign() == num_bigint::Sign::Minus, &value.magnitude().to_u64_digits())
    }
}

impl From<u8> for JsBigInt {
    fn from(value: u8) -> Self {
        Self::from_digits(false, &[Digit::from(value)])
    }
}
