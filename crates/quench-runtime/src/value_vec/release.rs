//! Releasing runs to the free lists, compacting the arena, and returning free pages.
use super::*;

impl ValueArena {
    #[inline(always)] pub(crate) fn release(&mut self, vector: ValueVec) {
        if !self.young_runs().hold(vector) {
            self.release_ordinary(vector);
        }
    }

    /// `release` of storage that is no young run: that of any record but a direct object
    /// (`young_run_padded`), whose sweeps skip the young range test. Storage that holds no
    /// arena words, as most arrays' and functions' does, is the inlined test alone, where the
    /// call cost a dead array 20 Ir (array-callback); a range or dictionary takes the call
    /// (`release_held`).
    #[inline(always)] pub(crate) fn release_ordinary(&mut self, vector: ValueVec) {
        debug_assert!(!self.holds_young_run(vector));
        // Inline storage and an empty vector without a range hold no arena words.
        if vector.has_inline_property_storage() || (vector.start == EMPTY_START && !vector.is_dictionary()) {
            return;
        }
        self.release_held(vector);
    }

    /// Links the released run at `start` into `bucket`'s list (`FreeRuns`).
    #[inline(always)]
    pub(super) fn push_free(&mut self, bucket: usize, start: u32) {
        let list = &mut self.free[bucket];
        self.values[start as usize] = Value::from_bits(u64::from(list.head));
        *list = FreeRuns { head: start, len: list.len + 1, unreleased: list.unreleased + 1 };
    }

    /// The last run released into `bucket`, unlinked.
    #[inline(always)]
    pub(super) fn pop_free(&mut self, bucket: usize) -> Option<u32> {
        let list = self.free[bucket];
        if list.is_empty() {
            return None;
        }
        let next = self.values[list.head as usize].to_bits() as u32;
        // The head is one of the `unreleased` runs at the front while there are any (`FreeRuns`).
        self.free[bucket] = FreeRuns { head: next, len: list.len - 1, unreleased: list.unreleased.saturating_sub(1) };
        Some(list.head)
    }

    /// `bucket`'s released runs, last released first.
    pub(super) fn free_starts(&self, bucket: usize) -> Vec<u32> {
        let mut starts = Vec::with_capacity(self.free[bucket].len());
        let mut start = self.free[bucket].head;
        for _ in 0..self.free[bucket].len {
            starts.push(start);
            start = self.values[start as usize].to_bits() as u32;
        }
        starts
    }

    #[inline(never)]
    pub(super) fn release_held(&mut self, vector: ValueVec) {
        if vector.is_dictionary() {
            self.release_dictionary(vector.dictionary_id());
            return;
        }
        let capacity = Self::ordered_capacity(vector, self.len(vector));
        if capacity != 0 {
            self.push_free(Self::bucket(capacity), vector.start & START_MASK);
            self.released_values += capacity;
        }
    }

    /// Whether released ranges have grown enough that compaction is worth rewriting every
    /// live object's storage.
    pub(crate) fn compaction_due(&self) -> bool {
        let idle_nursery = self.nursery.end - self.nursery.top;
        self.released_values > 0
            && self.released_values >= (self.values.len() - idle_nursery) / COMPACTION_RELEASED_SHARE_DIVISOR
    }

    /// Values held past the idle nursery and the released runs.
    pub(crate) fn occupied_values(&self) -> usize {
        let idle_nursery = self.nursery.end - self.nursery.top;
        (self.values.len() - idle_nursery).saturating_sub(self.released_values)
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
                let low = self.values[source + 2 * word].to_narrow().expect("checked narrow");
                let high = if 2 * word + 1 < length { self.values[source + 2 * word + 1].to_narrow().expect("checked narrow") } else { 0 };
                let (first, second) = if cfg!(target_endian = "little") { (low, high) } else { (high, low) };
                self.values[target + word] = Value::from_bits(u64::from(first) | u64::from(second) << 32);
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
        self.forget_reservations();
    }

    /// Gives back the whole pages inside each released run (`return_free_pages`'s contract).
    /// A run's first value links its free list and stays; the values after it are dead, so
    /// every page wholly inside the run is released, and a page that shares a value with a
    /// live range or another run is kept. A run already given back is not given back again
    /// (`FreeRuns::unreleased`).
    pub(crate) fn return_free_pages(&mut self) {
        let bytes = self.values.capacity() * size_of::<Value>();
        if bytes < FREE_PAGES_MIN_BYTES || self.released_values * 2 <= self.values.len() {
            return;
        }
        for bucket in 0..BUCKETS {
            let list = self.free[bucket];
            if list.unreleased == 0 {
                continue;
            }
            let capacity = Self::bucket_capacity(bucket);
            let mut start = list.head;
            for _ in 0..list.unreleased {
                let next = self.values[start as usize].to_bits() as u32;
                let first = start as usize + 1;
                self.values.release_pages(first, start as usize + capacity);
                start = next;
            }
            self.free[bucket].unreleased = 0;
        }
    }
}
