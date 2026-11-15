//! Compaction: sliding live runs down to free the arena's tail, and narrowing a run's values into words
//! where they fit (`ValueArena::compact_vector`).

use super::*;

impl ValueArena {
    /// Whether released ranges have grown enough that compaction is worth rewriting every
    /// live object's storage.
    pub(crate) fn compaction_due(&self) -> bool {
        let idle_nursery = self.nursery.end - self.nursery.top;
        self.released_values > 0
            && self.released_values >= (self.values.len() - idle_nursery) / COMPACTION_RELEASED_SHARE_DIVISOR
    }

    /// Records what a full collection left (`GenerationalPacing::arena_growth_reached`).
    pub(crate) fn settle_after_full(&mut self) {
        self.settled_values = self.occupied_values();
    }

    /// The values the last full collection left held (`settle_after_full`).
    pub(crate) fn settled_values(&self) -> usize {
        self.settled_values
    }

    pub(crate) fn has_compact_range(&self, vector: ValueVec) -> bool {
        vector.has_compact_range()
    }

    /// Slides `vector`'s run down to `target` and answers the words it now takes. Where
    /// `narrows` (a plain object's storage) and every value has a narrow form, the run becomes
    /// a narrow one (`NARROW_FORM`), half the words; its owner stores the vector back.
    pub(crate) fn compact_vector(&mut self, vector: &mut ValueVec, target: usize, narrows: bool) -> usize {
        assert!(
            !vector.is_dictionary(),
            "dictionary storage is not arena-backed"
        );
        let length = self.len(*vector);
        let source = vector.start();
        assert!(
            target <= source,
            "property ranges must compact in source order"
        );
        if narrows && !vector.is_narrow() && length > 0 && self.narrows_to_fewer_words(*vector, length) {
            let mut narrow = *vector;
            narrow.auxiliary |= NARROW_FORM;
            let capacity = Self::ordered_capacity(narrow, length);
            // Words are written in order, each from values at or past it: word `k` reads
            // values `2k` and `2k + 1` of a source at or past the target.
            for word in 0..narrow_words(length) {
                self.values[target + word] = self.packed_narrow_word(source, length, word);
            }
            self.values[target + narrow_words(length)..target + capacity].fill(Value::UNDEFINED);
            narrow.relocate(target);
            *vector = narrow;
            self.narrowed_words += capacity;
            return capacity;
        }
        let capacity = self.capacity(*vector);
        let words = if vector.is_narrow() { narrow_words(length) } else { length };
        if vector.is_narrow() { self.narrowed_words += capacity; }
        self.values.copy_within(source..source + words, target);
        self.values[target + words..target + capacity].fill(Value::UNDEFINED);
        vector.relocate(target);
        capacity
    }

    /// Whether `length` values fill a smaller class as a narrow run (census).
    pub(crate) fn narrow_class_smaller(&self, length: usize) -> bool {
        capacity_class(narrow_words(length)) < capacity_class(length)
    }

    /// Whether the `length` values at `source` all have a narrow form and fill a smaller
    /// class as a narrow run than as values.
    pub(super) fn narrows_to_fewer_words(&self, vector: ValueVec, length: usize) -> bool {
        let source = vector.start();
        capacity_class(narrow_words(length)) < capacity_class(length)
            && !self.widened_shapes.contains(&vector.auxiliary())
            && self.values[source..source + length].iter().all(|value| value.to_narrow().is_some())
    }

    pub(crate) fn finish_compaction(&mut self, length: usize) {
        self.values.truncate(length);
        if crate::trace::Channel::Gc.on() {
            // Narrow-run words hold value pairs, not values: the census counts them apart and
            // leaves them out of the wide counts only in sum (they read as numbers).
            let wide = self.values[..length].iter().filter(|value| value.to_narrow().is_none()).count();
            let numbers = self.values[..length].iter().filter(|value| value.to_narrow().is_none() && value.as_number().is_some()).count();
            eprintln!("arena census: compacted={length} narrow_run_words={} wide={wide} wide_numbers={numbers} (narrow-run words included)", self.narrowed_words);
        }
        self.narrowed_words = 0;
        // A burst's growth stays resident past the compaction that frees it: keep the room
        // the next doubling would give the live values, and return the rest.
        self.values.shrink_to(length.saturating_mul(COMPACTED_CAPACITY_FACTOR));
        self.free = [FreeRuns::EMPTY; BUCKETS];
        self.released_values = 0;
        // Compaction moved every live run below `length`, young ones too.
        self.nursery = Nursery::default();
    }

    /// The narrow words of the `length` values at `source`, each of which has a narrow form.
    pub(super) fn narrow_words_of(&self, source: usize, length: usize) -> smallvec::SmallVec<[Value; 16]> {
        (0..narrow_words(length)).map(|word| self.packed_narrow_word(source, length, word)).collect()
    }

    /// Word `word` of the narrow run of the `length` values at `source`: its first two values
    /// packed, the second absent past `length`. Each value has a narrow form.
    #[inline(always)]
    pub(super) fn packed_narrow_word(&self, source: usize, length: usize, word: usize) -> Value {
        let slot = NARROW_PER_WORD * word;
        let low = self.values[source + slot].to_narrow().expect("checked narrow");
        let high = if slot + 1 < length { self.values[source + slot + 1].to_narrow().expect("checked narrow") } else { 0 };
        let (first, second) = if cfg!(target_endian = "little") { (low, high) } else { (high, low) };
        Value::from_bits(u64::from(first) | u64::from(second) << u32::BITS)
    }
}
