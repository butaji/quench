//! Magnitude arithmetic on little-endian digit slices: schoolbook addition, subtraction,
//! multiplication and Knuth's Algorithm D (TAOCP vol. 2, 4.3.1), with each two-by-one
//! digit division a multiplication by the divisor's reciprocal (Möller and Granlund,
//! "Improved division by invariant integers", 2011, Algorithm 4), as V8's
//! `src/bigint/div-schoolbook.cc` does. Every function takes normalized inputs (no zero
//! top digit; zero is the empty slice) unless it says otherwise, and returns the
//! normalized length of what it wrote.

use std::cmp::Ordering;

pub(crate) type Digit = u64;
type Wide = u128;
pub(crate) const DIGIT_BITS: u32 = Digit::BITS;

/// Digits of scratch a call keeps on the stack; longer operands take a heap buffer. Two
/// eight-digit (512-bit) operands' product fits.
pub(crate) const STACK_DIGITS: usize = 16;

/// `f` over `len` zeroed digits of scratch.
#[inline(always)]
pub(crate) fn with_scratch<R>(len: usize, f: impl FnOnce(&mut [Digit]) -> R) -> R {
    if len <= STACK_DIGITS {
        let mut buffer = [0; STACK_DIGITS];
        f(&mut buffer[..len])
    } else {
        f(&mut vec![0; len])
    }
}

/// The length of `digits` without its zero top digits.
#[inline(always)]
pub(crate) fn normalized_len(digits: &[Digit]) -> usize {
    digits.iter().rposition(|&digit| digit != 0).map_or(0, |top| top + 1)
}

#[inline(always)]
fn split(wide: Wide) -> (Digit, Digit) {
    (wide as Digit, (wide >> DIGIT_BITS) as Digit)
}

#[inline(always)]
fn join(high: Digit, low: Digit) -> Wide {
    (Wide::from(high) << DIGIT_BITS) | Wide::from(low)
}

pub(crate) fn cmp(a: &[Digit], b: &[Digit]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.iter().rev().cmp(b.iter().rev()))
}

/// `a + b` into `out`, `a.len() >= b.len()`, `out.len() > a.len()`.
pub(crate) fn add(a: &[Digit], b: &[Digit], out: &mut [Digit]) -> usize {
    let mut carry = false;
    for (index, slot) in out[..a.len()].iter_mut().enumerate() {
        let (sum, first) = a[index].overflowing_add(b.get(index).copied().unwrap_or(0));
        let (sum, second) = sum.overflowing_add(Digit::from(carry));
        *slot = sum;
        carry = first | second;
    }
    out[a.len()] = Digit::from(carry);
    a.len() + usize::from(carry)
}

/// `a - b` into `out`, `a >= b`, `out.len() >= a.len()`.
pub(crate) fn sub(a: &[Digit], b: &[Digit], out: &mut [Digit]) -> usize {
    let mut borrow = false;
    for (index, slot) in out[..a.len()].iter_mut().enumerate() {
        let (difference, first) = a[index].overflowing_sub(b.get(index).copied().unwrap_or(0));
        let (difference, second) = difference.overflowing_sub(Digit::from(borrow));
        *slot = difference;
        borrow = first | second;
    }
    debug_assert!(!borrow);
    normalized_len(&out[..a.len()])
}

/// `a * b` into `out`, zeroed and `a.len() + b.len()` long.
pub(crate) fn mul(a: &[Digit], b: &[Digit], out: &mut [Digit]) -> usize {
    for (shift, &factor) in b.iter().enumerate() {
        let row = &mut out[shift..=shift + a.len()];
        let mut carry: Digit = 0;
        for (slot, &digit) in row.iter_mut().zip(a) {
            let (low, high) =
                split(Wide::from(digit) * Wide::from(factor) + Wide::from(*slot) + Wide::from(carry));
            *slot = low;
            carry = high;
        }
        row[a.len()] = carry;
    }
    normalized_len(out)
}

/// `a << bits` into `out`, zeroed and `a.len() + bits / DIGIT_BITS + 1` long.
pub(crate) fn shl(a: &[Digit], bits: usize, out: &mut [Digit]) -> usize {
    let (whole, part) = (bits / DIGIT_BITS as usize, bits as u32 % DIGIT_BITS);
    let out_digits = &mut out[whole..=whole + a.len()];
    if part == 0 {
        out_digits[..a.len()].copy_from_slice(a);
    } else {
        let mut carry = 0;
        for (slot, &digit) in out_digits.iter_mut().zip(a) {
            *slot = (digit << part) | carry;
            carry = digit >> (DIGIT_BITS - part);
        }
        out_digits[a.len()] = carry;
    }
    normalized_len(out)
}

/// `a >> bits` into `out`, at least `a.len() - bits / DIGIT_BITS` long: the length, and
/// whether any one bit shifted out.
pub(crate) fn shr(a: &[Digit], bits: usize, out: &mut [Digit]) -> (usize, bool) {
    let (whole, part) = (bits / DIGIT_BITS as usize, bits as u32 % DIGIT_BITS);
    if whole >= a.len() {
        return (0, !a.is_empty());
    }
    let kept = &a[whole..];
    let mut lost = a[..whole].iter().any(|&digit| digit != 0);
    if part == 0 {
        out[..kept.len()].copy_from_slice(kept);
    } else {
        lost |= kept[0] << (DIGIT_BITS - part) != 0;
        for index in 0..kept.len() {
            let next = kept.get(index + 1).map_or(0, |&digit| digit << (DIGIT_BITS - part));
            out[index] = (kept[index] >> part) | next;
        }
    }
    (normalized_len(&out[..kept.len()]), lost)
}

