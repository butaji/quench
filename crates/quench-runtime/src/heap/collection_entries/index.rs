//! A Map's or Set's key index as two flat open-addressed arrays, by bucket: a tag byte
//! (seven bits of the key hash, or `EMPTY`) and the slot. Buckets are probed a group of
//! eight tags at a time, one word compared against the probe's tag, so a probe reads a slot
//! only on a tag match and a miss usually ends after one group with no data-dependent
//! branch. Slots and their hashes stay in the collection (`CollectionEntries`), which
//! rebuilds the index from the hashes, never reading a key. Nothing is ever unlinked, so a
//! key always sits in the first group, in probe order, that had an empty bucket when it was
//! linked, and a probe stops at the first group holding an empty bucket.

/// The tag of an empty bucket; a key's tag leaves this bit clear.
const EMPTY: u8 = 1 << (u8::BITS - 1);
/// Tags probed at once, one word's bytes.
const GROUP: usize = size_of::<u64>();
/// The low bit of every tag in a group word.
const LOW_BITS: u64 = u64::from_le_bytes([1; GROUP]);
/// The high bit of every tag in a group word: the empty ones there.
const HIGH_BITS: u64 = LOW_BITS * EMPTY as u64;
/// The fewest buckets an index starts with.
const MIN_BUCKETS: usize = 16;
/// Buckets per slot at least: groups rarely fill, so probes stay within one.
const BUCKETS_PER_SLOT: usize = 2;

#[derive(Clone, Debug)]
pub(super) struct KeyIndex {
    /// Per bucket, `EMPTY` or the hash's top seven bits; a power of two, at least
    /// `BUCKETS_PER_SLOT` per slot.
    tags: Vec<u8>,
    /// Per bucket, its slot.
    slots: Vec<u32>,
}

/// A hash's tag: its top bits, which the bucket position (its low bits) leaves out.
fn tag(hash: u32) -> u8 {
    (hash >> (u32::BITS - u8::BITS + 1)) as u8
}

/// The buckets of `bits`, set at the high bit of a group word's tags, from the group's start.
fn buckets_of(bits: u64) -> impl Iterator<Item = usize> {
    let mut remaining = bits;
    std::iter::from_fn(move || {
        (remaining != 0).then(|| {
            let bucket = remaining.trailing_zeros() / u8::BITS;
            remaining &= remaining - 1;
            bucket as usize
        })
    })
}

impl KeyIndex {
    /// The index of slots whose folded hashes are `hashes`.
    pub(super) fn of(hashes: &[u32]) -> Self {
        let buckets = (hashes.len() * BUCKETS_PER_SLOT).next_power_of_two().max(MIN_BUCKETS);
        let mut index = Self { tags: vec![EMPTY; buckets], slots: vec![0; buckets] };
        hashes.iter().enumerate().for_each(|(slot, &hash)| index.link(hash, slot));
        index
    }

    /// The groups' starts in probe order for `hash`, from the group holding its bucket.
    fn probe(&self, hash: u32) -> impl Iterator<Item = (usize, u64)> + '_ {
        let mask = self.tags.len() - 1;
        let first = hash as usize & mask & !(GROUP - 1);
        std::iter::successors(Some(first), move |start| Some((start + GROUP) & mask)).map(|start| {
            let word = self.tags[start..start + GROUP].try_into().map(u64::from_le_bytes);
            (start, word.expect("a group is a whole word"))
        })
    }

    fn link(&mut self, hash: u32, slot: usize) {
        let bucket = self
            .probe(hash)
            .find_map(|(start, group)| buckets_of(group & HIGH_BITS).next().map(|bucket| start + bucket))
            .expect("an index keeps empty buckets");
        self.tags[bucket] = tag(hash);
        self.slots[bucket] = u32::try_from(slot).expect("a collection's slots fit its index");
    }

    /// Indexes the last of `hashes`, the slot just appended; grows when the slots outgrow
    /// their share of the buckets.
    pub(super) fn push(&mut self, hashes: &[u32]) {
        match hashes {
            [.., last] if hashes.len() * BUCKETS_PER_SLOT <= self.tags.len() => self.link(*last, hashes.len() - 1),
            _ => *self = Self::of(hashes),
        }
    }

    /// The first slot whose tag agrees with `hash` and that `matches` accepts.
    #[inline(always)]
    pub(super) fn find(&self, hash: u32, mut matches: impl FnMut(usize) -> bool) -> Option<usize> {
        let repeated = LOW_BITS * u64::from(tag(hash));
        for (start, group) in self.probe(hash) {
            // A byte of `differing` is zero where the tags agree; the borrow trick can flag
            // a byte past a true match, which the slot's key then rejects.
            let differing = group ^ repeated;
            let agreeing = differing.wrapping_sub(LOW_BITS) & !differing & HIGH_BITS;
            let found = buckets_of(agreeing).map(|bucket| self.slots[start + bucket] as usize).find(|&slot| matches(slot));
            if found.is_some() || group & HIGH_BITS != 0 {
                return found;
            }
        }
        unreachable!("an index keeps empty buckets")
    }

    pub(super) fn bytes(&self) -> usize {
        self.tags.capacity() + self.slots.capacity() * size_of::<u32>()
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
