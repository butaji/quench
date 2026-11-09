//! A fixed-width set of registers: [`RegisterSet::BITS`] bits, one per register.
//!
//! Liveness, root maps (the stack map a native frame recovers through) and move
//! coalescing carry this set in registers. Its width bounds the register file
//! those stages cover; a wider body keeps interpreter frames instead.

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, Not, Shl, Shr, Sub};

/// The number of 64-bit words a set holds.
const WORDS: usize = 8;
/// The words one `u128` spans, the width a half mask keeps.
const HALF_WORDS: usize = (u128::BITS / u64::BITS) as usize;

/// A set of registers, one bit each, `u64::BITS * WORDS` wide.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RegisterSet([u64; WORDS]);

impl RegisterSet {
    /// The registers a set covers.
    pub(crate) const BITS: u32 = (u64::BITS as usize * WORDS) as u32;
    /// Every covered register.
    pub(crate) const MAX: Self = Self([u64::MAX; WORDS]);

    /// The set of `register` alone; empty past the covered width.
    #[inline(always)]
    pub(crate) fn bit(register: usize) -> Self {
        let mut set = Self::default();
        set.insert(register);
        set
    }

    /// Whether `register` is in the set.
    #[inline(always)]
    pub(crate) fn contains(self, register: usize) -> bool {
        let Some(word) = self.0.get(register / u64::BITS as usize) else {
            return false;
        };
        word & (1 << (register % u64::BITS as usize)) != 0
    }

    /// Adds `register`; past the covered width the set is unchanged.
    #[inline(always)]
    pub(crate) fn insert(&mut self, register: usize) {
        if let Some(word) = self.0.get_mut(register / u64::BITS as usize) {
            *word |= 1 << (register % u64::BITS as usize);
        }
    }

    /// The set shifted left by `bits`; `None` when a set bit would leave the width.
    #[inline(always)]
    pub(crate) fn checked_shl(self, bits: u32) -> Option<Self> {
        if bits >= Self::BITS { return self.is_empty().then_some(Self::default()); }
        let shifted = self << bits;
        (shifted >> bits == self).then_some(shifted)
    }

    /// Whether no register is in the set.
    #[inline(always)]
    pub(crate) fn is_empty(self) -> bool {
        self.0.iter().all(|word| *word == 0)
    }

    /// Bits set in the set.
    #[inline(always)]
    pub(crate) fn count_ones(self) -> u32 {
        self.0.iter().map(|word| word.count_ones()).sum()
    }

    /// The lowest set bit, or the width when the set is empty.
    #[inline(always)]
    pub(crate) fn trailing_zeros(self) -> u32 {
        for (index, word) in self.0.iter().enumerate() {
            if *word != 0 {
                return index as u32 * u64::BITS + word.trailing_zeros();
            }
        }
        Self::BITS
    }

    /// Word `index` of the set, the unit a register run is cleared a machine word at a time.
    #[inline(always)]
    pub(crate) fn word(self, index: usize) -> u64 {
        self.0.get(index).copied().unwrap_or(0)
    }

    /// The set as one `u128`, when no register past 128 is in it.
    #[inline(always)]
    pub(crate) fn as_u128(self) -> Option<u128> {
        (self.0[HALF_WORDS..].iter().all(|word| *word == 0)).then(||
            u128::from(self.0[0]) | (u128::from(self.0[1]) << u64::BITS))
    }
}

impl From<u64> for RegisterSet {
    #[inline(always)]
    fn from(word: u64) -> Self {
        let mut set = Self::default();
        set.0[0] = word;
        set
    }
}

impl From<u128> for RegisterSet {
    #[inline(always)]
    fn from(bits: u128) -> Self {
        let mut set = Self::default();
        set.0[0] = bits as u64;
        set.0[1] = (bits >> u64::BITS) as u64;
        set
    }
}

impl TryFrom<RegisterSet> for u64 {
    type Error = ();
    #[inline(always)]
    fn try_from(set: RegisterSet) -> Result<Self, Self::Error> {
        (set.0[1..].iter().all(|word| *word == 0)).then_some(set.0[0]).ok_or(())
    }
}

