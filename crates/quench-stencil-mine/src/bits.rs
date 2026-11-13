//! A fixed-size set of small indices, one bit each.

const WORD_BITS: usize = u64::BITS as usize;

pub struct Bits(Vec<u64>);

impl Bits {
    /// An empty set of indices below `len`.
    pub fn new(len: usize) -> Self {
        Self(vec![0; len.div_ceil(WORD_BITS)])
    }

    pub fn insert(&mut self, index: usize) {
        self.0[index / WORD_BITS] |= 1 << (index % WORD_BITS);
    }

    pub fn contains(&self, index: usize) -> bool {
        self.0.get(index / WORD_BITS).is_some_and(|word| word & (1 << (index % WORD_BITS)) != 0)
    }

    /// The members, ascending.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(base, &word)| {
            (0..WORD_BITS).filter(move |bit| word >> bit & 1 != 0).map(move |bit| base * WORD_BITS + bit)
        })
    }
}
