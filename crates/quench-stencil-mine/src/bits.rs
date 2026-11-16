//! A fixed-size set of small indices, one bit each.

/// Indices one word of the set holds.
const MEMBERS_PER_WORD: usize = u64::BITS as usize;

pub struct Bits(Vec<u64>);

impl Bits {
    /// An empty set of indices below `len`.
    pub fn new(len: usize) -> Self {
        Self(vec![0; len.div_ceil(MEMBERS_PER_WORD)])
    }

    pub fn insert(&mut self, index: usize) {
        self.0[index / MEMBERS_PER_WORD] |= 1 << (index % MEMBERS_PER_WORD);
    }

    pub fn contains(&self, index: usize) -> bool {
        self.0.get(index / MEMBERS_PER_WORD).is_some_and(|word| word & (1 << (index % MEMBERS_PER_WORD)) != 0)
    }

    /// The members, ascending.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(base, &word)| {
            (0..MEMBERS_PER_WORD).filter(move |bit| word >> bit & 1 != 0).map(move |bit| base * MEMBERS_PER_WORD + bit)
        })
    }
}
