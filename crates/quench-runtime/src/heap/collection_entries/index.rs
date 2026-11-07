//! A Map's or Set's key index as flat arrays: per bucket the newest slot whose folded hash
//! lands there, and per slot the slot it chains to. Slots and their hashes stay in the
//! collection (`CollectionEntries`), so the index holds indices only and rebuilds from the
//! hashes, never reading a key.

/// No slot: an empty bucket, or the end of a chain.
const NONE: u32 = u32::MAX;
/// The fewest buckets an index starts with.
const MIN_BUCKETS: usize = 16;
/// Buckets per slot at least: chains average under one slot.
const BUCKETS_PER_SLOT: usize = 2;

#[derive(Clone, Debug)]
pub(super) struct KeyIndex {
    /// A power of two: at least twice the slots, so chains stay short.
    buckets: Vec<u32>,
    /// Per slot, the next slot of its bucket's chain.
    next: Vec<u32>,
}

fn slot_index(slot: usize) -> u32 {
    u32::try_from(slot).ok().filter(|&slot| slot != NONE).expect("a collection's slots fit its index")
}

impl KeyIndex {
    /// The index of slots whose folded hashes are `hashes`.
    pub(super) fn of(hashes: &[u32]) -> Self {
        let buckets = (hashes.len() * BUCKETS_PER_SLOT).next_power_of_two().max(MIN_BUCKETS);
        let mut index = Self { buckets: vec![NONE; buckets], next: Vec::with_capacity(hashes.len()) };
        for slot in 0..hashes.len() {
            index.link(hashes, slot);
        }
        index
    }

    fn link(&mut self, hashes: &[u32], slot: usize) {
        let bucket = hashes[slot] as usize & (self.buckets.len() - 1);
        self.next.push(self.buckets[bucket]);
        self.buckets[bucket] = slot_index(slot);
    }

    /// Indexes the last of `hashes`, the slot just appended; grows when the slots outgrow
    /// half the buckets.
    pub(super) fn push(&mut self, hashes: &[u32]) {
        if hashes.len() * BUCKETS_PER_SLOT > self.buckets.len() {
            *self = Self::of(hashes);
        } else {
            self.link(hashes, hashes.len() - 1);
        }
    }

    /// The first slot of `hash`'s chain whose hash is `hash` and that `matches` accepts.
    #[inline(always)]
    pub(super) fn find(&self, hashes: &[u32], hash: u32, mut matches: impl FnMut(usize) -> bool) -> Option<usize> {
        let mut slot = self.buckets[hash as usize & (self.buckets.len() - 1)];
        while slot != NONE {
            let at = slot as usize;
            if hashes[at] == hash && matches(at) {
                return Some(at);
            }
            slot = self.next[at];
        }
        None
    }

    pub(super) fn bytes(&self) -> usize {
        (self.buckets.capacity() + self.next.capacity()) * size_of::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::super::CollectionEntries;
    use crate::value::Value;

    fn key(n: i32) -> Value {
        Value::integer(n)
    }

    /// Finds `n` by identity, with the hash `hash` gives it.
    fn find(entries: &CollectionEntries<Value>, n: i32, hash: impl Fn(i32) -> u64) -> Option<usize> {
        entries.position(key(n), hash(n), |_| false)
    }

    #[test]
    fn index_finds_every_key_through_growth_deletes_and_compaction() {
        // A constant hash puts every key in one chain; a spread one exercises the buckets.
        for hash in [(|_| 7) as fn(i32) -> u64, |n| (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)] {
            let mut entries = CollectionEntries::default();
            for n in 0..200 {
                assert_eq!(find(&entries, n, hash), None);
                entries.push(key(n), hash(n));
                assert_eq!(find(&entries, n, hash), Some(n as usize));
            }
            let mut compacted = false;
            for n in (0..200).filter(|n| n % 3 != 0) {
                let slot = find(&entries, n, hash).expect("a pushed key is found");
                compacted |= entries.remove(slot).is_some();
                assert_eq!(find(&entries, n, hash), None);
            }
            assert!(compacted);
            for n in 0..200 {
                let found = find(&entries, n, hash).map(|slot| entries.get(slot));
                assert_eq!(found, (n % 3 == 0).then_some(Some(key(n))));
            }
            entries.clear();
            assert_eq!((entries.len(), find(&entries, 0, hash)), (0, None));
        }
    }

    #[test]
    fn a_clear_during_a_walk_leaves_holes_the_index_skips() {
        let mut entries = CollectionEntries::default();
        (0..20).for_each(|n| entries.push(key(n), n as u64));
        entries.begin_walk();
        entries.clear();
        assert_eq!(entries.position(key(3), 3, |_| false), None);
        entries.push(key(3), 3);
        assert_eq!(entries.position(key(3), 3, |_| false), Some(20));
        entries.end_walk();
    }
}
