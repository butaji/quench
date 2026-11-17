//! The escaped strings and templates of the lexed tokens, by token start, kept in source order:
//! a lazily parsed body asks whether one lies past it and drops the ones inside it, which a
//! hash map answered by visiting every entry, once per body (quadratic in a bundle).

/// Values by token start, sorted by it. Tokens are lexed in source order, so an insert is a push,
/// unless a rewind lexes one again.
#[derive(Debug)]
pub struct Escaped<V> {
    entries: Vec<(u32, V)>,
}

impl<V> Default for Escaped<V> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<V: Copy> Escaped<V> {
    /// Notes `value` for the token at `start`, replacing one lexed there before.
    pub fn insert(&mut self, start: u32, value: V) {
        match self.entries.last() {
            Some((last, _)) if *last >= start => match self.entries.binary_search_by_key(&start, |(key, _)| *key) {
                Ok(at) => self.entries[at].1 = value,
                Err(at) => self.entries.insert(at, (start, value)),
            },
            _ => self.entries.push((start, value)),
        }
    }

    /// The value of the token at `start`.
    ///
    /// # Panics
    /// Where no token at `start` was escaped.
    pub fn get(&self, start: u32) -> V {
        let at = self.entries.binary_search_by_key(&start, |(key, _)| *key).expect("an escaped token");
        self.entries[at].1
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether a token at or past `at` is listed.
    pub fn any_from(&self, at: u32) -> bool {
        self.entries.last().is_some_and(|(last, _)| *last >= at)
    }

    /// Drops the tokens starting in `range`.
    pub fn remove_range(&mut self, range: std::ops::Range<u32>) {
        let from = self.entries.partition_point(|(key, _)| *key < range.start);
        let to = self.entries.partition_point(|(key, _)| *key < range.end);
        self.entries.drain(from..to);
    }

    /// Keeps only the tokens starting before `end`.
    pub fn truncate_before(&mut self, end: u32) {
        let keep = self.entries.partition_point(|(key, _)| *key < end);
        self.entries.truncate(keep);
    }
}