/// Division of two-digit numerators by one normalized digit through its reciprocal.
struct Divisor {
    digit: Digit,
    reciprocal: Digit,
}

impl Divisor {
    /// `digit`, whose top bit is set: its reciprocal floor((B² − 1) / d) − B.
    fn new(digit: Digit) -> Self {
        debug_assert!(digit.leading_zeros() == 0);
        let reciprocal = (join(!digit, Digit::MAX) / Wide::from(digit)) as Digit;
        Self { digit, reciprocal }
    }

    /// (`high`:`low`) / d as quotient and remainder, `high < d`.
    #[inline(always)]
    fn div(&self, high: Digit, low: Digit) -> (Digit, Digit) {
        let (q0, q1) = split(Wide::from(self.reciprocal) * Wide::from(high) + join(high, low));
        let mut quotient = q1.wrapping_add(1);
        let mut remainder = low.wrapping_sub(quotient.wrapping_mul(self.digit));
        if remainder > q0 {
            quotient = quotient.wrapping_sub(1);
            remainder = remainder.wrapping_add(self.digit);
        }
        if remainder >= self.digit {
            quotient += 1;
            remainder -= self.digit;
        }
        (quotient, remainder)
    }
}

/// `a / b` into `quotient` (zeroed, at least `a.len() - b.len() + 1` long) and `a % b` into
/// `remainder` (at least `b.len()` long), `b` nonzero and `a >= b`: both lengths.
pub(crate) fn div_rem(
    a: &[Digit],
    b: &[Digit],
    quotient: &mut [Digit],
    remainder: &mut [Digit],
) -> (usize, usize) {
    let shift = b[b.len() - 1].leading_zeros();
    // Each shifted operand takes one more digit, which stays zero for the divisor.
    with_scratch(a.len() + b.len() + 2, |scratch| {
        let (u, v) = scratch.split_at_mut(a.len() + 1);
        shl(a, shift as usize, u);
        shl(b, shift as usize, v);
        let v = &v[..b.len()];
        if v.len() == 1 {
            let divisor = Divisor::new(v[0]);
            // The shift's carry digit is below the shifted divisor.
            let mut rest = u[a.len()];
            for index in (0..a.len()).rev() {
                (quotient[index], rest) = divisor.div(rest, u[index]);
            }
            remainder[0] = rest >> shift;
            return (normalized_len(&quotient[..a.len()]), usize::from(remainder[0] != 0));
        }
        let n = v.len();
        let (top, next) = (v[n - 1], v[n - 2]);
        let divisor = Divisor::new(top);
        for j in (0..=a.len() - n).rev() {
            let window = &mut u[j..=j + n];
            let mut estimate = Digit::MAX;
            if window[n] != top {
                let (q, mut rest) = divisor.div(window[n], window[n - 1]);
                estimate = q;
                while Wide::from(estimate) * Wide::from(next) > join(rest, window[n - 2]) {
                    estimate -= 1;
                    let (sum, overflow) = rest.overflowing_add(top);
                    rest = sum;
                    if overflow {
                        break;
                    }
                }
            }
            if estimate != 0 && sub_mul(window, v, estimate) {
                estimate -= 1;
                add_back(window, v);
            }
            quotient[j] = estimate;
        }
        let (remainder_len, _) = shr(&u[..n], shift as usize, remainder);
        (normalized_len(&quotient[..=a.len() - n]), remainder_len)
    })
}

/// `u -= v * factor`, `u.len() == v.len() + 1`: whether it borrowed.
fn sub_mul(u: &mut [Digit], v: &[Digit], factor: Digit) -> bool {
    let (low_digits, top) = u.split_at_mut(v.len());
    let mut carry: Digit = 0;
    let mut borrow = false;
    for (slot, &digit) in low_digits.iter_mut().zip(v) {
        let (low, high) = split(Wide::from(digit) * Wide::from(factor) + Wide::from(carry));
        carry = high;
        let (difference, first) = slot.overflowing_sub(low);
        let (difference, second) = difference.overflowing_sub(Digit::from(borrow));
        *slot = difference;
        borrow = first | second;
    }
    let (difference, first) = top[0].overflowing_sub(carry);
    let (difference, second) = difference.overflowing_sub(Digit::from(borrow));
    top[0] = difference;
    first | second
}

/// `u += v`, discarding the carry out of `u`'s top digit, which cancels `sub_mul`'s borrow.
fn add_back(u: &mut [Digit], v: &[Digit]) {
    let (low_digits, top) = u.split_at_mut(v.len());
    let mut carry = false;
    for (slot, &digit) in low_digits.iter_mut().zip(v) {
        let (sum, first) = slot.overflowing_add(digit);
        let (sum, second) = sum.overflowing_add(Digit::from(carry));
        *slot = sum;
        carry = first | second;
    }
    top[0] = top[0].wrapping_add(Digit::from(carry));
}
