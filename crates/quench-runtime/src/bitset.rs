//! Sets of small indices, one bit each: the one bitset every analysis and table shares.
//!
//! `BitSet` keeps two words inline, so a set of up to 128 members (most frames' registers,
//! most functions' slots) allocates nothing; `BitRows` keeps one equal-width row per program
//! point in a single flat buffer, for dataflow over blocks or instructions.

use smallvec::SmallVec;

const WORD_BITS: usize = u64::BITS as usize;
/// Words kept inline: 128 members without allocation.
const INLINE_WORDS: usize = 2;

#[inline(always)]
fn position(index: usize) -> (usize, u64) {
    (index / WORD_BITS, 1 << (index % WORD_BITS))
}

/// Whether bit `index` of the membership words `words` (`BitSet::words`) is set; false past
/// their end. For a set stored as its words alone, as a compact record keeps one.
#[inline(always)]
pub(crate) fn words_contain(words: &[u64], index: usize) -> bool {
    let (word, bit) = position(index);
    words.get(word).is_some_and(|&word| word & bit != 0)
}

fn words_for(len: usize) -> usize {
    len.div_ceil(WORD_BITS)
}

/// A set of indices below `len`, which `grow` raises.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BitSet {
    words: SmallVec<[u64; INLINE_WORDS]>,
    len: usize,
}

impl BitSet {
    /// An empty set of indices below `len`.
    pub(crate) fn new(len: usize) -> Self {
        Self { words: smallvec::smallvec![0; words_for(len)], len }
    }

    /// The set of indices below `len` that `member` accepts.
    pub(crate) fn from_fn(len: usize, mut member: impl FnMut(usize) -> bool) -> Self {
        let mut set = Self::new(len);
        (0..len).filter(|&index| member(index)).for_each(|index| set.words[index / WORD_BITS] |= position(index).1);
        set
    }

    /// The bound every member stays below.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.words.iter().all(|&word| word == 0)
    }

    #[inline(always)]
    pub(crate) fn contains(&self, index: usize) -> bool {
        let (word, bit) = position(index);
        index < self.len && self.words[word] & bit != 0
    }

    /// Adds `index`, which must be below `len`; whether it was absent.
    #[inline(always)]
    pub(crate) fn insert(&mut self, index: usize) -> bool {
        debug_assert!(index < self.len, "bit {index} of a set of {}", self.len);
        let (word, bit) = position(index);
        let absent = self.words[word] & bit == 0;
        self.words[word] |= bit;
        absent
    }

    /// Adds `index` when it is below `len`; `None` (nothing added) otherwise.
    pub(crate) fn checked_insert(&mut self, index: usize) -> Option<bool> {
        (index < self.len).then(|| self.insert(index))
    }

    /// Adds `index`, first raising `len` past it when needed; whether it was absent.
    pub(crate) fn insert_growing(&mut self, index: usize) -> bool {
        if index >= self.len {
            self.grow(index + 1);
        }
        self.insert(index)
    }

    #[inline(always)]
    pub(crate) fn remove(&mut self, index: usize) {
        if index < self.len {
            let (word, bit) = position(index);
            self.words[word] &= !bit;
        }
    }

    /// Raises the bound to `len`; new positions are not members.
    pub(crate) fn grow(&mut self, len: usize) {
        if len > self.len {
            self.words.resize(words_for(len), 0);
            self.len = len;
        }
    }

    /// Removes every member, keeping the bound.
    pub(crate) fn clear(&mut self) {
        self.words.fill(0);
    }

    /// Adds every member of `other`; whether this set grew.
    pub(crate) fn union_with(&mut self, other: &Self) -> bool {
        let mut grew = false;
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            let next = *word | other;
            grew |= next != *word;
            *word = next;
        }
        grew
    }

    /// Removes every member of `other`.
    pub(crate) fn subtract(&mut self, other: &Self) {
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            *word &= !other;
        }
    }

    /// The members, ascending.
    pub(crate) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(index, &word)| {
            let mut remaining = word;
            std::iter::from_fn(move || {
                (remaining != 0).then(|| {
                    let bit = remaining.trailing_zeros() as usize;
                    remaining &= remaining - 1;
                    index * WORD_BITS + bit
                })
            })
        })
    }

    /// The membership words, lowest index first.
    pub(crate) fn words(&self) -> &[u64] {
        &self.words
    }

    pub(crate) fn words_mut(&mut self) -> &mut [u64] {
        &mut self.words
    }

    /// Whether the words spilled to the heap.
    #[cfg(test)]
    pub(crate) fn spilled(&self) -> bool {
        self.words.spilled()
    }
}

/// One equal-width row of bits per point, in one flat buffer.
pub(crate) struct BitRows {
    words: usize,
    bits: Vec<u64>,
}

impl BitRows {
    /// `points` empty rows of `len` bits each.
    pub(crate) fn new(points: usize, len: usize) -> Self {
        let words = words_for(len);
        Self { words, bits: vec![0; points * words] }
    }

    pub(crate) fn row(&self, point: usize) -> &[u64] {
        &self.bits[point * self.words..(point + 1) * self.words]
    }

    /// Joins `state` into `point`'s row; whether the row grew.
    pub(crate) fn join(&mut self, point: usize, state: &[u64]) -> bool {
        let row = &mut self.bits[point * self.words..(point + 1) * self.words];
        let mut grew = false;
        for (word, incoming) in row.iter_mut().zip(state) {
            let next = *word | incoming;
            grew |= next != *word;
            *word = next;
        }
        grew
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_sets_stay_inline_and_iterate_in_order() {
        let mut set = BitSet::new(128);
        assert!(!set.spilled());
        for index in [127, 0, 64, 63] {
            assert!(set.insert(index));
        }
        assert!(!set.insert(64));
        assert_eq!(set.iter().collect::<Vec<_>>(), [0, 63, 64, 127]);
        set.remove(63);
        assert!(!set.contains(63) && set.contains(127) && !set.contains(500));
        assert_eq!(set.checked_insert(128), None);
    }

    #[test]
    fn growing_and_predicates_round_trip() {
        let mut set = BitSet::default();
        assert!(set.insert_growing(300));
        assert!(set.spilled() && set.len() == 301 && set.contains(300));
        assert!(words_contain(set.words(), 300) && !words_contain(set.words(), 301) && !words_contain(&[], 0));
        let flags = [true, false, false, true];
        let set = BitSet::from_fn(4, |index| flags[index]);
        assert_eq!((0..4).map(|index| set.contains(index)).collect::<Vec<_>>(), flags);
    }

    #[test]
    fn set_algebra_reports_growth() {
        let mut left = BitSet::from_fn(70, |index| index % 2 == 0);
        let right = BitSet::from_fn(70, |index| index % 3 == 0);
        assert!(left.union_with(&right));
        assert!(!left.union_with(&right));
        left.subtract(&right);
        assert!(left.contains(2) && !left.contains(3) && !left.contains(6));
        let mut rows = BitRows::new(2, 70);
        assert!(rows.join(1, right.words()));
        assert!(!rows.join(1, right.words()));
        assert_eq!(rows.row(1), right.words());
        assert!(rows.row(0).iter().all(|&word| word == 0));
    }
}