impl std::fmt::Debug for RegisterSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.0.iter().rev()).finish()
    }
}

impl PartialEq<u32> for RegisterSet {
    #[inline(always)]
    fn eq(&self, other: &u32) -> bool {
        *other == 0 && self.is_empty()
    }
}

impl BitOr for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn bitor(self, other: Self) -> Self {
        let mut words = self.0;
        for (word, other) in words.iter_mut().zip(other.0) { *word |= other; }
        Self(words)
    }
}

impl BitAnd for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn bitand(self, other: Self) -> Self {
        let mut words = self.0;
        for (word, other) in words.iter_mut().zip(other.0) { *word &= other; }
        Self(words)
    }
}

impl BitXor for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn bitxor(self, other: Self) -> Self {
        let mut words = self.0;
        for (word, other) in words.iter_mut().zip(other.0) { *word ^= other; }
        Self(words)
    }
}

impl Not for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn not(self) -> Self {
        let mut words = self.0;
        for word in &mut words { *word = !*word; }
        Self(words)
    }
}

impl BitOrAssign for RegisterSet {
    #[inline(always)]
    fn bitor_assign(&mut self, other: Self) { *self = *self | other; }
}

impl BitAndAssign for RegisterSet {
    #[inline(always)]
    fn bitand_assign(&mut self, other: Self) { *self = *self & other; }
}

impl Sub<u32> for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn sub(self, bits: u32) -> Self {
        let mut words = self.0;
        let mut borrow = u64::from(bits);
        for word in &mut words {
            let (value, underflow) = word.overflowing_sub(borrow);
            *word = value;
            borrow = u64::from(underflow);
        }
        Self(words)
    }
}

impl Shr<u32> for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn shr(self, bits: u32) -> Self {
        if bits >= Self::BITS { return Self::default(); }
        let word_shift = (bits / u64::BITS) as usize;
        let bit_shift = bits % u64::BITS;
        let mut shifted = [0_u64; WORDS];
        for (index, slot) in shifted.iter_mut().enumerate().take(WORDS - word_shift) {
            let source = index + word_shift;
            *slot = self.0[source] >> bit_shift;
            if bit_shift != 0 && source + 1 < WORDS {
                *slot |= self.0[source + 1] << (u64::BITS - bit_shift);
            }
        }
        Self(shifted)
    }
}

impl Shl<u32> for RegisterSet {
    type Output = Self;
    #[inline(always)]
    fn shl(self, bits: u32) -> Self {
        if bits >= Self::BITS { return Self::default(); }
        let word_shift = (bits / u64::BITS) as usize;
        let bit_shift = bits % u64::BITS;
        let mut shifted = [0_u64; WORDS];
        for (index, slot) in shifted.iter_mut().enumerate().skip(word_shift) {
            let source = index - word_shift;
            *slot = self.0[source] << bit_shift;
            if bit_shift != 0 && source > 0 {
                *slot |= self.0[source - 1] >> (u64::BITS - bit_shift);
            }
        }
        Self(shifted)
    }
}

#[cfg(test)]
mod tests {
    use super::RegisterSet;

    #[test]
    fn a_set_spans_eight_words_and_shifts_within_them() {
        assert_eq!(RegisterSet::BITS, 512);
        for register in [0, 63, 64, 127, 128, 255, 256, 511] {
            let set = RegisterSet::bit(register);
            assert!(set.contains(register));
            assert!(!set.contains(register + 1));
        }
        assert_eq!(RegisterSet::bit(200).checked_shl(400).map(|set| set.contains(600)), None);
        assert!(RegisterSet::bit(8).checked_shl(300).is_some_and(|set| set.contains(308)));
        assert!((RegisterSet::MAX >> 4).contains(507));
        assert_eq!(u64::try_from(RegisterSet::bit(63)).map(|word| word.count_ones()), Ok(1));
        assert!(u64::try_from(RegisterSet::bit(64)).is_err());
        assert_eq!(RegisterSet::from(u128::MAX).as_u128(), Some(u128::MAX));
        assert_eq!(RegisterSet::bit(255).as_u128(), None);
    }
}
